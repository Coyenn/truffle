pub use crate::commands::highlight::{HighlightArgs, run as highlight_run};
pub use crate::commands::palette::{PaletteArgs, run as palette_run};

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ImageCommands {
    /// Generate highlight variants of PNG images with white outlines
    Highlight(HighlightArgs),
    /// Apply a color palette to PNG images
    Palette(PaletteArgs),
}

pub fn run(command: ImageCommands) -> bool {
    match command {
        ImageCommands::Highlight(args) => highlight_run(args),
        ImageCommands::Palette(args) => palette_run(args),
    }
}
