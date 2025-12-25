pub use crate::commands::highlight::{HighlightArgs, run as highlight_run};

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
