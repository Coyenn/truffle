use crate::image::{generate, snap};
use crate::prompt;
use anyhow::{ensure, Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tokio::runtime::Runtime;
use truffle_config::TruffleConfig;

#[derive(Parser)]
#[command(
    about = "Generate images from a prompt Markdown file via Replicate, then snap them to the pixel grid"
)]
pub struct GenerateArgs {
    /// Prompt Markdown file with YAML front matter (model + generation parameters)
    #[arg(value_name = "PROMPT_FILE")]
    pub prompt_file: PathBuf,

    /// Output PNG file (single output) or output directory (multiple outputs).
    /// Defaults to the prompt file's `output`, else `<stem>-generated.png` beside it.
    #[arg(short, long, value_name = "OUTPUT")]
    pub output: Option<PathBuf>,

    /// Preview the resolved model, inputs, and output paths without calling Replicate
    #[arg(long)]
    pub dry_run: bool,

    /// Overwrite existing raw and snapped outputs
    #[arg(long)]
    pub force: bool,

    /// Skip chaining downloaded images through the Pixel Snapper
    #[arg(long)]
    pub no_snap: bool,

    /// Replicate API token (overrides `REPLICATE_API_TOKEN` env and truffle.toml `replicate_token`)
    #[arg(long, env = "REPLICATE_API_TOKEN")]
    pub replicate_token: Option<String>,
}

pub fn run(args: GenerateArgs) -> bool {
    let dry_run = args.dry_run;
    let rt = Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
        match run_async(args).await {
            Ok(generated) => generated > 0 || dry_run,
            Err(error) => {
                eprintln!("[generate] ERROR: {error:#}");
                false
            }
        }
    })
}

async fn run_async(args: GenerateArgs) -> Result<usize> {
    ensure!(
        args.prompt_file.is_file(),
        "Prompt file does not exist: {}",
        args.prompt_file.display()
    );
    let prompt = prompt::read_prompt_file(&args.prompt_file)?;
    let palette = snap::resolve_palette_option(
        prompt.snap.palette.as_deref(),
        prompt.snap.palette_png.as_deref(),
    )?;

    println!(
        "[generate] Model: {}/{}",
        prompt.replicate.owner, prompt.replicate.name
    );
    if let Some(version) = &prompt.replicate.version {
        println!("[generate] Version: {version}");
    }

    if args.dry_run {
        return dry_run(&args, &prompt);
    }

    let token = resolve_token(args.replicate_token.clone()).await?;
    let client = generate::build_client()?;

    println!("[generate] Creating Replicate prediction …");
    let urls = generate::generate_image_urls(&client, &token, &prompt.replicate).await?;
    println!("[generate] Received {} output(s)", urls.len());

    let outputs = generate::plan_outputs(
        &args.prompt_file,
        args.output.as_deref(),
        prompt.output.as_deref(),
        urls.len(),
    )?;
    ensure_writable(&outputs, args.force, !args.no_snap)?;

    for (url, output) in urls.iter().zip(outputs.iter()) {
        println!("[generate] Downloading {url} -> {}", output.display());
        generate::download_to_file(&client, url, output).await?;
        let (raw_width, raw_height) = generate::png_dimensions(output)?;
        println!(
            "[generate] Generated: {} ({raw_width}x{raw_height})",
            output.display()
        );

        if !args.no_snap {
            let snapped = generate::snapped_sibling(output)?;
            println!(
                "[generate] Snapping {} -> {}",
                output.display(),
                snapped.display()
            );
            snap::snap_file(
                output,
                &snapped,
                prompt.snap.colors,
                prompt.snap.pixel_size,
                palette.as_deref(),
            )
            .with_context(|| format!("Failed to snap {}", output.display()))?;
            let (snapped_width, snapped_height) = generate::png_dimensions(&snapped)?;
            ensure!(
                snapped_width > 0 && snapped_height > 0,
                "Snapped output has empty dimensions: {}",
                snapped.display()
            );
            let block_width = raw_width as f64 / snapped_width as f64;
            let block_height = raw_height as f64 / snapped_height as f64;
            println!(
                "[generate] Snapped: {} ({snapped_width}x{snapped_height}, ~{block_width:.1}x{block_height:.1} px blocks)",
                snapped.display()
            );
            if let Some(warning) =
                generate::scale_warning(raw_width, raw_height, snapped_width, snapped_height)
            {
                eprintln!("[generate] WARNING: {warning}");
            }
        }
    }

    println!("[generate] Done. Generated: {}", outputs.len());
    Ok(outputs.len())
}

