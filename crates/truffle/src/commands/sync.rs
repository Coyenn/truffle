use crate::assets::{
    augment_assets, build_atlased_assets, build_atlases, load_assets, render_dts_module,
    render_luau_module, AtlasExclude, AtlasOptions, FsImageMetadata,
};
use crate::commands::image::HighlightArgs;
use anyhow::Context;
use asphalt::{
    cli::{SyncArgs as AsphaltSyncArgs, SyncTarget},
    config::{Config as AsphaltConfig, Input as AsphaltInput},
    glob::Glob,
    sync, sync_with_config,
};
use clap::Parser;
use indicatif::MultiProgress;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use tokio::runtime::Runtime;
use truffle_config::TruffleConfig;

#[derive(Parser)]
#[command(about = "Sync assets and augment metadata with image dimensions")]
pub struct SyncArgs {
    /// Path to the Luau assets module file
    #[arg(long, default_value = "src/shared/data/assets/assets.luau")]
    pub assets_input: PathBuf,

    /// Path to write the augmented Luau assets module
    #[arg(long, default_value = "src/shared/data/assets/assets.luau")]
    pub assets_output: PathBuf,

    /// Path to write the TypeScript declaration file
    #[arg(long, default_value = "src/shared/data/assets/assets.d.ts")]
    pub dts_output: PathBuf,

    /// Path to the raw assets images folder
    #[arg(long, default_value = "assets/images")]
    pub images_folder: PathBuf,

    /// Pack images into atlas textures before syncing
    #[arg(long)]
    pub atlas: bool,

    /// Atlas texture size (power-of-two square)
    #[arg(long)]
    pub atlas_size: Option<u32>,

    /// Padding (in pixels) around each sprite in the atlas
    #[arg(long)]
    pub atlas_padding: Option<u32>,

    /// Image keys to exclude from atlas packing (repeatable)
    #[arg(long)]
    pub atlas_exclude: Vec<String>,

    /// Write outputs without syncing to Roblox
    #[arg(long)]
    pub dry_run: bool,

    /// Scratch directory for intermediate/generated files
    #[arg(long)]
    pub scratch_dir: Option<PathBuf>,

    /// TRUFFLE_API_KEY environment variable (or read from .env file)
    #[arg(long)]
    pub api_key: Option<String>,
}

pub fn run(args: SyncArgs) -> bool {
    let rt = Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
        match run_async(args).await {
            Ok(()) => true,
            Err(e) => {
                eprintln!("[sync] ERROR: {}", e);
                false
            }
        }
    })
}

