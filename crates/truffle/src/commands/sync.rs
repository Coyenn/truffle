use crate::assets::{
    augment_assets, load_assets, render_dts_module, render_luau_module, FsImageMetadata,
};
use crate::commands::image::HighlightArgs;
use anyhow::Context;
use asphalt::{
    cli::{SyncArgs as AsphaltSyncArgs, SyncTarget},
    sync,
};
use clap::Parser;
use indicatif::MultiProgress;
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

    // Resolve API key (TRUFFLE_API_KEY instead of ASPHALT_API_KEY)
    let api_key = resolve_api_key(args.api_key)?;

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

    // Run Asphalt sync
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

    println!("[sync] Done ✅");
    Ok(())
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
