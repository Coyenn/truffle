use anyhow::{Context, Result};
use asphalt::config::Config as AsphaltConfig;
use fs_err::tokio as fs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const FILE_NAME: &str = "truffle.toml";

/// Fused Truffle configuration.
///
/// One `truffle.toml` holds everything: the Asphalt basics (`creator`,
/// `inputs`) plus all former `[truffle]` options flattened to top level,
/// sync outputs, auth, and the optional `[font]` preset.
///
/// Codegen is intentionally not configurable: Truffle always generates nested
/// tables with extensions kept plus TypeScript declarations, which is the
/// only shape the sync pipeline understands.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TruffleConfig {
    /// Asphalt configuration (`creator`, `inputs`).
    #[serde(flatten)]
    pub asphalt: AsphaltConfig,

    /// Roblox Open Cloud API key (`asset:read`, `asset:write`).
    ///
    /// Precedence: `--api-key` flag, then the `TRUFFLE_API_KEY` environment
    /// variable (`.env` is loaded automatically), then this field.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Replicate API token for `truffle image generate`.
    ///
    /// Precedence: `--replicate-token` flag, then the `REPLICATE_API_TOKEN`
    /// environment variable (`.env` is loaded automatically), then this field.
    #[serde(default)]
    pub replicate_token: Option<String>,

    /// Existing Luau asset catalog to read (also the merge base for `--sync-only`).
    #[serde(default = "default_assets_input")]
    pub assets_input: PathBuf,

    /// Augmented Luau asset catalog to write.
    #[serde(default = "default_assets_output")]
    pub assets_output: PathBuf,

    /// TypeScript declarations to write next to the Luau catalog.
    #[serde(default = "default_dts_output")]
    pub dts_output: PathBuf,

    /// Root folder containing PNG sources.
    #[serde(default = "default_images_folder")]
    pub images_folder: PathBuf,

    /// Generate `*-highlight.png` siblings before sync so they get synced too.
    #[serde(default)]
    pub auto_highlight: bool,

    /// Outline thickness in pixels when auto-highlighting.
    #[serde(default = "default_thickness")]
    pub highlight_thickness: u32,

    /// Overwrite existing highlight variants when auto-highlighting.
    #[serde(default)]
    pub highlight_force: bool,

    /// Pack sprites into atlas textures before upload.
    #[serde(default)]
    pub atlas: bool,

    /// Square atlas texture size (must be a power of two).
    #[serde(default = "default_atlas_size")]
    pub atlas_size: u32,

    /// Padding in pixels around each sprite in the atlas.
    #[serde(default = "default_atlas_padding")]
    pub atlas_padding: u32,

    /// Image keys to exclude from atlas packing (synced individually).
    #[serde(default)]
    pub atlas_exclude: Vec<String>,

    /// Scratch directory for intermediate/generated files.
    ///
    /// Layout:
    /// - `{scratch}/atlases/` — packed atlas PNG textures
    /// - `{scratch}/sync/` — Asphalt codegen scratch (`atlases.*`, `unatlased/assets.*`)
    /// - `{scratch}/sync/subset/` — transient output for `--sync-only` (removed after merge)
    #[serde(default = "default_scratch_dir")]
    pub scratch_dir: PathBuf,

    /// Optional shared defaults for `truffle font`. CLI flags win over these,
    /// and these win over the built-in defaults.
    #[serde(default)]
    pub font: FontPreset,
}

/// Shared defaults for `truffle font`.
///
/// Every field is optional: unset means "fall back to the built-in default".
/// Input/output paths stay per-invocation CLI arguments.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(default)]
pub struct FontPreset {
    /// Padding in pixels around each glyph in the atlas.
    pub padding: Option<u32>,
    /// Charset string; glyphs are packed in this order.
    pub charset: Option<String>,
    /// Path to a UTF-8 text file containing the charset (overrides `charset`).
    pub charset_file: Option<PathBuf>,
    /// Rasterization pixel size (fontdue px). Derived from line height when unset.
    pub px: Option<f32>,
    /// Design line height in pixels (layout reference size).
    pub line_height: Option<u32>,
    /// Maximum atlas page size (square, power of two).
    pub max_atlas_size: Option<u32>,
    /// Target minimum ink gap for kerning class generation.
    pub kerning_gap: Option<u32>,
    /// Output Luau metadata module path. Defaults to OUTPUT_PNG with `.luau`.
    pub luau: Option<PathBuf>,
    /// Output TypeScript declaration path. Defaults to OUTPUT_PNG with `.d.ts`.
    pub dts: Option<PathBuf>,
    /// Copy the truffle-text runtime into this directory.
    pub runtime_out: Option<PathBuf>,
    /// Outline dilation in pixels. Unset/0 disables outline generation.
    pub outline: Option<u32>,
    /// Output PNG path for the outline variant. Defaults to OUTPUT_PNG with `_outline` suffix.
    pub outline_png: Option<PathBuf>,
    /// Disable anti-aliasing (hard 0/255 alpha).
    pub no_antialias: Option<bool>,
}

fn default_assets_input() -> PathBuf {
    PathBuf::from("src/shared/data/assets/assets.luau")
}

fn default_assets_output() -> PathBuf {
    PathBuf::from("src/shared/data/assets/assets.luau")
}

fn default_dts_output() -> PathBuf {
    PathBuf::from("src/shared/data/assets/assets.d.ts")
}

fn default_images_folder() -> PathBuf {
    PathBuf::from("assets/images")
}