async fn run_async(args: SyncArgs) -> anyhow::Result<()> {
    // Load truffle.toml config
    let config = TruffleConfig::read()
        .await
        .context("Failed to read truffle.toml. Make sure it exists in the current directory.")?;

    let scratch_dir = args
        .scratch_dir
        .clone()
        .unwrap_or_else(|| config.truffle.scratch_dir.clone());

    // Auto-generate highlights if configured (before sync so they get synced too)
    if config.truffle.auto_highlight {
        println!("[sync] Generating highlight variants …");
        let highlight_args = HighlightArgs {
            input_path: args.images_folder.clone(),
            dry_run: false,
            force: config.truffle.highlight_force,
            thickness: config.truffle.highlight_thickness,
            recursive: true,
        };
        crate::commands::image::run(crate::commands::image::ImageCommands::Highlight(
            highlight_args,
        ));
    }

    let atlas_enabled = args.atlas || config.truffle.atlas;
    if atlas_enabled {
        println!("[sync] Building image atlases …");
        let atlas_dir = scratch_dir.join("atlases");
        let atlas_codegen_dir = scratch_dir.join("asphalt");
        // Asphalt codegen writes `{input_name}.luau`. Our atlas input is named `atlases`.
        let atlas_assets_output = atlas_codegen_dir.join("atlases.luau");
        let atlas_padding = args.atlas_padding.unwrap_or(config.truffle.atlas_padding);
        let atlas_size = args.atlas_size.unwrap_or(config.truffle.atlas_size);
        let atlas_exclude = resolve_atlas_exclude(
            &args.atlas_exclude,
            &config.truffle.atlas_exclude,
            &args.images_folder,
        );
        let atlas_exclude_matcher = build_atlas_exclude(&atlas_exclude)?;

        let placements = build_atlases(
            &args.images_folder,
            &atlas_dir,
            AtlasOptions {
                padding: atlas_padding,
                size: atlas_size,
                exclude: atlas_exclude_matcher.clone(),
            },
        )
        .context("Failed to build atlases")?;

        std::fs::create_dir_all(&atlas_codegen_dir).ok();

        let excluded_assets_output = if atlas_exclude.is_empty() {
            None
        } else {
            Some(atlas_codegen_dir.join("excluded.luau"))
        };

        if !args.dry_run {
            // Resolve API key (TRUFFLE_API_KEY instead of ASPHALT_API_KEY)
            let api_key = resolve_api_key(args.api_key.clone())?;

            let mut asphalt_config = AsphaltConfig::read_from(PathBuf::from("."))
                .await
                .context("Failed to read Asphalt config from truffle.toml")?;

            // Ensure atlas file names are preserved as keys.
            asphalt_config.codegen.strip_extensions = false;
            asphalt_config.inputs = {
                let mut inputs = HashMap::new();

                let atlas_glob = format!("{}/**/*.png", atlas_dir.display());
                inputs.insert(
                    "atlases".to_string(),
                    AsphaltInput {
                        include: Glob::new(atlas_glob.as_str())
                            .context("Invalid atlas include glob")?,
                        output_path: atlas_codegen_dir.clone(),
                        bleed: false,
                        web: HashMap::new(),
                    },
                );

                if let Some(excluded_assets_output) = &excluded_assets_output {
                    if let Some(exclude_glob) =
                        build_exclude_glob(&args.images_folder, &atlas_exclude)
                    {
                        inputs.insert(
                            "excluded".to_string(),
                            AsphaltInput {
                                include: Glob::new(exclude_glob.as_str())
                                    .context("Invalid atlas exclude glob")?,
                                output_path: excluded_assets_output
                                    .parent()
                                    .map(PathBuf::from)
                                    .unwrap_or_else(|| atlas_codegen_dir.clone()),
                                bleed: false,
                                web: HashMap::new(),
                            },
                        );
                    }
                }

                inputs
            };

            // Run Asphalt sync on the generated atlas PNGs
            println!("[sync] Running backend sync …");
            let multi_progress = MultiProgress::new();
            let sync_args = AsphaltSyncArgs {
                api_key: Some(api_key),
                target: Some(SyncTarget::Cloud { dry_run: false }),
                expected_price: None,
                project: PathBuf::from("."),
            };

            sync_with_config(asphalt_config, sync_args, multi_progress)
                .await
                .context("Failed to sync atlases with Asphalt")?;
        }

        // Load atlas asset ids produced by Asphalt
        let atlas_ids = if atlas_assets_output.exists() {
            let atlas_assets = load_assets(&atlas_assets_output)
                .map_err(|e| anyhow::anyhow!("Failed to load atlas assets: {}", e))?;
            atlas_file_ids_from_assets(&atlas_assets)
        } else {
            HashMap::new()
        };

        let mut atlas_ids = atlas_ids;
        if atlas_ids.is_empty() {
            // In dry-run or missing output, fill placeholder ids so we can still write modules.
            for placement in placements.values() {
                atlas_ids
                    .entry(placement.atlas_file_name.clone())
                    .or_insert_with(|| "rbxassetid://0".into());
            }
        }

        // Build the final assets tree keyed by original image paths
        let mut final_assets = build_atlased_assets(&placements, &atlas_ids)
            .context("Failed to build atlased asset metadata")?;

        if !atlas_exclude.is_empty() {
            let excluded_assets = match excluded_assets_output.as_ref() {
                Some(path) if path.exists() => load_assets(path)
                    .map_err(|e| anyhow::anyhow!("Failed to load excluded assets: {}", e))?,
                _ => load_assets(&args.assets_input)
                    .map_err(|e| anyhow::anyhow!("Failed to load assets: {}", e))?,
            };

            let filtered_excluded =
                filter_assets_by_exclude(&excluded_assets, &atlas_exclude_matcher);
            let augmented_excluded =
                augment_assets(&filtered_excluded, &args.images_folder, &FsImageMetadata);
            merge_asset_values(&mut final_assets, &augmented_excluded);
        }

        println!("[sync] Writing augmented Luau module …");
        fs::write(&args.assets_output, render_luau_module(&final_assets))
            .context("Failed to write Luau file")?;

        println!("[sync] Writing TypeScript declaration …");
        fs::write(&args.dts_output, render_dts_module(&final_assets))
            .context("Failed to write TypeScript file")?;

        println!("[sync] Done");
        return Ok(());
    }

    if args.dry_run {
        println!("[sync] Dry-run: skipping backend sync …");

        println!("[sync] Augmenting with image dimensions …");
        let assets = load_assets(&args.assets_input)
            .map_err(|e| anyhow::anyhow!("Failed to load assets: {}", e))?;
        let augmented_assets = augment_assets(&assets, &args.images_folder, &FsImageMetadata);

        println!("[sync] Writing augmented Luau module …");
        fs::write(&args.assets_output, render_luau_module(&augmented_assets))
            .context("Failed to write Luau file")?;

        println!("[sync] Writing TypeScript declaration …");
        fs::write(&args.dts_output, render_dts_module(&augmented_assets))
            .context("Failed to write TypeScript file")?;

        println!("[sync] Done");
        return Ok(());
    }

    // Run Asphalt sync
    // Resolve API key (TRUFFLE_API_KEY instead of ASPHALT_API_KEY)
    let api_key = resolve_api_key(args.api_key)?;
    println!("[sync] Running backend sync …");
    let multi_progress = MultiProgress::new();
    let sync_args = AsphaltSyncArgs {
        api_key: Some(api_key),
        target: Some(SyncTarget::Cloud { dry_run: false }),
        expected_price: None,
        project: PathBuf::from("."),
    };
    sync(sync_args, multi_progress)
        .await
        .context("Failed to sync assets with Asphalt")?;

    // Augment with image dimensions
    println!("[sync] Augmenting with image dimensions …");
    let assets = load_assets(&args.assets_input)
        .map_err(|e| anyhow::anyhow!("Failed to load assets: {}", e))?;

    let augmented_assets = augment_assets(&assets, &args.images_folder, &FsImageMetadata);

    println!("[sync] Writing augmented Luau module …");
    fs::write(&args.assets_output, render_luau_module(&augmented_assets))
        .context("Failed to write Luau file")?;

    println!("[sync] Writing TypeScript declaration …");
    fs::write(&args.dts_output, render_dts_module(&augmented_assets))
        .context("Failed to write TypeScript file")?;

    println!("[sync] Done");
    Ok(())
}

