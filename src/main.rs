mod assets;
mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "truffle")]
#[command(about = "Truffle CLI tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Sync assets and augment metadata with image dimensions
    Sync(commands::sync::SyncArgs),
    /// Generate highlight variants of PNG images with white outlines
    Highlights(commands::highlights::HighlightsArgs),
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Sync(args) => commands::sync::run(args),
        Commands::Highlights(args) => commands::highlights::run(args),
    };

    std::process::exit(if result { 0 } else { 1 });
}
