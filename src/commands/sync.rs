use crate::assets::{
    augment_assets, load_assets, render_dts_module, render_luau_module, FsImageMetadata,
};
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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

    /// ASPHALT_API_KEY environment variable (or read from .env file)
    #[arg(long)]
    pub asphalt_api_key: Option<String>,
}

pub fn run(args: SyncArgs) -> bool {
    let backend = AsphaltBackend;

    if let Err(e) = backend.ensure_available() {
        eprintln!("[sync] ERROR: {}", e);
        return false;
    }

    let api_key = match resolve_api_key(args.asphalt_api_key) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("[sync] WARN: {}", e);
            return false;
        }
    };

    std::env::set_var("ASPHALT_API_KEY", &api_key);

    println!("[sync] Running backend sync …");
    if let Err(e) = backend.sync(&api_key) {
        eprintln!("[sync] ERROR: {}", e);
        return false;
    }

    println!("[sync] Augmenting with image dimensions …");
    let assets = match load_assets(&args.assets_input) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[sync] ERROR: Failed to load assets: {}", e);
            return false;
        }
    };

    let augmented_assets = augment_assets(&assets, &args.images_folder, &FsImageMetadata);

    println!("[sync] Writing augmented Luau module …");
    if let Err(e) = fs::write(&args.assets_output, render_luau_module(&augmented_assets)) {
        eprintln!("[sync] ERROR: Failed to write Luau file: {}", e);
        return false;
    }

    println!("[sync] Writing TypeScript declaration …");
    if let Err(e) = fs::write(&args.dts_output, render_dts_module(&augmented_assets)) {
        eprintln!("[sync] ERROR: Failed to write TypeScript file: {}", e);
        return false;
    }

    println!("[sync] Done ✅");
    true
}

fn resolve_api_key(provided: Option<String>) -> Result<String, String> {
    if let Some(key) = provided {
        return Ok(key);
    }

    if let Ok(key) = std::env::var("ASPHALT_API_KEY") {
        return Ok(key);
    }

    if let Ok(env_content) = fs::read_to_string(".env") {
        for line in env_content.lines() {
            if let Some(key) = line.strip_prefix("ASPHALT_API_KEY=") {
                return Ok(key.trim().to_string());
            }
        }
    }

    Err("ASPHALT_API_KEY environment variable is not set. Not syncing assets.".to_string())
}

trait SyncBackend {
    fn ensure_available(&self) -> Result<(), String>;
    fn sync(&self, api_key: &str) -> Result<(), String>;
}

struct AsphaltBackend;

impl SyncBackend for AsphaltBackend {
    fn ensure_available(&self) -> Result<(), String> {
        Command::new("asphalt")
            .arg("--version")
            .output()
            .map(|_| ())
            .map_err(|_| "asphalt command is not available. Please install asphalt.".to_string())
    }

    fn sync(&self, _api_key: &str) -> Result<(), String> {
        let output = Command::new("asphalt")
            .arg("sync")
            .output()
            .map_err(|e| format!("Failed to run asphalt sync: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("asphalt sync failed: {}", stderr))
        }
    }
}
