use clap::Parser;
use std::env;
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

/// Find the magick executable path.
/// Checks in order:
/// 1. TRUFFLE_MAGICK environment variable
/// 2. Bundled binary relative to current executable
/// 3. System "magick" command
fn find_magick() -> Result<PathBuf, String> {
    // Check environment variable first
    if let Ok(custom_path) = env::var("TRUFFLE_MAGICK") {
        let path = PathBuf::from(custom_path);
        if path.exists() {
            return Ok(path);
        }
    }

    // Try to find bundled binary
    if let Ok(exe_path) = env::current_exe() {
        let exe_dir = exe_path
            .parent()
            .ok_or("Could not determine executable directory")?;

        // Detect platform
        let platform_dir = if cfg!(target_os = "linux") {
            if cfg!(target_arch = "x86_64") {
                "linux-x86_64"
            } else if cfg!(target_arch = "aarch64") {
                "linux-aarch64"
            } else {
                return Err(format!(
                    "Unsupported Linux architecture: {}",
                    env::consts::ARCH
                ));
            }
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "x86_64") {
                "macos-x86_64"
            } else if cfg!(target_arch = "aarch64") {
                "macos-arm64"
            } else {
                return Err(format!(
                    "Unsupported macOS architecture: {}",
                    env::consts::ARCH
                ));
            }
        } else if cfg!(target_os = "windows") {
            if cfg!(target_arch = "x86_64") {
                "windows-x86_64"
            } else if cfg!(target_arch = "aarch64") {
                "windows-arm64"
            } else {
                return Err(format!(
                    "Unsupported Windows architecture: {}",
                    env::consts::ARCH
                ));
            }
        } else {
            return Err(format!("Unsupported OS: {}", env::consts::OS));
        };

        // Try relative to executable: ../vendor/imagemagick/<platform>/magick[.exe]
        let bundled_path = exe_dir
            .parent()
            .map(|p| {
                let mut path = p.to_path_buf();
                path.push("vendor");
                path.push("imagemagick");
                path.push(platform_dir);
                if cfg!(target_os = "windows") {
                    path.push("magick.exe");
                } else {
                    path.push("magick");
                }
                path
            })
            .filter(|p| p.exists());

        if let Some(path) = bundled_path {
            return Ok(path);
        }

        // Also try same directory as executable (for packaged releases)
        let bundled_path_same_dir = {
            let mut path = exe_dir.to_path_buf();
            path.push("vendor");
            path.push("imagemagick");
            path.push(platform_dir);
            if cfg!(target_os = "windows") {
                path.push("magick.exe");
            } else {
                path.push("magick");
            }
            path
        };

        if bundled_path_same_dir.exists() {
            return Ok(bundled_path_same_dir);
        }
    }

    // Fall back to system "magick" command
    Ok(PathBuf::from("magick"))
}

fn check_magick() -> Result<(), String> {
    let magick_path = find_magick()?;

    let output = Command::new(&magick_path)
        .arg("-version")
        .output()
        .map_err(|e| {
            format!(
                "magick (ImageMagick) is not available at {}: {}. Please install ImageMagick or ensure bundled binary is present.",
                magick_path.display(),
                e
            )
        })?;

    if !output.status.success() {
        return Err(format!(
            "magick (ImageMagick) at {} failed to execute. Please install ImageMagick or ensure bundled binary is present.",
            magick_path.display()
        ));
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

    let magick_path =
        find_magick().map_err(|e| format!("Failed to locate magick executable: {}", e))?;

    let output = Command::new(&magick_path)
        .args(&magick_args)
        .output()
        .map_err(|e| format!("Failed to run magick at {}: {}", magick_path.display(), e))?;

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
