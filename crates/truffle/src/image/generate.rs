//! Generate images from prompt files via the Replicate API.
//!
//! The flow is: create a prediction from the parsed prompt file, poll until it
//! reaches a terminal status, download every output URL, then chain each raw
//! image through the Pixel Snapper so the checked-in artifact is crisp pixel
//! art. Raw downloads are kept next to their `-snapped.png` siblings so a
//! review loop can compare candidates before cleanup.

use crate::prompt::ResolvedReplicate;
use anyhow::{ensure, Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const API_BASE: &str = "https://api.replicate.com/v1";

/// Build an HTTP client for Replicate API calls and output downloads.
pub fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("truffle/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed to build HTTP client")
}

/// Plan one raw output path per Replicate output URL.
///
/// Resolution order for the base: `--output`, then the prompt file's `output`,
/// then `<prompt-stem>-generated.png` beside the prompt file. A single output
/// writes to the base directly (an existing directory receives
/// `<prompt-stem>.png`). Multiple outputs expand a `stem.png` base into
/// `stem-1.png`, `stem-2.png`, … or fill an existing directory with
/// `<prompt-stem>-N.png` files.
pub fn plan_outputs(
    prompt_path: &Path,
    cli_output: Option<&Path>,
    file_output: Option<&Path>,
    count: usize,
) -> Result<Vec<PathBuf>> {
    ensure!(
        count > 0,
        "Replicate returned no output URLs to plan paths for"
    );

    let prompt_stem = prompt_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("generated");

    let base = cli_output
        .or(file_output)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| prompt_path.with_file_name(format!("{prompt_stem}-generated.png")));

    if count == 1 {
        if base.is_dir() {
            return Ok(vec![base.join(format!("{prompt_stem}.png"))]);
        }
        ensure_png(&base)?;
        return Ok(vec![base]);
    }

    if base.is_dir() {
        return Ok((1..=count)
            .map(|index| base.join(format!("{prompt_stem}-{index}.png")))
            .collect());
    }

    ensure_png(&base)?;
    let stem = base
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .with_context(|| format!("Output has no file stem: {}", base.display()))?;
    let parent = base.parent().unwrap_or_else(|| Path::new(""));
    Ok((1..=count)
        .map(|index| parent.join(format!("{stem}-{index}.png")))
        .collect())
}

fn ensure_png(path: &Path) -> Result<()> {
    ensure!(
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png")),
        "Output must have a .png extension: {}",
        path.display()
    );
    Ok(())
}

/// `<stem>-snapped.png` sibling for a raw generated image.
pub fn snapped_sibling(raw: &Path) -> Result<PathBuf> {
    let stem = raw
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .with_context(|| format!("Output has no file stem: {}", raw.display()))?;
    Ok(raw.with_file_name(format!("{stem}-snapped.png")))
}

/// PNG dimensions of a file on disk, for scale validation.
pub fn png_dimensions(path: &Path) -> Result<(u32, u32)> {
    image::image_dimensions(path)
        .with_context(|| format!("Failed to read dimensions of {}", path.display()))
}

/// Describe a suspicious snap where the pixel grid likely collapsed.
///
/// Returns a warning message when the snapped output is tiny (a single-digit
/// sprite from a large raw almost never survives cleanup) or the implied
/// pixel blocks are enormous (the snapper found a coarse period instead of
/// the real grid). Returns `None` for sane scales.
pub fn scale_warning(
    raw_width: u32,
    raw_height: u32,
    snapped_width: u32,
    snapped_height: u32,
) -> Option<String> {
    if snapped_width.min(snapped_height) < 16 {
        return Some(format!(
            "snapped output is only {snapped_width}x{snapped_height} from a {raw_width}x{raw_height} raw: \
             the pixel grid likely collapsed; inspect before cleanup"
        ));
    }
    let block_width = raw_width as f64 / snapped_width.max(1) as f64;
    let block_height = raw_height as f64 / snapped_height.max(1) as f64;
    if block_width > 64.0 || block_height > 64.0 {
        return Some(format!(
            "implied pixel blocks are ~{block_width:.0}x{block_height:.0} px: \
             the snapper may have merged the real grid; inspect before cleanup"
        ));
    }
    None
}

