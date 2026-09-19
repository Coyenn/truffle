//! Prompt Markdown files for `truffle image generate`.
//!
//! A prompt file is Markdown with YAML front matter. The front matter pins the
//! Replicate model plus all of its generation parameters, so a prompt is fully
//! reproducible from one file. The Markdown body is the image prompt text.
//!
//! ```markdown
//! ---
//! version: 1
//! replicate:
//!   model: "google/nano-banana-2"
//!   version: "abdf4a3f..." # optional pin; may also be inline as model:version
//!   input:
//!     aspect_ratio: "1:1"
//!     resolution: "2K"
//!     output_format: "png"
//!   timeout_secs: 600
//! snap:
//!   colors: 16
//! output: "assets/generated/slime.png"
//! ---
//!
//! A cozy pixel-art slime on a plain white background ...
//! ```
//!
//! The body is sent as `replicate.input.prompt` unless that key is already set
//! explicitly in `input`, in which case the explicit value wins. When the body
//! contains a fenced code block (Caramel prompts keep the runnable prompt in a
//! ```text fence below human instructions), the first fenced block is sent and
//! the surrounding Markdown is ignored. `input` is a generic passthrough map
//! so any Replicate model works without code changes.

use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Front matter format version written by this Truffle release.
pub const FORMAT_VERSION: u32 = 1;

/// Default Replicate polling timeout when `replicate.timeout_secs` is unset.
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

/// Default delay between Replicate status polls.
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 2;

/// Default Pixel Snapper palette size when `snap.colors` is unset.
pub const DEFAULT_SNAP_COLORS: usize = 16;

/// A parsed prompt file: resolved Replicate request plus snap/output settings.
pub struct PromptFile {
    /// Resolved Replicate model, version, and full input (prompt injected).
    pub replicate: ResolvedReplicate,
    /// Pixel Snapper settings for chaining after download.
    pub snap: SnapSpec,
    /// Desired raw output path from front matter, if any.
    pub output: Option<PathBuf>,
}

/// Resolved Replicate request derived from front matter plus the body.
pub struct ResolvedReplicate {
    /// Model owner (`owner` in `owner/name`).
    pub owner: String,
    /// Model name (`name` in `owner/name`).
    pub name: String,
    /// Pinned model version hash, if any.
    pub version: Option<String>,
    /// Full model input with `prompt` injected.
    pub input: BTreeMap<String, serde_json::Value>,
    /// Seconds to poll before giving up.
    pub timeout_secs: u64,
    /// Seconds between status polls.
    pub poll_interval_secs: u64,
}

/// Pixel Snapper settings from `snap` front matter.
pub struct SnapSpec {
    /// Palette colors quantized before snapping.
    pub colors: usize,
    /// Override the auto-detected pixel size.
    pub pixel_size: Option<f64>,
    /// Constrain the output to comma-separated 6-digit hex colors.
    pub palette: Option<String>,
    /// Constrain the output to the visible colors of a palette PNG.
    pub palette_png: Option<PathBuf>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontMatter {
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    replicate: Option<ReplicateFront>,
    #[serde(default)]
    snap: Option<SnapFront>,
    #[serde(default)]
    output: Option<PathBuf>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplicateFront {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    input: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    poll_interval_secs: Option<u64>,
}

#[derive(serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SnapFront {
    #[serde(default)]
    colors: Option<usize>,
    #[serde(default)]
    pixel_size: Option<f64>,
    #[serde(default)]
    palette: Option<String>,
    #[serde(default)]
    palette_png: Option<PathBuf>,
}

/// Read and parse a prompt Markdown file.
pub fn read_prompt_file(path: &Path) -> Result<PromptFile> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read prompt file {}", path.display()))?;
    parse_prompt_file(&content)
        .with_context(|| format!("Failed to parse prompt file {}", path.display()))
}

