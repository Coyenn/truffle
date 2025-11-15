// Library interface for Asphalt
// This allows Asphalt to be used as a library dependency

pub mod asset;
pub mod auth;
pub mod cli;
pub mod config;
pub mod glob;
pub mod lockfile;
pub mod migrate_lockfile;
pub mod progress_bar;
pub mod sync;
pub mod upload;
pub mod util;
pub mod web_api;

// Re-export commonly used types
pub use cli::{SyncArgs, SyncTarget};
pub use config::Config;
pub use sync::sync;
