pub use crate::commands::highlight::{run as highlight_run, HighlightArgs};
pub use crate::commands::palette::{run as palette_run, PaletteArgs};
pub use crate::commands::project::{run as project_run, ProjectArgs};
pub use crate::commands::terrain::{run as terrain_run, TerrainArgs};

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ImageCommands {
    /// Generate highlight variants of PNG images with white outlines
    Highlight(HighlightArgs),
    /// Apply a color palette to PNG images
    Palette(PaletteArgs),
    /// Project a PNG through a self-contained coordinate and shading map
    Project(ProjectArgs),
    /// Generate grass integration PNG overlays
    Terrain(TerrainArgs),
}

pub fn run(command: ImageCommands) -> bool {
    match command {
        ImageCommands::Highlight(args) => highlight_run(args),
        ImageCommands::Palette(args) => palette_run(args),
        ImageCommands::Project(args) => project_run(args),
        ImageCommands::Terrain(args) => terrain_run(args),
    }
}