fn atlas_file_ids_from_assets(
    assets: &std::collections::BTreeMap<String, crate::assets::model::AssetValue>,
) -> HashMap<String, String> {
    fn walk(out: &mut HashMap<String, String>, node: &crate::assets::model::AssetValue) {
        let crate::assets::model::AssetValue::Table(map) = node else {
            return;
        };

        for (k, v) in map {
            match v {
                crate::assets::model::AssetValue::String(s) => {
                    if k.ends_with(".png") {
                        out.insert(k.clone(), s.clone());
                    }
                }
                crate::assets::model::AssetValue::Object(meta) => {
                    if k.ends_with(".png") {
                        out.insert(k.clone(), meta.id.clone());
                    }
                }
                crate::assets::model::AssetValue::Table(_) => walk(out, v),
                _ => {}
            }
        }
    }

    let mut out = HashMap::new();
    for (k, v) in assets {
        match v {
            crate::assets::model::AssetValue::String(s) => {
                if k.ends_with(".png") {
                    out.insert(k.clone(), s.clone());
                }
            }
            crate::assets::model::AssetValue::Object(meta) => {
                if k.ends_with(".png") {
                    out.insert(k.clone(), meta.id.clone());
                }
            }
            crate::assets::model::AssetValue::Table(_) => walk(&mut out, v),
            _ => {}
        }
    }
    out
}

fn resolve_api_key(provided: Option<String>) -> anyhow::Result<String> {
    if let Some(key) = provided {
        return Ok(key);
    }

    if let Ok(key) = std::env::var("TRUFFLE_API_KEY") {
        return Ok(key);
    }

    if let Ok(env_content) = fs::read_to_string(".env") {
        for line in env_content.lines() {
            if let Some(key) = line.strip_prefix("TRUFFLE_API_KEY=") {
                return Ok(key.trim().to_string());
            }
        }
    }

    anyhow::bail!("TRUFFLE_API_KEY environment variable is not set. Not syncing assets.")
}

fn resolve_atlas_exclude(
    cli: &[String],
    config: &[String],
    images_folder: &PathBuf,
) -> Vec<String> {
    let raw = if !cli.is_empty() { cli } else { config };
    let mut out: Vec<String> = raw
        .iter()
        .map(|item| normalize_atlas_key(item, images_folder))
        .collect();
    out.retain(|item| !item.is_empty());
    out.sort();
    out.dedup();
    out
}

fn normalize_atlas_key(value: &str, images_folder: &PathBuf) -> String {
    let mut key = value.replace('\\', "/");
    while let Some(stripped) = key.strip_prefix("./") {
        key = stripped.to_string();
    }
    while let Some(stripped) = key.strip_prefix('/') {
        key = stripped.to_string();
    }

    let images_folder = images_folder.to_string_lossy().replace('\\', "/");
    let images_folder = images_folder.trim_end_matches('/').to_string();
    if !images_folder.is_empty() {
        let with_sep = format!("{}/", images_folder);
        if key.starts_with(&with_sep) {
            key = key[with_sep.len()..].to_string();
        } else if key == images_folder {
            key.clear();
        }
    }

    key
}