/// Run the prompt on Replicate and return every output image URL.
pub async fn generate_image_urls(
    client: &reqwest::Client,
    token: &str,
    request: &ResolvedReplicate,
) -> Result<Vec<String>> {
    let version = match &request.version {
        Some(version) => format!("{}/{}:{version}", request.owner, request.name),
        None => format!("{}/{}", request.owner, request.name),
    };
    let body = serde_json::json!({ "version": version, "input": request.input });

    let response = client
        .post(format!("{API_BASE}/predictions"))
        .bearer_auth(token)
        .header("Prefer", "wait")
        .json(&body)
        .send()
        .await
        .context("Failed to create Replicate prediction")?;
    let prediction: Prediction = error_for_status_with_body(response)
        .await?
        .json()
        .await
        .context("Failed to decode Replicate prediction response")?;
    let mut prediction = prediction;

    let started = Instant::now();
    let timeout = Duration::from_secs(request.timeout_secs);
    let interval = Duration::from_secs(request.poll_interval_secs);

    loop {
        match prediction.status.as_str() {
            "succeeded" => return output_urls(&prediction.output),
            "failed" | "canceled" => {
                let detail = prediction
                    .error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| prediction.status.clone());
                anyhow::bail!("Replicate prediction {}: {detail}", prediction.status);
            }
            _ => {
                ensure!(
                    started.elapsed() < timeout,
                    "Timed out after {}s waiting for Replicate prediction{}",
                    timeout.as_secs(),
                    prediction
                        .id
                        .as_deref()
                        .map(|id| format!(" ({id})"))
                        .unwrap_or_default()
                );
                tokio::time::sleep(interval).await;
                prediction = get_prediction(client, token, &prediction).await?;
            }
        }
    }
}

async fn get_prediction(
    client: &reqwest::Client,
    token: &str,
    prediction: &Prediction,
) -> Result<Prediction> {
    let url = match (&prediction.urls, &prediction.id) {
        (Some(urls), _) => urls.get.clone(),
        (None, Some(id)) => format!("{API_BASE}/predictions/{id}"),
        (None, None) => anyhow::bail!("Replicate response has no prediction id or status URL"),
    };
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .context("Failed to poll Replicate prediction")?;
    error_for_status_with_body(response)
        .await?
        .json()
        .await
        .context("Failed to decode Replicate prediction response")
}

/// Download one URL to `path`, creating parent directories as needed.
pub async fn download_to_file(client: &reqwest::Client, url: &str, path: &Path) -> Result<()> {
    let bytes = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to download {url}"))?;
    let bytes = error_for_status_with_body(bytes)
        .await?
        .bytes()
        .await
        .with_context(|| format!("Failed to read download body from {url}"))?;
    ensure!(!bytes.is_empty(), "Download from {url} was empty");

    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }
    std::fs::write(path, &bytes)
        .with_context(|| format!("Failed to write output {}", path.display()))?;
    Ok(())
}

fn output_urls(output: &Option<serde_json::Value>) -> Result<Vec<String>> {
    let urls = match output {
        Some(serde_json::Value::String(url)) => vec![url.clone()],
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .with_context(|| format!("Unsupported Replicate output entry: {item}"))
                    .map(str::to_string)
            })
            .collect::<Result<Vec<_>>>()?,
        Some(other) => anyhow::bail!("Unsupported Replicate output: {other}"),
        None => anyhow::bail!("Replicate prediction succeeded but returned no output"),
    };
    let urls: Vec<String> = urls
        .into_iter()
        .filter(|url| !url.trim().is_empty())
        .collect();
    ensure!(
        !urls.is_empty(),
        "Replicate prediction succeeded but returned no output URLs"
    );
    Ok(urls)
}

