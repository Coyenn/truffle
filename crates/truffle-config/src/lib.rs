use anyhow::{Context, Result};
use asphalt::config::Config as AsphaltConfig;
use fs_err::tokio as fs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const FILE_NAME: &str = "truffle.toml";

/// Truffle configuration that extends Asphalt's configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TruffleConfig {
    /// Asphalt configuration (all fields from truffle.toml, flattened)
    #[serde(flatten)]
    pub asphalt: AsphaltConfig,

    /// Truffle-specific configuration
    #[serde(default)]
    pub truffle: TruffleOptions,
}

/// Truffle-specific options
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct TruffleOptions {
    /// Enable automatic highlight generation after sync
    #[serde(default)]
    pub auto_highlight: bool,

    /// Default highlight thickness (used when auto_highlight is true)
    #[serde(default = "default_thickness")]
    pub highlight_thickness: u32,

    /// Force regenerate highlights even if they exist
    #[serde(default)]
    pub highlight_force: bool,

    /// Pack UI images into 4k atlas textures before syncing
    #[serde(default)]
    pub atlas: bool,

    /// Padding (in pixels) around each sprite in the atlas
    #[serde(default = "default_atlas_padding")]
    pub atlas_padding: u32,

    /// Scratch directory for intermediate/generated files
    #[serde(default = "default_scratch_dir")]
    pub scratch_dir: PathBuf,
}

fn default_thickness() -> u32 {
    1
}

fn default_atlas_padding() -> u32 {
    4
}

fn default_scratch_dir() -> PathBuf {
    PathBuf::from(".truffle")
}

impl TruffleConfig {
    /// Read truffle.toml from the current directory
    pub async fn read() -> Result<Self> {
        let config_str = fs::read_to_string(FILE_NAME)
            .await
            .context("Failed to read truffle.toml")?;

        let config: TruffleConfig =
            toml::from_str(&config_str).context("Failed to parse truffle.toml")?;

        Ok(config)
    }

    /// Convert to Asphalt config (for passing to Asphalt functions)
    pub fn to_asphalt_config(&self) -> &AsphaltConfig {
        &self.asphalt
    }
}
