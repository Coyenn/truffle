pub use crate::commands::highlight::{run as highlight_run, HighlightArgs};

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ImageCommands {
    /// Generate highlight variants of PNG images with white outlines
    Highlight(HighlightArgs),
}

pub fn run(command: ImageCommands) -> bool {
    match command {
        ImageCommands::Highlight(args) => highlight_run(args),
    }
}
