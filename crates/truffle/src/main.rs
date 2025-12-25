mod assets;
mod commands;
mod image;

use clap::{Parser, Subcommand, builder::styling};

#[derive(Parser)]
#[command(name = "truffle")]
#[command(about = "Truffle")]
#[command(version = env!("TRUFFLE_VERSION"))]
#[command(long_version = env!("TRUFFLE_VERSION"))]
#[command(
    styles = styling::Styles::styled()
        .header(styling::AnsiColor::Green.on_default() | styling::Effects::BOLD)
        .usage(styling::AnsiColor::Green.on_default() | styling::Effects::BOLD)
        .literal(styling::AnsiColor::Cyan.on_default() | styling::Effects::BOLD)
        .placeholder(styling::AnsiColor::Cyan.on_default())
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Sync assets and augment metadata with image dimensions
    Sync(commands::sync::SyncArgs),
    /// Generate a bitmap atlas from a .ttf font
    Font(commands::font::FontArgs),
    /// Image manipulation commands
    Image {
        #[command(subcommand)]
        command: commands::image::ImageCommands,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Sync(args) => commands::sync::run(args),
        Commands::Font(args) => commands::font::run(args),
        Commands::Image { command } => commands::image::run(command),
    };

    std::process::exit(if result { 0 } else { 1 });
}
