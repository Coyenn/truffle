use clap::Parser;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(about = "Generate highlight variants of PNG images with white outlines")]
pub struct HighlightArgs {
    /// Input path (file or directory)
    #[arg(value_name = "INPUT_PATH")]
    pub input_path: PathBuf,

    /// Preview what would be generated without creating files
    #[arg(long)]
    pub dry_run: bool,

    /// Overwrite existing highlight variants
    #[arg(long)]
    pub force: bool,

    /// Outline thickness in pixels
    #[arg(long, default_value = "1")]
    pub thickness: u32,
}

fn check_magick() -> Result<(), String> {
    let output = Command::new("magick")
        .arg("-version")
        .output()
        .map_err(|_| "magick (ImageMagick) is not available. Please install ImageMagick.")?;

    if !output.status.success() {
        return Err(
            "magick (ImageMagick) is not available. Please install ImageMagick.".to_string(),
        );
    }

    Ok(())
}

fn get_highlight_path(image_path: &Path) -> PathBuf {
    if let Some(stem) = image_path.file_stem().and_then(|s| s.to_str()) {
        let mut path = image_path.to_path_buf();
        path.set_file_name(format!("{}-highlight.png", stem));
        path
    } else {
        let mut path = image_path.to_path_buf();
        path.set_file_name(format!("{}-highlight.png", image_path.display()));
        path
    }
}

fn process_image(
    image_path: &Path,
    dry_run: bool,
    force: bool,
    thickness: u32,
) -> Result<bool, String> {
    let highlight_path = get_highlight_path(image_path);

    if highlight_path.exists() && !force {
        println!(
            "[highlight] SKIP: {} (highlight already exists)",
            image_path.display()
        );
        return Ok(false);
    }

    if dry_run {
        println!(
            "[highlight] DRY-RUN: Would generate {}",
            highlight_path.display()
        );
        return Ok(true);
    }

    println!("[highlight] Processing: {}", image_path.display());

    let thickness_str = thickness.to_string();
    let diamond_str = format!("Diamond:{}", thickness);
    let shave_str = thickness.to_string();

    let magick_args = vec![
        image_path.to_str().unwrap(),
        "-write",
        "mpr:original",
        "+delete",
        "(",
        "mpr:original",
        "-alpha",
        "extract",
        "-bordercolor",
        "black",
        "-border",
        &thickness_str,
        "-morphology",
        "EdgeIn",
        &diamond_str,
        "-shave",
        &shave_str,
        "-write",
        "mpr:outline-mask",
        "+delete",
        ")",
        "(",
        "mpr:original",
        "-alpha",
        "off",
        "-fill",
        "white",
        "-colorize",
        "100",
        "-channel",
        "A",
        "mpr:outline-mask",
        "-compose",
        "CopyOpacity",
        "-composite",
        "+channel",
        "-write",
        "mpr:white-outline",
        "+delete",
        ")",
        "mpr:original",
        "mpr:white-outline",
        "-compose",
        "Over",
        "-composite",
        "-filter",
        "Point",
        highlight_path.to_str().unwrap(),
    ];

    let output = Command::new("magick")
        .args(&magick_args)
        .output()
        .map_err(|e| format!("Failed to run magick: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Failed to generate highlight for {}: {}",
            image_path.display(),
            stderr
        ));
    }

    println!("[highlight] ✅ Generated: {}", highlight_path.display());
    Ok(true)
}

fn process_path(
    path: &Path,
    dry_run: bool,
    force: bool,
    thickness: u32,
) -> Result<(usize, usize, usize), String> {
    let mut processed = 0;
    let mut skipped = 0;
    let mut errors = 0;

    if !path.exists() {
        return Err(format!("Path does not exist: {}", path.display()));
    }

    if path.is_file() {
        if path.extension().and_then(|s| s.to_str()) != Some("png") {
            return Err(format!("Input must be a PNG file: {}", path.display()));
        }

        match process_image(path, dry_run, force, thickness) {
            Ok(true) => processed += 1,
            Ok(false) => skipped += 1,
            Err(_) => errors += 1,
        }
    } else {
        let png_files: Vec<PathBuf> = WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .filter(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("png")
                    && !p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.contains("-highlight.png"))
                        .unwrap_or(false)
            })
            .collect();

        if png_files.is_empty() {
            println!("[highlight] No PNG files found in: {}", path.display());
            return Ok((0, 0, 0));
        }

        println!(
            "[highlight] Found {} PNG file(s) to process",
            png_files.len()
        );

        for file in png_files {
            match process_image(&file, dry_run, force, thickness) {
                Ok(true) => processed += 1,
                Ok(false) => {
                    let highlight_path = get_highlight_path(&file);
                    if highlight_path.exists() {
                        skipped += 1;
                    } else {
                        errors += 1;
                    }
                }
                Err(_) => errors += 1,
            }
        }
    }

    if dry_run {
        println!("[highlight] DRY-RUN: Would process {} file(s)", processed);
    } else {
        println!(
            "[highlight] Done ✅ Processed: {}, Skipped: {}, Errors: {}",
            processed, skipped, errors
        );
    }

    Ok((processed, skipped, errors))
}

pub fn run(args: HighlightArgs) -> bool {
    if args.thickness < 1 {
        eprintln!("[highlight] ERROR: Thickness must be >= 1");
        return false;
    }

    if let Err(e) = check_magick() {
        eprintln!("[highlight] ERROR: {}", e);
        return false;
    }

    match process_path(&args.input_path, args.dry_run, args.force, args.thickness) {
        Ok((processed, _, _)) => processed > 0 || args.dry_run,
        Err(e) => {
            eprintln!("[highlight] ERROR: {}", e);
            false
        }
    }
}
