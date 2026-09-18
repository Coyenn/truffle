use crate::assets::{
    augment_assets, build_atlased_assets, build_atlases, load_assets, render_dts_module,
    render_luau_module, AtlasExclude, AtlasOptions, FsImageMetadata,
};
use crate::commands::image::HighlightArgs;
use anyhow::Context;
use asphalt::{
    cli::{SyncArgs as AsphaltSyncArgs, SyncTarget},
    config::Input as AsphaltInput,
    glob::Glob,
    lockfile::RawLockfile,
    sync_with_config,
};
use clap::Parser;
use indicatif::MultiProgress;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::runtime::Runtime;
use truffle_config::TruffleConfig;

const SCRATCH_ATLAS_PNG_DIR: &str = "atlases";
const SCRATCH_SYNC_DIR: &str = "sync";
const SCRATCH_UNATLASED_DIR: &str = "unatlased";
const SCRATCH_SUBSET_DIR: &str = "subset";
/// Packing state (tracked via gitignore exception) so atlases stay stable
/// across machines and syncs.
const ATLAS_STATE_FILE: &str = "truffle-atlases.toml";

fn scratch_atlas_png_dir(scratch_dir: &Path) -> PathBuf {
    scratch_dir.join(SCRATCH_ATLAS_PNG_DIR)
}

fn scratch_sync_dir(scratch_dir: &Path) -> PathBuf {
    scratch_dir.join(SCRATCH_SYNC_DIR)
}

fn scratch_unatlased_dir(scratch_dir: &Path) -> PathBuf {
    scratch_sync_dir(scratch_dir).join(SCRATCH_UNATLASED_DIR)
}

fn scratch_subset_dir(scratch_dir: &Path) -> PathBuf {
    scratch_sync_dir(scratch_dir).join(SCRATCH_SUBSET_DIR)
}

fn prepare_scratch_dir(scratch_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(scratch_dir)
        .with_context(|| format!("Failed to create scratch dir {}", scratch_dir.display()))?;

    for legacy in ["asphalt", "subset-sync"] {
        let legacy_path = scratch_dir.join(legacy);
        if legacy_path.is_dir() {
            fs::remove_dir_all(&legacy_path).with_context(|| {
                format!("Failed to remove legacy dir {}", legacy_path.display())
            })?;
        }
    }
    Ok(())
}

fn remove_scratch_subset(scratch_dir: &Path) -> anyhow::Result<()> {
    let subset_dir = scratch_subset_dir(scratch_dir);
    if subset_dir.is_dir() {
        fs::remove_dir_all(&subset_dir)
            .with_context(|| format!("Failed to remove {}", subset_dir.display()))?;
    }
    Ok(())
}

#[derive(Parser)]
#[command(about = "Sync assets and augment metadata with image dimensions")]
pub struct SyncArgs {
    /// Path to the Luau assets module file (defaults to truffle.toml `assets_input`)
    #[arg(long)]
    pub assets_input: Option<PathBuf>,

    /// Path to write the augmented Luau assets module (defaults to truffle.toml `assets_output`)
    #[arg(long)]
    pub assets_output: Option<PathBuf>,

    /// Path to write the TypeScript declaration file (defaults to truffle.toml `dts_output`)
    #[arg(long)]
    pub dts_output: Option<PathBuf>,

    /// Path to the raw assets images folder (defaults to truffle.toml `images_folder`)
    #[arg(long)]
    pub images_folder: Option<PathBuf>,

    /// Pack images into atlas textures before syncing (overrides truffle.toml `atlas`)
    #[arg(long)]
    pub atlas: bool,

    /// Atlas texture size, power-of-two square (overrides truffle.toml `atlas_size`)
    #[arg(long)]
    pub atlas_size: Option<u32>,

    /// Padding in pixels around each sprite in the atlas (overrides truffle.toml `atlas_padding`)
    #[arg(long)]
    pub atlas_padding: Option<u32>,

    /// Image keys to exclude from atlas packing, repeatable (overrides truffle.toml `atlas_exclude`)
    #[arg(long)]
    pub atlas_exclude: Vec<String>,

    /// Write outputs without syncing to Roblox
    #[arg(long)]
    pub dry_run: bool,

    /// Scratch directory for intermediate/generated files (overrides truffle.toml `scratch_dir`)
    #[arg(long)]
    pub scratch_dir: Option<PathBuf>,

    /// Roblox Open Cloud API key (overrides `TRUFFLE_API_KEY` env and truffle.toml `api_key`)
    #[arg(long, env = "TRUFFLE_API_KEY")]
    pub api_key: Option<String>,

