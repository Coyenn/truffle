use asphalt::{asset, cli, config, lockfile, sync, web_api};
use clap::Parser;
use cli::{Cli, Commands};
use dotenvy::dotenv;
use fs_err::tokio as fs;
use indicatif::MultiProgress;
use log::LevelFilter;
use schemars::schema_for;

use crate::config::Config;

mod migrate_lockfile;
mod upload;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenv();

    let args = Cli::parse();

    let mut binding = env_logger::Builder::new();
    let logger = binding
        .filter_level(LevelFilter::Info)
        .filter_module("asphalt", args.verbose.log_level_filter())
        .format_timestamp(None)
        .format_module_path(false)
        .build();

    let level = logger.filter();

    let multi_progress = MultiProgress::new();
    indicatif_log_bridge::LogWrapper::new(multi_progress.clone(), logger).try_init()?;

    log::set_max_level(level);

    match args.command {
        Commands::Sync(args) => sync::sync(args, multi_progress).await,
        Commands::Upload(args) => upload::upload(args).await,
        Commands::MigrateLockfile(args) => migrate_lockfile::migrate_lockfile(args).await,
        Commands::GenerateConfigSchema => generate_config_schema().await,
    }
}

async fn generate_config_schema() -> anyhow::Result<()> {
    let schema = schema_for!(Config);
    fs::write(
        "schema.json",
        serde_json::to_string_pretty(&schema).unwrap(),
    )
    .await?;

    Ok(())
}
