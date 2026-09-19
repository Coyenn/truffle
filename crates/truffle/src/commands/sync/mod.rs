mod atlas;
mod catalog;
mod direct;
mod paths;
mod preview;
mod transaction;

use crate::commands::image::HighlightArgs;
use anyhow::Context;
use clap::Parser;
use paths::prepare_scratch_dir;
use std::path::PathBuf;
use tokio::runtime::Runtime;
use truffle_config::TruffleConfig;

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

    /// Preview synchronization without modifying project files or uploading
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

    /// Select source images. With atlases, validates the selection and synchronizes
    /// the complete shared layout; only changed atlas pages are uploaded.
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

    if args.dry_run {
        return preview::run(&args, &config);
    }

    if let Some(pattern) = &args.sync_only {
        paths::validate_selection(pattern, &args.images_folder)?;
    }
    let transaction = transaction::SyncTransaction::begin(&transaction_paths(&args, &config))?;
    match run_async_inner(&args, &config).await {
        Ok(()) => {
            transaction.commit();
            Ok(())
        }
        Err(error) => {
            transaction
                .rollback()
                .with_context(|| format!("Sync failed: {error:#}; failed to restore outputs"))?;
            Err(error)
        }
    }
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

async fn run_async_inner(args: &EffectiveSync, config: &TruffleConfig) -> anyhow::Result<()> {
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
        anyhow::ensure!(
            crate::commands::image::run(crate::commands::image::ImageCommands::Highlight(
                highlight_args
            )),
            "Failed to generate configured highlight variants"
        );
    }

    if !args.skip_atlas && (args.atlas || config.atlas) {
        if args.sync_only.is_some() {
            println!("[sync] Selected images share atlas pages: synchronizing the complete layout; only changed pages upload.");
        }
        atlas::run(args, config).await
    } else {
        direct::run(args, config).await
    }
}

fn transaction_paths(args: &EffectiveSync, config: &TruffleConfig) -> Vec<PathBuf> {
    let mut paths = vec![
        args.assets_output.clone(),
        args.dts_output.clone(),
        PathBuf::from(asphalt::lockfile::FILE_NAME),
        args.scratch_dir.join(paths::ATLAS_STATE_FILE),
    ];
    for (name, input) in &config.asphalt.inputs {
        // Backend outputs are also restored if final augmentation fails.
        paths.push(input.output_path.join(format!("{name}.luau")));
        paths.push(input.output_path.join(format!("{name}.d.ts")));
    }
    for directory in [
        paths::scratch_sync_dir(&args.scratch_dir),
        paths::scratch_unatlased_dir(&args.scratch_dir),
        paths::scratch_subset_dir(&args.scratch_dir),
        paths::scratch_sync_dir(&args.scratch_dir).join("direct"),
    ] {
        for name in config
            .asphalt
            .inputs
            .keys()
            .map(String::as_str)
            .chain(["assets", "atlases"])
        {
            paths.push(directory.join(format!("{name}.luau")));
            paths.push(directory.join(format!("{name}.d.ts")));
        }
    }
    paths
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
