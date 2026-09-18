use crate::image::{palette as palette_image, snap};
use anyhow::{ensure, Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(about = "Snap messy or off-grid pixels back to a crisp pixel-art grid")]
pub struct SnapArgs {
    /// Input path (PNG/JPEG file or directory)
    #[arg(value_name = "INPUT_PATH")]
    pub input_path: PathBuf,

    /// Output PNG file (single input) or output directory (directory input).
    /// Defaults to `<stem>-snapped.png` siblings beside each source.
    #[arg(short, long, value_name = "OUTPUT")]
    pub output: Option<PathBuf>,

    /// Number of palette colors quantized before snapping
    #[arg(long, default_value = "16")]
    pub colors: usize,

    /// Override the auto-detected pixel size
    #[arg(long, value_name = "PIXELS")]
    pub pixel_size: Option<f64>,

    /// Constrain the output to comma-separated 6-digit hex colors
    #[arg(long, value_name = "HEX,...", conflicts_with = "palette_png")]
    pub palette: Option<String>,

    /// Constrain the output to the visible colors of a palette PNG
    #[arg(long, value_name = "PNG")]
    pub palette_png: Option<PathBuf>,

    /// Preview what would be snapped without writing files
    #[arg(long)]
    pub dry_run: bool,

    /// Overwrite existing snapped outputs
    #[arg(long)]
    pub force: bool,

    /// Recursively process directories
    #[arg(short, long)]
    pub recursive: bool,
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg"
            )
        })
}

fn is_snapped_output(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("-snapped.png"))
}

fn same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn sibling_output(input: &Path) -> Result<PathBuf> {
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .with_context(|| format!("Input has no file stem: {}", input.display()))?;
    Ok(input.with_file_name(format!("{stem}-snapped.png")))
}

fn mirrored_output(input: &Path, input_dir: &Path, output_dir: &Path) -> Result<PathBuf> {
    let relative = input.strip_prefix(input_dir).with_context(|| {
        format!(
            "Failed to relativize {} against {}",
            input.display(),
            input_dir.display()
        )
    })?;
    let mut output = output_dir.join(relative);
    output.set_extension("png");
    Ok(output)
}

fn resolve_palette(args: &SnapArgs) -> Result<Option<Vec<[u8; 3]>>> {
    if let Some(hex) = &args.palette {
        return snap::parse_palette_hex(hex).map(Some);
    }
    if let Some(palette_path) = &args.palette_png {
        let colors = palette_image::load_palette_colors(palette_path)
            .with_context(|| format!("Failed to load palette {}", palette_path.display()))?;
        ensure!(
            colors.len() <= snap::MAX_PALETTE_COLORS,
            "Palette PNG must contain at most {} distinct colors (found {})",
            snap::MAX_PALETTE_COLORS,
            colors.len()
        );
        return Ok(Some(colors));
    }
    Ok(None)
}

