pub use crate::commands::highlight::{run as highlight_run, HighlightArgs};
pub use crate::commands::palette::{run as palette_run, PaletteArgs};
pub use crate::commands::terrain::{run as terrain_run, TerrainArgs};

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ImageCommands {
    /// Generate highlight variants of PNG images with white outlines
    Highlight(HighlightArgs),
    /// Apply a color palette to PNG images
    Palette(PaletteArgs),
    /// Generate grass integration PNG overlays
    Terrain(TerrainArgs),
}

pub fn run(command: ImageCommands) -> bool {
    match command {
        ImageCommands::Highlight(args) => highlight_run(args),
        ImageCommands::Palette(args) => palette_run(args),
        ImageCommands::Terrain(args) => terrain_run(args),
    }
}
