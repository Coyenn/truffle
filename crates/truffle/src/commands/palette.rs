use crate::image::palette;
use anyhow::Context;
use clap::Parser;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(about = "Apply a palette PNG to one image or all images in a directory")]
pub struct PaletteArgs {
    /// Input path (file or directory)
    #[arg(value_name = "INPUT_PATH")]
    pub input_path: PathBuf,

    /// Palette PNG where each visible pixel represents one palette color
    #[arg(value_name = "PALETTE_PATH")]
    pub palette_path: PathBuf,

    /// Preview what would be changed without writing files
    #[arg(long)]
    pub dry_run: bool,

    /// Recursively process directories
    #[arg(short, long)]
    pub recursive: bool,
}

fn is_png(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("png")
}

fn same_file(path: &Path, other: &Path) -> bool {
    if path == other {
        return true;
    }

    match (std::fs::canonicalize(path), std::fs::canonicalize(other)) {
        (Ok(lhs), Ok(rhs)) => lhs == rhs,
        _ => false,
    }
}

fn process_image(
    image_path: &Path,
    palette_colors: &[[u8; 3]],
    dry_run: bool,
) -> anyhow::Result<()> {
    if dry_run {
        println!("[palette] DRY-RUN: Would process {}", image_path.display());
        return Ok(());
    }

    println!("[palette] Processing: {}", image_path.display());
    palette::apply_palette_to_path(image_path, palette_colors)
        .with_context(|| format!("Failed to apply palette to {}", image_path.display()))?;
    println!("[palette] ✅ Updated: {}", image_path.display());
    Ok(())
}

fn collect_png_files(path: &Path, recursive: bool) -> anyhow::Result<Vec<PathBuf>> {
    let mut png_files = Vec::new();
    if recursive {
        for entry in WalkDir::new(path).into_iter() {
            let entry =
                entry.with_context(|| format!("Failed to read entry under {}", path.display()))?;
            if entry.file_type().is_file() && is_png(entry.path()) {
                png_files.push(entry.path().to_path_buf());
            }
        }
    } else {
        for entry in std::fs::read_dir(path)
            .with_context(|| format!("Failed to read directory {}", path.display()))?
        {
            let entry =
                entry.with_context(|| format!("Failed to read entry under {}", path.display()))?;
            let is_file = match entry.file_type() {
                Ok(file_type) => file_type.is_file(),
                // Skip entries whose type cannot be determined.
                Err(_) => false,
            };
            if is_file && is_png(&entry.path()) {
                png_files.push(entry.path());
            }
        }
    }
    png_files.sort();
    Ok(png_files)
}

fn process_path(
    input_path: &Path,
    palette_path: &Path,
    dry_run: bool,
    recursive: bool,
) -> anyhow::Result<(usize, usize, usize)> {
    let mut processed = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;

    if !input_path.exists() {
        anyhow::bail!("Input path does not exist: {}", input_path.display());
    }

    if !palette_path.exists() {
        anyhow::bail!("Palette path does not exist: {}", palette_path.display());
    }

    if !palette_path.is_file() {
        anyhow::bail!("Palette path must be a file: {}", palette_path.display());
    }

    if !is_png(palette_path) {
        anyhow::bail!("Palette must be a PNG file: {}", palette_path.display());
    }

    let palette_colors = palette::load_palette_colors(palette_path)
        .with_context(|| format!("Failed to load palette {}", palette_path.display()))?;

    if input_path.is_file() {
        if !is_png(input_path) {
            anyhow::bail!("Input must be a PNG file: {}", input_path.display());
        }

        if same_file(input_path, palette_path) {
            println!(
                "[palette] SKIP: {} (palette image is excluded from processing)",
                input_path.display()
            );
            skipped += 1;
        } else {
            match process_image(input_path, &palette_colors, dry_run) {
                Ok(()) => processed += 1,
                Err(error) => {
                    eprintln!("[palette] ERROR: {error:#}");
                    errors += 1;
                }
            }
        }
    } else {
        let png_files = collect_png_files(input_path, recursive)?;

        if png_files.is_empty() {
            println!("[palette] No PNG files found in: {}", input_path.display());
            return Ok((0, 0, 0));
        }

        println!("[palette] Found {} PNG file(s) to process", png_files.len());

        for file in png_files {
            if same_file(&file, palette_path) {
                println!(
                    "[palette] SKIP: {} (palette image is excluded from processing)",
                    file.display()
                );
                skipped += 1;
                continue;
            }

            match process_image(&file, &palette_colors, dry_run) {
                Ok(()) => processed += 1,
                Err(error) => {
                    eprintln!("[palette] ERROR: {error:#}");
                    errors += 1;
                }
            }
        }
    }

    if dry_run {
        println!(
            "[palette] DRY-RUN: Would process {} file(s), Skipped: {}",
            processed, skipped
        );
    } else {
        println!(
            "[palette] Done ✅ Processed: {}, Skipped: {}, Errors: {}",
            processed, skipped, errors
        );
    }

    Ok((processed, skipped, errors))
}

pub fn run(args: PaletteArgs) -> bool {
    match process_path(
        &args.input_path,
        &args.palette_path,
        args.dry_run,
        args.recursive,
    ) {
        Ok((processed, _, _)) => processed > 0 || args.dry_run,
        Err(error) => {
            eprintln!("[palette] ERROR: {error:#}");
            false
        }
    }
}