fn collect_inputs(input_path: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if input_path.is_file() {
        ensure!(
            is_supported(input_path),
            "Input must be a PNG or JPEG file: {}",
            input_path.display()
        );
        return Ok(vec![input_path.to_path_buf()]);
    }

    let mut files = Vec::new();
    if recursive {
        for entry in WalkDir::new(input_path).into_iter() {
            let entry = entry
                .with_context(|| format!("Failed to read entry under {}", input_path.display()))?;
            if entry.file_type().is_file()
                && is_supported(entry.path())
                && !is_snapped_output(entry.path())
            {
                files.push(entry.path().to_path_buf());
            }
        }
    } else {
        for entry in std::fs::read_dir(input_path)
            .with_context(|| format!("Failed to read directory {}", input_path.display()))?
        {
            let entry = entry
                .with_context(|| format!("Failed to read entry under {}", input_path.display()))?;
            let is_file = match entry.file_type() {
                Ok(file_type) => file_type.is_file(),
                // Skip entries whose type cannot be determined.
                Err(_) => false,
            };
            if is_file && is_supported(&entry.path()) && !is_snapped_output(&entry.path()) {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

fn plan_outputs(
    inputs: &[PathBuf],
    input_path: &Path,
    output: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    if inputs.len() == 1 && input_path.is_file() {
        let input = &inputs[0];
        if let Some(output) = output {
            if output.is_dir() {
                let stem = input
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|stem| !stem.is_empty())
                    .with_context(|| format!("Input has no file stem: {}", input.display()))?;
                return Ok(vec![output.join(format!("{stem}.png"))]);
            }
            ensure!(
                output
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png")),
                "Output must have a .png extension: {}",
                output.display()
            );
            return Ok(vec![output.to_path_buf()]);
        }
        return Ok(vec![sibling_output(input)?]);
    }

    if let Some(output_dir) = output {
        ensure!(
            !output_dir.is_file(),
            "Output must be a directory for directory input: {}",
            output_dir.display()
        );
        return inputs
            .iter()
            .map(|input| mirrored_output(input, input_path, output_dir))
            .collect();
    }

    inputs.iter().map(|input| sibling_output(input)).collect()
}

fn snap(args: SnapArgs) -> Result<(usize, usize, usize)> {
    ensure!(args.colors > 0, "Number of colors must be greater than 0");
    if let Some(pixel_size) = args.pixel_size {
        ensure!(
            pixel_size.is_finite() && pixel_size > 0.0,
            "Pixel size must be a positive number"
        );
    }
    ensure!(
        args.input_path.exists(),
        "Input path does not exist: {}",
        args.input_path.display()
    );
    if let Some(output) = &args.output {
        ensure!(
            !(args.input_path.is_dir() && output.is_file()),
            "Output must be a directory for directory input: {}",
            output.display()
        );
    }

    let palette = resolve_palette(&args)?;
    let inputs = collect_inputs(&args.input_path, args.recursive)?;
    if inputs.is_empty() {
        println!(
            "[snap] No supported images found in: {}",
            args.input_path.display()
        );
        return Ok((0, 0, 0));
    }
    if args.input_path.is_dir() {
        println!("[snap] Found {} image(s) to process", inputs.len());
    }
    let outputs = plan_outputs(&inputs, &args.input_path, args.output.as_deref())?;

    let mut processed = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;

    for (input, output) in inputs.iter().zip(outputs.iter()) {
        if let Some(palette_png) = &args.palette_png {
            if same_file(input, palette_png) {
                println!(
                    "[snap] SKIP: {} (palette image is excluded from processing)",
                    input.display()
                );
                skipped += 1;
                continue;
            }
        }
        if output.exists() && !args.force {
            println!(
                "[snap] SKIP: {} ({} already exists)",
                input.display(),
                output.display()
            );
            skipped += 1;
            continue;
        }
        if args.dry_run {
            println!(
                "[snap] DRY-RUN: Would snap {} -> {}",
                input.display(),
                output.display()
            );
            processed += 1;
            continue;
        }

        println!("[snap] Processing: {}", input.display());
        match snap::snap_file(
            input,
            output,
            args.colors,
            args.pixel_size,
            palette.as_deref(),
        ) {
            Ok(()) => {
                println!("[snap] Generated: {}", output.display());
                processed += 1;
            }
            Err(error) => {
                eprintln!(
                    "[snap] ERROR: Failed to snap {}: {error:#}",
                    input.display()
                );
                errors += 1;
            }
        }
    }

    if args.dry_run {
        println!("[snap] DRY-RUN: Would snap {processed} file(s), Skipped: {skipped}");
    } else {
        println!("[snap] Done. Processed: {processed}, Skipped: {skipped}, Errors: {errors}");
    }
    Ok((processed, skipped, errors))
}

pub fn run(args: SnapArgs) -> bool {
    let dry_run = args.dry_run;
    match snap(args) {
        Ok((processed, _, _)) => processed > 0 || dry_run,
        Err(error) => {
            eprintln!("[snap] ERROR: {error:#}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn sibling_output_uses_snapped_suffix() {
        assert_eq!(
            sibling_output(Path::new("assets/sprite.png")).unwrap(),
            PathBuf::from("assets/sprite-snapped.png")
        );
        assert_eq!(
            sibling_output(Path::new("assets/photo.jpg")).unwrap(),
            PathBuf::from("assets/photo-snapped.png")
        );
    }

    #[test]
    fn mirrored_output_preserves_relative_path_as_png() {
        assert_eq!(
            mirrored_output(
                Path::new("assets/npc/hero.jpg"),
                Path::new("assets"),
                Path::new("out")
            )
            .unwrap(),
            PathBuf::from("out/npc/hero.png")
        );
    }

    #[test]
    fn snapped_outputs_are_excluded_from_resnapping() {
        assert!(is_snapped_output(Path::new("hero-snapped.png")));
        assert!(!is_snapped_output(Path::new("hero.png")));
    }

    #[test]
    fn jpeg_inputs_are_supported() {
        assert!(is_supported(Path::new("hero.jpg")));
        assert!(is_supported(Path::new("hero.jpeg")));
        assert!(is_supported(Path::new("hero.png")));
        assert!(!is_supported(Path::new("hero.gif")));
    }

    #[test]
    fn palette_and_palette_png_conflict() {
        let result = SnapArgs::try_parse_from([
            "snap",
            "input.png",
            "--palette",
            "0d2b45,ffecd6",
            "--palette-png",
            "palette.png",
        ]);
        assert!(result.is_err());
    }
}
