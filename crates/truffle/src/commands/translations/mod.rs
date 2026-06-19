mod api;
mod export;
mod lexicon;
mod push;
mod ui;

use clap::Subcommand;

pub use export::ExportArgs;
pub use push::PushArgs;

#[derive(Subcommand)]
pub enum TranslationsCommands {
    /// Push a Lexi lexicon to Roblox cloud localization
    Push(PushArgs),
    /// Export a Lexi lexicon to a Roblox localization CSV for manual upload
    Export(ExportArgs),
}

pub fn run(command: TranslationsCommands) -> bool {
    match command {
        TranslationsCommands::Push(args) => push::run(args),
        TranslationsCommands::Export(args) => export::run(args),
    }
}