fn parse_prompt_file(content: &str) -> Result<PromptFile> {
    let (front_matter, body) = split_front_matter(content)?;
    let matter: FrontMatter = serde_yaml::from_str(&front_matter)
        .context("Invalid YAML front matter (expected `---` block)")?;

    if let Some(version) = matter.version {
        ensure!(
            version == FORMAT_VERSION,
            "Unsupported prompt format version {version} (expected {FORMAT_VERSION})"
        );
    }

    let replicate_front = matter
        .replicate
        .context("Missing `replicate` table in prompt front matter")?;
    let model_raw = replicate_front
        .model
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    ensure!(
        !model_raw.is_empty(),
        "Missing `replicate.model` in prompt front matter (expected `owner/name`)"
    );
    let (owner, name, inline_version) = parse_model(&model_raw)?;
    if inline_version.is_some() && replicate_front.version.is_some() {
        anyhow::bail!(
            "Specify the model version once: either inline (`model: \"owner/name:version\"`) or via `replicate.version`, not both"
        );
    }
    let version = replicate_front.version.or(inline_version);

    let mut input = replicate_front.input;
    match input.get("prompt") {
        Some(serde_json::Value::String(explicit)) if !explicit.trim().is_empty() => {}
        Some(_) => anyhow::bail!(
            "`replicate.input.prompt` must be a non-empty string when set (omit it to use the Markdown body)"
        ),
        None => {
            let prompt = extract_fenced_prompt(&body)?
                .unwrap_or_else(|| body.trim().to_string());
            ensure!(
                !prompt.is_empty(),
                "Prompt body is empty: write the image prompt as Markdown below the front matter, or set `replicate.input.prompt` explicitly"
            );
            input.insert("prompt".to_string(), serde_json::Value::String(prompt));
        }
    };

    let timeout_secs = replicate_front.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
    ensure!(
        timeout_secs > 0,
        "`replicate.timeout_secs` must be greater than 0"
    );
    let poll_interval_secs = replicate_front
        .poll_interval_secs
        .unwrap_or(DEFAULT_POLL_INTERVAL_SECS);
    ensure!(
        poll_interval_secs > 0,
        "`replicate.poll_interval_secs` must be greater than 0"
    );

    let snap_front = matter.snap.unwrap_or_default();
    let colors = snap_front.colors.unwrap_or(DEFAULT_SNAP_COLORS);
    ensure!(colors > 0, "`snap.colors` must be greater than 0");
    if let Some(pixel_size) = snap_front.pixel_size {
        ensure!(
            pixel_size.is_finite() && pixel_size > 0.0,
            "`snap.pixel_size` must be a positive number"
        );
    }

    Ok(PromptFile {
        replicate: ResolvedReplicate {
            owner,
            name,
            version,
            input,
            timeout_secs,
            poll_interval_secs,
        },
        snap: SnapSpec {
            colors,
            pixel_size: snap_front.pixel_size,
            palette: snap_front.palette,
            palette_png: snap_front.palette_png,
        },
        output: matter.output,
    })
}

/// Split `---` front matter from the Markdown body.
fn split_front_matter(content: &str) -> Result<(String, String)> {
    let mut lines = content.split('\n');
    let first = lines.next().unwrap_or("").trim();
    ensure!(
        first == "---",
        "Prompt file must start with a `---` YAML front matter block"
    );

    let mut matter_lines = Vec::new();
    let mut body_lines: Option<Vec<&str>> = None;
    for line in lines {
        if body_lines.is_none() && line.trim() == "---" {
            body_lines = Some(Vec::new());
            continue;
        }
        match body_lines.as_mut() {
            Some(body) => body.push(line),
            None => matter_lines.push(line),
        }
    }
    ensure!(
        body_lines.is_some(),
        "Prompt file front matter is missing its closing `---`"
    );
    Ok((
        matter_lines.join("\n"),
        body_lines.unwrap_or_default().join("\n"),
    ))
}