    /// Skip atlas packing and sync source images directly
    #[arg(long)]
    pub skip_atlas: bool,

    /// Only sync files matching this glob (e.g. assets/images/interface/fonts/**/*.png)
    #[arg(long)]
    pub sync_only: Option<String>,
}

pub fn run(args: SyncArgs) -> bool {
    let rt = Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
        match run_async(args).await {
            Ok(()) => true,
            Err(e) => {
                eprintln!("[sync] ERROR: {e:#}");
                false
            }
        }
    })
}

async fn run_async(args: SyncArgs) -> anyhow::Result<()> {
    // Single config parse: CLI flags override truffle.toml, which already
    // carries built-in defaults for everything it does not set.
    let config = TruffleConfig::read()
        .await
        .context("Failed to read truffle.toml. Make sure it exists in the current directory.")?;
    let args = EffectiveSync::resolve(args, &config);

    let backup = backup_asset_modules(&args.assets_output, &args.dts_output)?;

    let result = run_async_inner(args, config).await;

    if result.is_err() {
        if let Some(backup) = backup {
            restore_asset_modules(&backup)?;
            eprintln!("[sync] Restored previous assets module after failed sync");
        }
    }

    result
}

/// `SyncArgs` with every `truffle.toml`-backed option resolved.
///
/// Resolution order per option: CLI flag, then `truffle.toml`, then the
/// built-in default (which `truffle.toml` itself already applies, so this
/// mostly fills in CLI `None`s from config).
struct EffectiveSync {
    assets_input: PathBuf,
    assets_output: PathBuf,
    dts_output: PathBuf,
    images_folder: PathBuf,
    scratch_dir: PathBuf,
    api_key: Option<String>,
    atlas: bool,
    atlas_size: u32,
    atlas_padding: u32,
    atlas_exclude: Vec<String>,
    dry_run: bool,
    skip_atlas: bool,
    sync_only: Option<String>,
}

impl EffectiveSync {
    fn resolve(args: SyncArgs, config: &TruffleConfig) -> Self {
        Self {
            assets_input: args
                .assets_input
                .unwrap_or_else(|| config.assets_input.clone()),
            assets_output: args
                .assets_output
                .unwrap_or_else(|| config.assets_output.clone()),
            dts_output: args.dts_output.unwrap_or_else(|| config.dts_output.clone()),
            images_folder: args
                .images_folder
                .unwrap_or_else(|| config.images_folder.clone()),
            scratch_dir: args
                .scratch_dir
                .unwrap_or_else(|| config.scratch_dir.clone()),
            api_key: args.api_key,
            atlas: args.atlas,
            atlas_size: args.atlas_size.unwrap_or(config.atlas_size),
            atlas_padding: args.atlas_padding.unwrap_or(config.atlas_padding),
            atlas_exclude: args.atlas_exclude,
            dry_run: args.dry_run,
            skip_atlas: args.skip_atlas,
            sync_only: args.sync_only,
        }
    }
}