#[derive(serde::Deserialize)]
struct Prediction {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    output: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<serde_json::Value>,
    #[serde(default)]
    urls: Option<PredictionUrls>,
}

#[derive(serde::Deserialize)]
struct PredictionUrls {
    #[serde(default)]
    get: String,
}

async fn error_for_status_with_body(response: reqwest::Response) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let detail: String = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|json| json.get("detail").cloned())
        .and_then(|detail| detail.as_str().map(str::to_string))
        .unwrap_or_else(|| body.trim().to_string());
    anyhow::bail!("Replicate API error {status}: {detail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_output_defaults_beside_prompt() {
        assert_eq!(
            plan_outputs(Path::new("prompts/slime.md"), None, None, 1).unwrap(),
            vec![PathBuf::from("prompts/slime-generated.png")]
        );
    }

    #[test]
    fn single_output_prefers_cli_over_file() {
        assert_eq!(
            plan_outputs(
                Path::new("prompts/slime.md"),
                Some(Path::new("out.png")),
                Some(Path::new("file.png")),
                1
            )
            .unwrap(),
            vec![PathBuf::from("out.png")]
        );
        assert_eq!(
            plan_outputs(
                Path::new("prompts/slime.md"),
                None,
                Some(Path::new("file.png")),
                1
            )
            .unwrap(),
            vec![PathBuf::from("file.png")]
        );
    }

    #[test]
    fn single_output_rejects_non_png() {
        assert!(plan_outputs(Path::new("p.md"), Some(Path::new("out.jpg")), None, 1).is_err());
    }

    #[test]
    fn multiple_outputs_expand_stem() {
        assert_eq!(
            plan_outputs(Path::new("p.md"), Some(Path::new("out/slime.png")), None, 3).unwrap(),
            vec![
                PathBuf::from("out/slime-1.png"),
                PathBuf::from("out/slime-2.png"),
                PathBuf::from("out/slime-3.png"),
            ]
        );
    }

    #[test]
    fn multiple_outputs_fill_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            plan_outputs(Path::new("prompts/slime.md"), Some(dir.path()), None, 2).unwrap(),
            vec![
                dir.path().join("slime-1.png"),
                dir.path().join("slime-2.png"),
            ]
        );
    }

    #[test]
    fn single_output_fills_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            plan_outputs(Path::new("prompts/slime.md"), Some(dir.path()), None, 1).unwrap(),
            vec![dir.path().join("slime.png")]
        );
    }

    #[test]
    fn zero_outputs_is_an_error() {
        assert!(plan_outputs(Path::new("p.md"), None, None, 0).is_err());
    }

    #[test]
    fn snapped_sibling_appends_suffix() {
        assert_eq!(
            snapped_sibling(Path::new("out/slime.png")).unwrap(),
            PathBuf::from("out/slime-snapped.png")
        );
    }

    #[test]
    fn output_urls_accepts_string_or_array() {
        assert_eq!(
            output_urls(&Some(serde_json::Value::String("https://x/y.png".into()))).unwrap(),
            vec!["https://x/y.png".to_string()]
        );
        assert_eq!(
            output_urls(&Some(serde_json::json!(["https://a", "https://b"]))).unwrap(),
            vec!["https://a".to_string(), "https://b".to_string()]
        );
        assert!(output_urls(&None).is_err());
        assert!(output_urls(&Some(serde_json::json!([]))).is_err());
        assert!(output_urls(&Some(serde_json::json!({"uri": "x"}))).is_err());
    }

    #[test]
    fn scale_warning_flags_collapsed_snaps() {
        // Healthy house-scale snap: ~11 px blocks.
        assert_eq!(scale_warning(5504, 3072, 505, 291), None);
        // Healthy sprite-scale snap: 16 px blocks.
        assert_eq!(scale_warning(2048, 2048, 128, 128), None);
        // Collapsed to a 10x10 thumbnail.
        assert!(scale_warning(2048, 2048, 10, 10).is_some());
        // Merged grid: enormous implied blocks.
        assert!(scale_warning(2048, 2048, 16, 16).is_some());
    }
}