/// Extract the first fenced code block from the Markdown body, if any.
///
/// Caramel prompts keep human instructions around the runnable prompt, so the
/// fenced block is what the model must receive. Returns `Ok(None)` when the
/// body has no fence (the whole body is the prompt).
fn extract_fenced_prompt(body: &str) -> Result<Option<String>> {
    let mut lines = body.lines();
    let mut fenced: Option<Vec<&str>> = None;
    for line in lines.by_ref() {
        if line.trim_start().starts_with("```") {
            fenced = Some(Vec::new());
            break;
        }
    }
    let mut fenced = match fenced {
        Some(fenced) => fenced,
        None => return Ok(None),
    };
    for line in lines {
        if line.trim_start().starts_with("```") {
            let prompt = fenced.join("\n").trim().to_string();
            ensure!(
                !prompt.is_empty(),
                "First fenced code block in the prompt body is empty"
            );
            return Ok(Some(prompt));
        }
        fenced.push(line);
    }
    anyhow::bail!("Prompt body has an unclosed fenced code block")
}

/// Parse `owner/name` with an optional `:version` suffix.
fn parse_model(raw: &str) -> Result<(String, String, Option<String>)> {
    let (model_part, inline_version) = match raw.split_once(':') {
        Some((model_part, version)) => {
            ensure!(
                !version.trim().is_empty(),
                "Invalid `replicate.model` {raw:?}: version after `:` must not be empty"
            );
            (model_part, Some(version.trim().to_string()))
        }
        None => (raw, None),
    };
    let mut parts = model_part.split('/');
    let owner = parts.next().unwrap_or("").trim().to_string();
    let name = parts.next().unwrap_or("").trim().to_string();
    ensure!(
        !owner.is_empty()
            && !name.is_empty()
            && parts.next().is_none()
            && !model_part.contains(':'),
        "Invalid `replicate.model` {raw:?}: expected `owner/name` with an optional `:version` suffix"
    );
    Ok((owner, name, inline_version))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "---\nreplicate:\n  model: \"owner/name\"\n---\n\nA pixel-art slime.\n";

    #[test]
    fn minimal_prompt_injects_body_as_input_prompt() {
        let parsed = parse_prompt_file(MINIMAL).unwrap();
        assert_eq!(parsed.replicate.owner, "owner");
        assert_eq!(parsed.replicate.name, "name");
        assert_eq!(parsed.replicate.version, None);
        assert_eq!(
            parsed.replicate.input.get("prompt").unwrap(),
            &serde_json::Value::String("A pixel-art slime.".to_string())
        );
        assert_eq!(parsed.replicate.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(parsed.snap.colors, DEFAULT_SNAP_COLORS);
        assert_eq!(parsed.output, None);
    }

    #[test]
    fn full_prompt_resolves_everything() {
        let parsed = parse_prompt_file(
            "---\nversion: 1\nreplicate:\n  model: \"owner/name\"\n  version: \"abc123\"\n  input:\n    aspect_ratio: \"16:9\"\n    num_outputs: 2\n  timeout_secs: 60\n  poll_interval_secs: 5\nsnap:\n  colors: 8\n  pixel_size: 4.0\noutput: \"out/slime.png\"\n---\nBody prompt.\n",
        )
        .unwrap();
        assert_eq!(parsed.replicate.version.as_deref(), Some("abc123"));
        assert_eq!(
            parsed.replicate.input.get("aspect_ratio").unwrap(),
            &serde_json::Value::String("16:9".to_string())
        );
        assert_eq!(parsed.replicate.timeout_secs, 60);
        assert_eq!(parsed.replicate.poll_interval_secs, 5);
        assert_eq!(parsed.snap.colors, 8);
        assert_eq!(parsed.snap.pixel_size, Some(4.0));
        assert_eq!(parsed.output, Some(PathBuf::from("out/slime.png")));
    }

    #[test]
    fn inline_version_is_accepted() {
        let parsed =
            parse_prompt_file("---\nreplicate:\n  model: \"owner/name:abc123\"\n---\nBody.\n")
                .unwrap();
        assert_eq!(parsed.replicate.owner, "owner");
        assert_eq!(parsed.replicate.version.as_deref(), Some("abc123"));
    }

    #[test]
    fn explicit_input_prompt_wins_over_body() {
        let parsed = parse_prompt_file(
            "---\nreplicate:\n  model: \"owner/name\"\n  input:\n    prompt: \"explicit\"\n---\nBody is ignored.\n",
        )
        .unwrap();
        assert_eq!(
            parsed.replicate.input.get("prompt").unwrap(),
            &serde_json::Value::String("explicit".to_string())
        );
    }

    #[test]
    fn fenced_block_is_sent_instead_of_surrounding_markdown() {
        let parsed = parse_prompt_file(
            "---\nreplicate:\n  model: \"owner/name\"\n---\n# Title\n\nCopy the prompt below.\n\n```text\nRunnable prompt.\n```\n",
        )
        .unwrap();
        assert_eq!(
            parsed.replicate.input.get("prompt").unwrap(),
            &serde_json::Value::String("Runnable prompt.".to_string())
        );
    }

    #[test]
    fn explicit_input_prompt_wins_over_fenced_block() {
        let parsed = parse_prompt_file(
            "---\nreplicate:\n  model: \"owner/name\"\n  input:\n    prompt: \"explicit\"\n---\n# Title\n\n```text\nFenced.\n```\n",
        )
        .unwrap();
        assert_eq!(
            parsed.replicate.input.get("prompt").unwrap(),
            &serde_json::Value::String("explicit".to_string())
        );
    }

    #[test]
    fn unclosed_fence_fails() {
        assert!(parse_prompt_file(
            "---\nreplicate:\n  model: \"owner/name\"\n---\n# Title\n\n```text\nNo closing.\n"
        )
        .is_err());
    }

    #[test]
    fn empty_fence_fails() {
        assert!(parse_prompt_file(
            "---\nreplicate:\n  model: \"owner/name\"\n---\n# Title\n\n```text\n   \n```\n"
        )
        .is_err());
    }

    #[test]
    fn empty_body_without_explicit_prompt_fails() {
        assert!(parse_prompt_file("---\nreplicate:\n  model: \"owner/name\"\n---\n").is_err());
    }

    #[test]
    fn missing_front_matter_fails() {
        assert!(parse_prompt_file("Just a body.\n").is_err());
    }

    #[test]
    fn missing_model_fails() {
        assert!(parse_prompt_file("---\nreplicate:\n  input: {}\n---\nBody.\n").is_err());
    }

    #[test]
    fn conflicting_versions_fail() {
        assert!(parse_prompt_file(
            "---\nreplicate:\n  model: \"owner/name:abc\"\n  version: \"def\"\n---\nBody.\n"
        )
        .is_err());
    }

    #[test]
    fn bad_model_shapes_fail() {
        for model in ["name-only", "a/b/c", "owner/", "/name", "owner/name:"] {
            assert!(
                parse_prompt_file(&format!(
                    "---\nreplicate:\n  model: \"{model}\"\n---\nBody.\n"
                ))
                .is_err(),
                "model {model:?} should fail"
            );
        }
    }

    #[test]
    fn unknown_fields_fail() {
        assert!(parse_prompt_file(
            "---\nreplicate:\n  model: \"owner/name\"\n  bogus: 1\n---\nBody.\n"
        )
        .is_err());
    }

    #[test]
    fn unsupported_version_fails() {
        assert!(parse_prompt_file(
            "---\nversion: 99\nreplicate:\n  model: \"owner/name\"\n---\nBody.\n"
        )
        .is_err());
    }

    #[test]
    fn invalid_timeouts_and_snap_fail() {
        assert!(parse_prompt_file(
            "---\nreplicate:\n  model: \"owner/name\"\n  timeout_secs: 0\n---\nBody.\n"
        )
        .is_err());
        assert!(parse_prompt_file(
            "---\nreplicate:\n  model: \"owner/name\"\nsnap:\n  colors: 0\n---\nBody.\n"
        )
        .is_err());
    }
}