async fn run_async_inner(args: EffectiveSync, config: TruffleConfig) -> anyhow::Result<()> {
    let scratch_dir: PathBuf = args.scratch_dir.clone();
    prepare_scratch_dir(&scratch_dir)?;

    // Auto-generate highlights if configured (before sync so they get synced too)
    if config.auto_highlight {
        println!("[sync] Generating highlight variants …");
        let highlight_args = HighlightArgs {
            input_path: args.images_folder.clone(),
            dry_run: false,
            force: config.highlight_force,
            thickness: config.highlight_thickness,
            recursive: true,
        };
        crate::commands::image::run(crate::commands::image::ImageCommands::Highlight(
            highlight_args,
        ));
    }

    let atlas_enabled = !args.skip_atlas && (args.atlas || config.atlas);
    if atlas_enabled {
        println!("[sync] Building image atlases …");
        let atlas_dir = scratch_atlas_png_dir(&scratch_dir);
        let sync_codegen_dir = scratch_sync_dir(&scratch_dir);
        // Asphalt codegen writes `{input_name}.luau`. Our atlas input is named `atlases`.
        let atlas_assets_output = sync_codegen_dir.join("atlases.luau");
        let atlas_padding = args.atlas_padding;
        let atlas_size = args.atlas_size;
        let atlas_exclude = resolve_atlas_exclude(
            &args.atlas_exclude,
            &config.atlas_exclude,
            &args.images_folder,
        );
        let atlas_exclude_matcher = build_atlas_exclude(&atlas_exclude)?;
        let atlas_state_path = scratch_dir.join(ATLAS_STATE_FILE);

        let placements = build_atlases(
            &args.images_folder,
            &atlas_dir,
            &atlas_state_path,
            AtlasOptions {
                padding: atlas_padding,
                size: atlas_size,
                exclude: atlas_exclude_matcher.clone(),
            },
        )
        .context("Failed to build atlases")?;

        std::fs::create_dir_all(&sync_codegen_dir).with_context(|| {
            format!(
                "Failed to create codegen dir {}",
                sync_codegen_dir.display()
            )
        })?;
        let unatlased_codegen_dir = scratch_unatlased_dir(&scratch_dir);

        if !args.dry_run {
            // Resolve API key: --api-key flag, TRUFFLE_API_KEY env (.env included), truffle.toml.
            let api_key = resolve_api_key(args.api_key.clone(), config.api_key.clone())?;

            // Reuse the already-parsed config instead of re-reading truffle.toml.
            let mut asphalt_config = config.asphalt.clone();
            asphalt_config.inputs = {
                let mut inputs = HashMap::new();

                let atlas_glob = format!("{}/**/*.png", atlas_dir.display());
                inputs.insert(
                    "atlases".to_string(),
                    AsphaltInput {
                        include: Glob::new(atlas_glob.as_str())
                            .context("Invalid atlas include glob")?,
                        output_path: sync_codegen_dir.clone(),
                        bleed: false,
                        web: HashMap::new(),
                    },
                );

                let exclude_glob = if atlas_exclude.is_empty() {
                    None
                } else {
                    Some(
                        build_exclude_glob(&args.images_folder, &atlas_exclude)
                            .context("Atlas exclude list was empty after normalization")?,
                    )
                };

                let mut found_images_input = false;
                for (name, input) in asphalt_config.inputs.iter() {
                    if is_images_input(&args.images_folder, &input.include.get_prefix()) {
                        found_images_input = true;
                        if let Some(exclude_glob) = &exclude_glob {
                            let mut updated = input.clone();
                            updated.include = Glob::new(exclude_glob.as_str())
                                .context("Invalid atlas exclude glob")?;
                            updated.output_path = unatlased_codegen_dir.clone();
                            inputs.insert(name.clone(), updated);
                        }
                        continue;
                    }

                    inputs.insert(name.clone(), input.clone());
                }

                if !atlas_exclude.is_empty() && !found_images_input {
                    anyhow::bail!("Failed to find images input matching images_folder");
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
            let unatlased_assets_path = unatlased_codegen_dir.join("assets.luau");
            let excluded_assets = if unatlased_assets_path.exists() {
                load_assets(&unatlased_assets_path)
                    .map_err(|e| anyhow::anyhow!("Failed to load unatlased assets: {}", e))?
            } else {
                load_assets(&args.assets_input)
                    .map_err(|e| anyhow::anyhow!("Failed to load assets: {}", e))?
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
    // Resolve API key: --api-key flag, TRUFFLE_API_KEY env (.env included), truffle.toml.
    let api_key = resolve_api_key(args.api_key.clone(), config.api_key.clone())?;
    println!("[sync] Running backend sync …");
    let multi_progress = MultiProgress::new();
    let sync_args = AsphaltSyncArgs {
        api_key: Some(api_key),
        target: Some(SyncTarget::Cloud { dry_run: false }),
        expected_price: None,
        project: PathBuf::from("."),
    };

    if let Some(sync_only) = &args.sync_only {
        // Reuse the already-parsed config instead of re-reading truffle.toml.
        let mut asphalt_config = config.asphalt.clone();
        remove_scratch_subset(&scratch_dir)?;
        let subset_output = scratch_subset_dir(&scratch_dir);
        asphalt_config.inputs = HashMap::from([(
            "assets".to_string(),
            AsphaltInput {
                include: Glob::new(sync_only.as_str()).context("Invalid --sync-only glob")?,
                output_path: subset_output.clone(),
                bleed: asphalt_config
                    .inputs
                    .get("assets")
                    .map(|input| input.bleed)
                    .unwrap_or(true),
                web: HashMap::new(),
            },
        )]);
        let project_dir = PathBuf::from(".");
        let full_lockfile = RawLockfile::read_from(&project_dir)
            .await
            .context("Failed to read lockfile before subset sync")?
            .into_lockfile()?;

        sync_with_config(asphalt_config, sync_args, multi_progress)
            .await
            .context("Failed to sync subset with Asphalt")?;

        println!("[sync] Merging synced subset into existing lockfile …");
        let subset_lockfile = RawLockfile::read_from(&project_dir)
            .await
            .context("Failed to read lockfile after subset sync")?
            .into_lockfile()?;
        let mut merged_lockfile = full_lockfile;
        merged_lockfile.absorb(subset_lockfile);
        merged_lockfile
            .write_to(&project_dir)
            .await
            .context("Failed to write merged lockfile")?;

        println!("[sync] Merging synced subset into existing assets module …");
        let synced_subset = load_assets(&subset_output.join("assets.luau"))
            .map_err(|e| anyhow::anyhow!("Failed to load synced subset assets: {}", e))?;
        let synced_subset =
            if let Some(prefix) = sync_subset_nested_prefix(sync_only, &args.images_folder) {
                nest_assets_under_path(synced_subset, &prefix)
            } else {
                synced_subset
            };
        let mut assets = load_assets(&args.assets_input)
            .map_err(|e| anyhow::anyhow!("Failed to load assets: {}", e))?;
        merge_asset_values(&mut assets, &synced_subset);
        let augmented_assets = augment_assets(&assets, &args.images_folder, &FsImageMetadata);

        println!("[sync] Writing augmented Luau module …");
        fs::write(&args.assets_output, render_luau_module(&augmented_assets))
            .context("Failed to write Luau file")?;

        println!("[sync] Writing TypeScript declaration …");
        fs::write(&args.dts_output, render_dts_module(&augmented_assets))
            .context("Failed to write TypeScript file")?;

        remove_scratch_subset(&scratch_dir)?;
        println!("[sync] Done");
        return Ok(());
    }

    sync_with_config(config.asphalt.clone(), sync_args, multi_progress)
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

struct AssetModuleBackup {
    luau: PathBuf,
    dts: PathBuf,
    luau_bytes: Vec<u8>,
    dts_bytes: Vec<u8>,
}

fn backup_asset_modules(
    assets_output: &Path,
    dts_output: &Path,
) -> anyhow::Result<Option<AssetModuleBackup>> {
    if !assets_output.exists() && !dts_output.exists() {
        return Ok(None);
    }

    let luau_bytes = if assets_output.exists() {
        fs::read(assets_output)
            .with_context(|| format!("Failed to read backup source {}", assets_output.display()))?
    } else {
        Vec::new()
    };

    let dts_bytes = if dts_output.exists() {
        fs::read(dts_output)
            .with_context(|| format!("Failed to read backup source {}", dts_output.display()))?
    } else {
        Vec::new()
    };

    Ok(Some(AssetModuleBackup {
        luau: assets_output.to_path_buf(),
        dts: dts_output.to_path_buf(),
        luau_bytes,
        dts_bytes,
    }))
}

fn restore_asset_modules(backup: &AssetModuleBackup) -> anyhow::Result<()> {
    if !backup.luau_bytes.is_empty() {
        fs::write(&backup.luau, &backup.luau_bytes).with_context(|| {
            format!("Failed to restore assets module {}", backup.luau.display())
        })?;
    }
    if !backup.dts_bytes.is_empty() {
        fs::write(&backup.dts, &backup.dts_bytes).with_context(|| {
            format!(
                "Failed to restore assets declarations {}",
                backup.dts.display()
            )
        })?;
    }
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

/// Resolve the API key: `--api-key` flag (which also reads `TRUFFLE_API_KEY`
/// from the environment, with `.env` loaded at startup), then truffle.toml.
fn resolve_api_key(flag: Option<String>, config_key: Option<String>) -> anyhow::Result<String> {
    if let Some(key) = flag {
        return Ok(key);
    }

    if let Some(key) = config_key {
        return Ok(key);
    }

    anyhow::bail!(
        "No API key found. Pass --api-key, set TRUFFLE_API_KEY (e.g. via .env), or set `api_key` in truffle.toml."
    )
}

fn resolve_atlas_exclude(cli: &[String], config: &[String], images_folder: &Path) -> Vec<String> {
    let raw = if !cli.is_empty() { cli } else { config };
    let mut out: Vec<String> = raw
        .iter()
        .filter_map(|item| normalize_atlas_key(item, images_folder))
        .collect();
    out.retain(|item| !item.is_empty());
    out.sort();
    out.dedup();
    out
}

fn normalize_atlas_key(value: &str, images_folder: &Path) -> Option<String> {
    let mut key = value.replace('\\', "/");
    while let Some(stripped) = key.strip_prefix("./") {
        key = stripped.to_string();
    }
    while let Some(stripped) = key.strip_prefix('/') {
        key = stripped.to_string();
    }

    let images_folder = normalize_path_for_compare(images_folder);
    if !images_folder.is_empty() {
        let with_sep = format!("{}/", images_folder);
        if key.starts_with(&with_sep) {
            key = key[with_sep.len()..].to_string();
        } else if key == images_folder {
            return None;
        } else if let Some(images_root) = images_folder.split('/').next() {
            let root_prefix = format!("{}/", images_root);
            if key.starts_with(&root_prefix) {
                return None;
            }
        }
    }

    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

fn build_exclude_glob(images_folder: &Path, keys: &[String]) -> Option<String> {
    let mut patterns = Vec::new();
    for key in keys {
        patterns.extend(build_exclude_patterns(key));
    }

    if patterns.is_empty() {
        return None;
    }

    patterns.sort();
    patterns.dedup();

    let images_folder = normalize_path_for_compare(images_folder);
    if images_folder.is_empty() {
        if patterns.len() == 1 {
            return Some(patterns[0].clone());
        }
        return Some(format!("{{{}}}", patterns.join(",")));
    }

    if patterns.len() == 1 {
        return Some(format!("{images_folder}/{}", patterns[0]));
    }

    Some(format!("{images_folder}/{{{}}}", patterns.join(",")))
}

fn build_exclude_patterns(value: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    let raw = value.trim().trim_matches('/').to_string();
    if raw.is_empty() {
        return patterns;
    }

    let has_glob = raw
        .chars()
        .any(|c| matches!(c, '*' | '?' | '{' | '}' | '[' | ']'));
    let is_file = raw.to_ascii_lowercase().contains(".png");

    let file_pattern = if !has_glob && !is_file {
        format!("{}/**", raw)
    } else {
        raw.clone()
    };
    patterns.push(file_pattern);

    let prefix = glob_prefix(&raw);
    let prefix = prefix.trim_end_matches('/');
    let dir = if is_file || prefix.to_ascii_lowercase().ends_with(".png") {
        prefix
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_string())
    } else if prefix.is_empty() {
        None
    } else {
        Some(prefix.to_string())
    };

    if let Some(dir) = dir {
        patterns.extend(path_ancestors(&dir));
    }

    patterns
}

fn glob_prefix(value: &str) -> &str {
    match value.find(|c| ['*', '?', '{', '}', '[', ']'].contains(&c)) {
        Some(index) => &value[..index],
        None => value,
    }
}

fn path_ancestors(path: &str) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut current = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if current.is_empty() {
            current = segment.to_string();
        } else {
            current.push('/');
            current.push_str(segment);
        }
        ancestors.push(current.clone());
    }
    ancestors
}

fn is_images_input(images_folder: &Path, input_prefix: &Path) -> bool {
    normalize_path_for_compare(images_folder) == normalize_path_for_compare(input_prefix)
}

fn normalize_path_for_compare(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('\\', "/");
    while let Some(stripped) = value.strip_prefix("./") {
        value = stripped.to_string();
    }
    while let Some(stripped) = value.strip_prefix('/') {
        value = stripped.to_string();
    }
    while let Some(stripped) = value.strip_suffix('/') {
        value = stripped.to_string();
    }
    value
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
    let has_glob = pattern
        .chars()
        .any(|c| ['*', '?', '{', '}', '[', ']'].contains(&c));

    if !has_glob {
        if pattern.ends_with('/') {
            pattern = format!("{}**/*.png", pattern);
        } else if !pattern.contains('.') {
            pattern = format!("{}/**/*.png", pattern);
        }
    }

    let is_glob = pattern
        .chars()
        .any(|c| ['*', '?', '{', '}', '[', ']'].contains(&c));

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

fn sync_subset_nested_prefix(sync_only: &str, images_folder: &Path) -> Option<Vec<String>> {
    let images = normalize_path_for_compare(images_folder);
    let mut pattern = sync_only.replace('\\', "/");
    let rest = pattern.strip_prefix(&format!("{images}/"))?;
    pattern = rest.to_string();

    let dir = pattern.split("/**").next()?.trim_end_matches('/');
    if dir.is_empty() {
        return None;
    }

    Some(
        dir.split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn nest_assets_under_path(
    assets: BTreeMap<String, crate::assets::model::AssetValue>,
    prefix: &[String],
) -> BTreeMap<String, crate::assets::model::AssetValue> {
    use crate::assets::model::AssetValue;

    let mut nested = assets;
    for segment in prefix.iter().rev() {
        let mut wrapped = BTreeMap::new();
        wrapped.insert(segment.clone(), AssetValue::Table(nested));
        nested = wrapped;
    }
    nested
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