fn dry_run(args: &GenerateArgs, prompt: &prompt::PromptFile) -> Result<usize> {
    let count = expected_outputs(prompt);
    let outputs = generate::plan_outputs(
        &args.prompt_file,
        args.output.as_deref(),
        prompt.output.as_deref(),
        count,
    )?;

    println!("[generate] DRY-RUN: Would call Replicate with input:");
    println!(
        "{}",
        serde_json::to_string_pretty(&prompt.replicate.input)
            .context("Failed to render dry-run input")?
    );
    for output in &outputs {
        println!("[generate] DRY-RUN: Would generate {}", output.display());
        if !args.no_snap {
            println!(
                "[generate] DRY-RUN: Would snap to {}",
                generate::snapped_sibling(output)?.display()
            );
        }
    }
    println!(
        "[generate] DRY-RUN: Would generate {} file(s) (expecting {count} output(s))",
        outputs.len()
    );
    Ok(outputs.len())
}

/// Read the advertised `num_outputs` model input, defaulting to one.
fn expected_outputs(prompt: &prompt::PromptFile) -> usize {
    prompt
        .replicate
        .input
        .get("num_outputs")
        .and_then(|value| value.as_u64())
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| *count > 0)
        .unwrap_or(1)
}

/// Fail before any costly API call when an output (or its snapped sibling)
/// already exists and `--force` was not passed.
fn ensure_writable(outputs: &[PathBuf], force: bool, with_snap: bool) -> Result<()> {
    if force {
        return Ok(());
    }
    let mut existing = Vec::new();
    for output in outputs {
        if output.exists() {
            existing.push(output.clone());
        }
        if with_snap {
            if let Ok(snapped) = generate::snapped_sibling(output) {
                if snapped.exists() {
                    existing.push(snapped);
                }
            }
        }
    }
    ensure!(
        existing.is_empty(),
        "Output(s) already exist (pass --force to overwrite): {}",
        existing
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

/// Resolve the Replicate token: `--replicate-token` flag (which also reads
/// `REPLICATE_API_TOKEN` from the environment, with `.env` loaded at startup),
/// then truffle.toml.
async fn resolve_token(flag: Option<String>) -> Result<String> {
    if let Some(token) = flag {
        return Ok(token);
    }
    if let Some(token) = TruffleConfig::read()
        .await
        .ok()
        .and_then(|config| config.replicate_token)
    {
        return Ok(token);
    }
    anyhow::bail!(
        "No Replicate token found. Pass --replicate-token, set REPLICATE_API_TOKEN (e.g. via .env), or set `replicate_token` in truffle.toml."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_outputs_reads_num_outputs() {
        let prompt = prompt::PromptFile {
            replicate: prompt::ResolvedReplicate {
                owner: "owner".into(),
                name: "name".into(),
                version: None,
                input: [(
                    "num_outputs".to_string(),
                    serde_json::Value::Number(3.into()),
                )]
                .into_iter()
                .collect(),
                timeout_secs: 600,
                poll_interval_secs: 2,
            },
            snap: prompt::SnapSpec {
                colors: 16,
                pixel_size: None,
                palette: None,
                palette_png: None,
            },
            output: None,
        };
        assert_eq!(expected_outputs(&prompt), 3);
    }

    #[test]
    fn ensure_writable_rejects_existing_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("slime.png");
        std::fs::write(&existing, "bytes").unwrap();
        assert!(ensure_writable(std::slice::from_ref(&existing), false, false).is_err());
        assert!(ensure_writable(&[existing], true, false).is_ok());
    }
}