fn build_exclude_glob(images_folder: &PathBuf, keys: &[String]) -> Option<String> {
    let mut patterns = Vec::new();
    for key in keys {
        let pattern = normalize_exclude_pattern(key).pattern;
        let pattern = images_folder
            .join(pattern)
            .to_string_lossy()
            .replace('\\', "/");
        patterns.push(pattern);
    }

    if patterns.is_empty() {
        return None;
    }

    patterns.sort();
    patterns.dedup();

    if patterns.len() == 1 {
        return Some(patterns[0].clone());
    }

    Some(format!("{{{}}}", patterns.join(",")))
}

fn build_atlas_exclude(keys: &[String]) -> anyhow::Result<AtlasExclude> {
    let mut exact = HashSet::new();
    let mut globs = Vec::new();

    for raw in keys {
        let normalized = raw.trim().to_string();
        if normalized.is_empty() {
            continue;
        }

        let pattern = normalize_exclude_pattern(&normalized);
        if pattern.is_glob {
            globs.push(
                Glob::new(pattern.pattern.as_str())
                    .with_context(|| format!("Invalid atlas exclude glob: {}", pattern.pattern))?,
            );
        } else {
            exact.insert(pattern.pattern);
        }
    }

    Ok(AtlasExclude { exact, globs })
}

fn normalize_exclude_pattern(value: &str) -> ExcludePattern {
    let trimmed = value.trim_matches('/');
    let mut pattern = trimmed.to_string();

    if pattern.ends_with('/') {
        pattern = format!("{}**/*.png", pattern);
    } else if !pattern.contains('.') && !pattern.contains('/') {
        pattern = format!("{}/**/*.png", pattern);
    } else if !pattern.contains('.') && pattern.contains('/') {
        pattern = format!("{}/**/*.png", pattern);
    }

    let is_glob = pattern.chars().any(|c| matches!(c, '*' | '?' | '{' | '}' | '[' | ']'));

    ExcludePattern { pattern, is_glob }
}

struct ExcludePattern {
    pattern: String,
    is_glob: bool,
}

fn merge_asset_values(
    dest: &mut BTreeMap<String, crate::assets::model::AssetValue>,
    src: &BTreeMap<String, crate::assets::model::AssetValue>,
) {
    use crate::assets::model::AssetValue;

    for (key, value) in src {
        match (dest.get_mut(key), value) {
            (Some(AssetValue::Table(dest_table)), AssetValue::Table(src_table)) => {
                merge_asset_values(dest_table, src_table);
            }
            _ => {
                dest.insert(key.clone(), value.clone());
            }
        }
    }
}

fn filter_assets_by_exclude(
    assets: &BTreeMap<String, crate::assets::model::AssetValue>,
    exclude: &AtlasExclude,
) -> BTreeMap<String, crate::assets::model::AssetValue> {
    let mut out = BTreeMap::new();
    let mut path = Vec::new();
    walk_asset_values(assets, exclude, &mut path, &mut out);
    out
}

fn walk_asset_values(
    assets: &BTreeMap<String, crate::assets::model::AssetValue>,
    exclude: &AtlasExclude,
    path: &mut Vec<String>,
    out: &mut BTreeMap<String, crate::assets::model::AssetValue>,
) {
    use crate::assets::model::AssetValue;

    for (key, value) in assets {
        path.push(key.clone());
        match value {
            AssetValue::Table(map) => {
                walk_asset_values(map, exclude, path, out);
            }
            _ => {
                if key.ends_with(".png") {
                    let joined = path.join("/");
                    if exclude.is_match(&joined) {
                        insert_asset_value(out, path, value.clone());
                    }
                }
            }
        }
        path.pop();
    }
}

fn insert_asset_value(
    root: &mut BTreeMap<String, crate::assets::model::AssetValue>,
    path: &[String],
    value: crate::assets::model::AssetValue,
) {
    use crate::assets::model::AssetValue;

    if path.is_empty() {
        return;
    }

    if path.len() == 1 {
        root.insert(path[0].clone(), value);
        return;
    }

    let head = path[0].clone();
    let entry = root
        .entry(head)
        .or_insert_with(|| AssetValue::Table(BTreeMap::new()));

    if !matches!(entry, AssetValue::Table(_)) {
        *entry = AssetValue::Table(BTreeMap::new());
    }

    let AssetValue::Table(map) = entry else {
        return;
    };

    insert_asset_value(map, &path[1..], value);
}
