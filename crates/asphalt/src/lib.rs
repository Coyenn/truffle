//! Minimal library surface for truffle integration.
//!
//! Upstream asphalt is primarily a binary crate, but we expose the pieces
//! needed by the truffle CLI and config crate.

pub mod asset;
pub mod cli;
pub mod config;
pub mod glob;
pub mod hash;
pub mod lockfile;
pub mod sync;
pub mod util;
pub mod web_api;

pub use cli::{SyncArgs, SyncTarget};
pub use config::Config;
pub use sync::sync;
