use crate::image::terrain;
use clap::Parser;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(about = "Generate grass integration PNG overlays")]
pub struct TerrainArgs {
    /// Input path (file or directory)
    #[arg(value_name = "INPUT_PATH")]
    pub input_path: PathBuf,

    /// Preview what would be generated without creating files
    #[arg(long)]
    pub dry_run: bool,

    /// Overwrite existing grass overlays
    #[arg(long)]
    pub force: bool,

    /// Recursively process directories
    #[arg(short, long)]
    pub recursive: bool,

    /// PNG sample whose visible pixels are used as grass colors
    #[arg(long, value_name = "PNG")]
    pub grass_sample: Option<PathBuf>,
}

struct ProcessOptions<'a> {
    colors: &'a [[u8; 3]],
    dry_run: bool,
    force: bool,
    recursive: bool,
}

fn is_png(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("png")
}

fn is_generated_terrain(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.ends_with("-grass.png"))
        .unwrap_or(false)
}

fn get_terrain_path(image_path: &Path) -> PathBuf {
    let mut path = image_path.to_path_buf();
    if let Some(stem) = image_path.file_stem().and_then(|s| s.to_str()) {
        path.set_file_name(format!("{}-grass.png", stem));
    } else {
        path.set_file_name(format!("{}-grass.png", image_path.display()));
    }
    path
}

fn load_colors(sample_path: Option<&Path>, defaults: Vec<[u8; 3]>) -> Result<Vec<[u8; 3]>, String> {
    match sample_path {
        Some(path) => terrain::load_sample_colors(path),
        None => Ok(defaults),
    }
}

fn collect_png_files(path: &Path, recursive: bool) -> Result<Vec<PathBuf>, String> {
    if recursive {
        Ok(WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .filter(|p| is_png(p) && !is_generated_terrain(p))
            .collect())
    } else {
        Ok(std::fs::read_dir(path)
            .map_err(|e| format!("Failed to read directory {}: {}", path.display(), e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| is_png(p) && !is_generated_terrain(p))
            .collect())
    }
}

fn process_image(
    image_path: &Path,
    colors: &[[u8; 3]],
    dry_run: bool,
    force: bool,
) -> Result<bool, String> {
    let output_path = get_terrain_path(image_path);

    if output_path.exists() && !force {
        println!(
            "[terrain] SKIP: {} ({} already exists)",
            image_path.display(),
            output_path.display()
        );
        return Ok(false);
    }

    if dry_run {
        println!(
            "[terrain] DRY-RUN: Would generate {}",
            output_path.display()
        );
        return Ok(true);
    }

    println!("[terrain] Processing: {}", image_path.display());
    terrain::generate_grass_variant(image_path, &output_path, colors).map_err(|e| {
        format!(
            "Failed to generate grass overlay for {}: {}",
            image_path.display(),
            e
        )
    })?;

    println!("[terrain] Generated: {}", output_path.display());
    Ok(true)
}

fn process_path(
    input_path: &Path,
    options: &ProcessOptions<'_>,
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

    let png_files = if input_path.is_file() {
        if !is_png(input_path) {
            return Err(format!(
                "Input must be a PNG file: {}",
                input_path.display()
            ));
        }
        vec![input_path.to_path_buf()]
    } else {
        collect_png_files(input_path, options.recursive)?
    };

    if png_files.is_empty() {
        println!("[terrain] No PNG files found in: {}", input_path.display());
        return Ok((0, 0, 0));
    }

    if input_path.is_dir() {
        println!("[terrain] Found {} PNG file(s) to process", png_files.len());
    }

    for file in png_files {
        match process_image(&file, options.colors, options.dry_run, options.force) {
            Ok(true) => processed += 1,
            Ok(false) => skipped += 1,
            Err(err) => {
                eprintln!("[terrain] ERROR: {}", err);
                errors += 1;
            }
        }
    }

    if options.dry_run {
        println!(
            "[terrain] DRY-RUN: Would generate {} file(s), Skipped: {}",
            processed, skipped
        );
    } else {
        println!(
            "[terrain] Done. Processed: {}, Skipped: {}, Errors: {}",
            processed, skipped, errors
        );
    }

    Ok((processed, skipped, errors))
}

pub fn run(args: TerrainArgs) -> bool {
    let colors = match load_colors(
        args.grass_sample.as_deref(),
        terrain::default_grass_colors(),
    ) {
        Ok(colors) => colors,
        Err(err) => {
            eprintln!("[terrain] ERROR: {}", err);
            return false;
        }
    };

    let options = ProcessOptions {
        colors: &colors,
        dry_run: args.dry_run,
        force: args.force,
        recursive: args.recursive,
    };

    match process_path(&args.input_path, &options) {
        Ok((processed, _, _)) => processed > 0 || args.dry_run,
        Err(err) => {
            eprintln!("[terrain] ERROR: {}", err);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_path_uses_grass_suffix() {
        assert_eq!(
            get_terrain_path(Path::new("assets/house.png")),
            PathBuf::from("assets/house-grass.png")
        );
    }

    #[test]
    fn generated_terrain_outputs_are_detected() {
        assert!(is_generated_terrain(Path::new("house-grass.png")));
        assert!(!is_generated_terrain(Path::new("house.png")));
    }
}
