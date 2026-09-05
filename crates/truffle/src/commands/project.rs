use crate::image::project::ProjectionMap;
use anyhow::{ensure, Context, Result};
use clap::Parser;
use image::ImageFormat;
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

#[derive(Parser)]
#[command(about = "Project a PNG through a self-contained JSON coordinate and shading map")]
pub struct ProjectArgs {
    /// Projection artwork, with all surfaces packed into one PNG
    #[arg(value_name = "SOURCE_PNG")]
    pub source: PathBuf,

    /// Versioned JSON map containing output dimensions, coordinates and shading
    #[arg(long, value_name = "MAP_JSON")]
    pub map: PathBuf,

    /// Output PNG; defaults to <source-stem>-projected.png beside the source
    #[arg(short, long, value_name = "OUTPUT_PNG")]
    pub output: Option<PathBuf>,

    /// Replace a different existing output; identical outputs are always left untouched
    #[arg(long)]
    pub force: bool,

    /// Validate both inputs and render without writing files
    #[arg(long)]
    pub dry_run: bool,
}

fn default_output(source: &Path) -> PathBuf {
    let mut name = source.file_stem().unwrap_or_default().to_os_string();
    name.push("-projected.png");
    source.with_file_name(name)
}

fn project(args: ProjectArgs) -> Result<()> {
    let output_path = args.output.unwrap_or_else(|| default_output(&args.source));
    ensure!(
        output_path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png")),
        "Output must have a .png extension"
    );
    let map = ProjectionMap::load(&args.map)?;
    let source = image::ImageReader::open(&args.source)
        .with_context(|| format!("Failed to read source {}", args.source.display()))?
        .with_guessed_format()?;
    ensure!(
        source.format() == Some(ImageFormat::Png),
        "Source must be a PNG image"
    );
    let source = source
        .decode()
        .context("Failed to decode source PNG")?
        .to_rgba8();
    let output = map
        .project(&source)
        .with_context(|| format!("Failed to project using {}", args.map.display()))?;
    if output_path.exists() {
        for input in [&args.source, &args.map] {
            ensure!(
                !same_file::is_same_file(input, &output_path)?,
                "Cannot overwrite projection input {}",
                input.display()
            );
        }
    }
    let mut encoded = Cursor::new(Vec::new());
    output
        .write_to(&mut encoded, ImageFormat::Png)
        .context("Failed to encode projection PNG")?;
    let encoded = encoded.into_inner();
    if output_path.exists() {
        let existing = fs::read(&output_path)
            .with_context(|| format!("Failed to read output {}", output_path.display()))?;
        if existing == encoded {
            println!("[project] Unchanged: {}", output_path.display());
            return Ok(());
        }
        ensure!(
            args.force,
            "Output already exists: {}; use --force to replace it",
            output_path.display()
        );
    }
    if args.dry_run {
        println!(
            "[project] DRY-RUN: Would write {} ({}x{})",
            output_path.display(),
            output.width(),
            output.height()
        );
        return Ok(());
    }
    let parent = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    let mut temporary =
        NamedTempFile::new_in(parent).context("Failed to create temporary output")?;
    std::io::Write::write_all(&mut temporary, &encoded)
        .context("Failed to write projection PNG")?;
    if args.force {
        temporary.persist(&output_path)
    } else {
        temporary.persist_noclobber(&output_path)
    }
    .with_context(|| format!("Failed to save projection {}", output_path.display()))?;
    println!(
        "[project] Generated: {} ({}x{})",
        output_path.display(),
        output.width(),
        output.height()
    );
    Ok(())
}

pub fn run(args: ProjectArgs) -> bool {
    match project(args) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("[project] ERROR: {error:#}");
            false
        }
    }
}