fn default_thickness() -> u32 {
    1
}

fn default_atlas_padding() -> u32 {
    4
}

fn default_atlas_size() -> u32 {
    1024
}

fn default_scratch_dir() -> PathBuf {
    PathBuf::from(".truffle")
}

fn parse(config_str: &str) -> Result<TruffleConfig> {
    let value: toml::Value = toml::from_str(config_str).context("Failed to parse truffle.toml")?;

    if let toml::Value::Table(ref table) = value {
        if table.contains_key("truffle") {
            anyhow::bail!(
                "The [truffle] table was removed: move its keys to the top level of truffle.toml \
                 (e.g. `[truffle] atlas = true` becomes `atlas = true`)."
            );
        }
        if table.contains_key("codegen") {
            anyhow::bail!(
                "The [codegen] table was removed: Truffle always generates nested tables with \
                 extensions kept plus TypeScript declarations. Delete the [codegen] table."
            );
        }
    }

    toml::from_str(config_str).context("Failed to deserialize truffle.toml")
}

impl TruffleConfig {
    /// Read truffle.toml from the current directory.
    pub async fn read() -> Result<Self> {
        let config_str = fs::read_to_string(FILE_NAME)
            .await
            .context("Failed to read truffle.toml")?;

        let mut config = parse(&config_str)?;
        config.asphalt.project_dir = PathBuf::from(".");
        Ok(config)
    }
}

impl FontPreset {
    /// Read just the `[font]` table from truffle.toml in the current directory.
    ///
    /// Best-effort by design: a missing/unparseable file or table yields
    /// defaults, so `truffle font` keeps working standalone and never fails
    /// over unrelated config content.
    pub fn read_blocking() -> Self {
        #[derive(Deserialize)]
        struct Partial {
            #[serde(default)]
            font: FontPreset,
        }

        std::fs::read_to_string(FILE_NAME)
            .ok()
            .and_then(|config_str| toml::from_str::<Partial>(&config_str).ok())
            .map(|partial| partial.font)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
[creator]
type = "user"
id = 1234

[inputs.assets]
path = "assets/images/**/*"
output_path = "src/shared/data/assets"
"#;

    #[test]
    fn minimal_config_uses_builtin_defaults() {
        let config: TruffleConfig = parse(MINIMAL).unwrap();
        assert_eq!(config.api_key, None);
        assert_eq!(config.replicate_token, None);
        assert_eq!(
            config.assets_input,
            PathBuf::from("src/shared/data/assets/assets.luau")
        );
        assert_eq!(
            config.assets_output,
            PathBuf::from("src/shared/data/assets/assets.luau")
        );
        assert_eq!(
            config.dts_output,
            PathBuf::from("src/shared/data/assets/assets.d.ts")
        );
        assert_eq!(config.images_folder, PathBuf::from("assets/images"));
        assert!(!config.auto_highlight);
        assert_eq!(config.highlight_thickness, 1);
        assert!(!config.highlight_force);
        assert!(!config.atlas);
        assert_eq!(config.atlas_size, 1024);
        assert_eq!(config.atlas_padding, 4);
        assert!(config.atlas_exclude.is_empty());
        assert_eq!(config.scratch_dir, PathBuf::from(".truffle"));
        assert_eq!(config.font.padding, None);
    }

    #[test]
    fn full_config_parses() {
        let config: TruffleConfig = parse(
            r#"
api_key = "key-from-config"
replicate_token = "replicate-from-config"
assets_input = "in.luau"
assets_output = "out.luau"
dts_output = "out.d.ts"
images_folder = "images"
auto_highlight = true
highlight_thickness = 2
highlight_force = true
atlas = true
atlas_size = 2048
atlas_padding = 8
atlas_exclude = ["backgrounds"]
scratch_dir = ".scratch"

[creator]
type = "group"
id = 99

[inputs.assets]
path = "images/**/*"
output_path = "out"
bleed = false

[font]
padding = 5
px = 121.0
line_height = 95
no_antialias = true
"#,
        )
        .unwrap();
        assert_eq!(config.api_key.as_deref(), Some("key-from-config"));
        assert_eq!(
            config.replicate_token.as_deref(),
            Some("replicate-from-config")
        );
        assert_eq!(config.assets_input, PathBuf::from("in.luau"));
        assert_eq!(config.images_folder, PathBuf::from("images"));
        assert!(config.auto_highlight);
        assert_eq!(config.highlight_thickness, 2);
        assert!(config.atlas);
        assert_eq!(config.atlas_size, 2048);
        assert_eq!(config.atlas_exclude, vec!["backgrounds".to_string()]);
        assert_eq!(config.scratch_dir, PathBuf::from(".scratch"));
        assert_eq!(config.font.padding, Some(5));
        assert_eq!(config.font.px, Some(121.0));
        assert_eq!(config.font.line_height, Some(95));
        assert_eq!(config.font.no_antialias, Some(true));
        assert_eq!(config.font.charset, None);
    }

    #[test]
    fn legacy_truffle_table_errors() {
        let err = parse(&format!("{MINIMAL}\n[truffle]\natlas = true\n")).unwrap_err();
        assert!(err.to_string().contains("[truffle]"));
    }

    #[test]
    fn legacy_codegen_table_errors() {
        let err = parse(&format!(
            "{MINIMAL}\n[codegen]\nstyle = \"nested\"\ntypescript = true\n"
        ))
        .unwrap_err();
        assert!(err.to_string().contains("[codegen]"));
    }
}
