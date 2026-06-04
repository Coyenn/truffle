mod api;
mod lexicon;
mod push;
mod ui;

use clap::Subcommand;

pub use push::PushArgs;

#[derive(Subcommand)]
pub enum TranslationsCommands {
    /// Push a Lexi lexicon to Roblox cloud localization
    Push(PushArgs),
}

pub fn run(command: TranslationsCommands) -> bool {
    match command {
        TranslationsCommands::Push(args) => push::run(args),
    }
}
