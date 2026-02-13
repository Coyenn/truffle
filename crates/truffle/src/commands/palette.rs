use crate::image::palette;
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
) -> Result<(), String> {
    if dry_run {
        println!("[palette] DRY-RUN: Would process {}", image_path.display());
        return Ok(());
    }

    println!("[palette] Processing: {}", image_path.display());
    palette::apply_palette_to_path(image_path, palette_colors)?;
    println!("[palette] ✅ Updated: {}", image_path.display());
    Ok(())
}

fn collect_png_files(path: &Path, recursive: bool) -> Result<Vec<PathBuf>, String> {
    if recursive {
        Ok(WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .filter(|p| is_png(p))
            .collect())
    } else {
        Ok(std::fs::read_dir(path)
            .map_err(|e| format!("Failed to read directory {}: {}", path.display(), e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| is_png(p))
            .collect())
    }
}

fn process_path(
    input_path: &Path,
    palette_path: &Path,
    dry_run: bool,
    recursive: bool,
) -> Result<(usize, usize, usize), String> {
    let mut processed = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;

    if !input_path.exists() {
        return Err(format!(
            "Input path does not exist: {}",
            input_path.display()
        ));
    }

    if !palette_path.exists() {
        return Err(format!(
            "Palette path does not exist: {}",
            palette_path.display()
        ));
    }

    if !palette_path.is_file() {
        return Err(format!(
            "Palette path must be a file: {}",
            palette_path.display()
        ));
    }

    if !is_png(palette_path) {
        return Err(format!(
            "Palette must be a PNG file: {}",
            palette_path.display()
        ));
    }

    let palette_colors = palette::load_palette_colors(palette_path)?;

    if input_path.is_file() {
        if !is_png(input_path) {
            return Err(format!(
                "Input must be a PNG file: {}",
                input_path.display()
            ));
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
                Err(err) => {
                    eprintln!("[palette] ERROR: {}", err);
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
                Err(err) => {
                    eprintln!("[palette] ERROR: {}", err);
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
        Err(err) => {
            eprintln!("[palette] ERROR: {}", err);
            false
        }
    }
}
