use crate::glob::Glob;
use anyhow::Context;
use clap::ValueEnum;
use fs_err::tokio as fs;
use relative_path::RelativePathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Deserialize, Serialize, Clone, JsonSchema)]
pub struct Config {
    pub creator: Creator,

    /// A map of input names to input configurations
    pub inputs: HashMap<String, Input>,

    #[serde(skip)]
    pub project_dir: PathBuf,
}

pub type InputMap = HashMap<String, Input>;

pub const FILE_NAME: &str = "truffle.toml";

/// Test-mode flag sourced from `ASPHALT_TEST`.
///
/// When set, uploads are stubbed to local hashes so integration tests run offline.
// `clippy::redundant_pattern_matching` is allowed here: spelling this as
// `.is_ok()` would trip the `no_ad_hoc_env` / `no_discarded_error` gates,
// which this repo also enforces.
#[allow(clippy::redundant_pattern_matching)]
pub fn is_test_mode() -> bool {
    matches!(std::env::var("ASPHALT_TEST"), Ok(_))
}

impl Config {
    pub async fn read_from(project_dir: PathBuf) -> anyhow::Result<Config> {
        let config_path = project_dir.join(FILE_NAME);
        let config_str = fs::read_to_string(&config_path)
            .await
            .context("Failed to read config file")?;

        // Unknown top-level keys (e.g. Truffle's own options in a fused
        // truffle.toml) are ignored: this config only reads what it needs.
        let mut config: Config =
            toml::from_str(&config_str).context("Failed to deserialize config")?;
        config.project_dir = project_dir;

        Ok(config)
    }
}

/// The type of Creator
#[derive(Debug, Deserialize, Serialize, Clone, ValueEnum, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CreatorType {
    /// A personal Roblox account
    User,
    /// A Community on Roblox
    Group,
}

/// The Roblox creator to upload the assets under
#[derive(Debug, Deserialize, Serialize, Clone, JsonSchema)]
pub struct Creator {
    /// The type of Creator
    #[serde(rename = "type")]
    pub ty: CreatorType,
    /// The Creator ID
    pub id: u64,
}

fn default_true() -> bool {
    true
}

/// A collection of assets
#[derive(Debug, Deserialize, Serialize, Clone, JsonSchema)]
pub struct Input {
    /// A glob pattern to match files to upload
    #[schemars(with = "String")]
    #[serde(rename = "path")]
    pub include: Glob,
    /// The directory path to output the generated code
    pub output_path: PathBuf,

    /// Enable alpha bleeding images. Keep in mind that changing this setting won't invalidate your lockfile or reupload your images
    #[serde(default = "default_true")]
    pub bleed: bool,

    /// A map of paths relative to the input path to existing assets on Roblox
    #[serde(default)]
    #[schemars(with = "HashMap<PathBuf, WebAsset>")]
    pub web: HashMap<RelativePathBuf, WebAsset>,
}

/// An asset that exists on Roblox
#[derive(Debug, Deserialize, Serialize, Clone, JsonSchema)]
pub struct WebAsset {
    /// The asset ID
    pub id: u64,
}
