//! Snap off-grid pixels back to a crisp pixel-art grid.
//!
//! Thin wrapper over the open-source Sprite Fusion Pixel Snapper
//! (MIT, <https://github.com/Hugo-Dz/spritefusion-pixel-snapper>).
//! Upstream only exposes batch-directory processing, so single files are
//! staged through temporary directories before the snapped PNG is moved to
//! its final destination.

use anyhow::{ensure, Context, Result};
use spritefusion_pixel_snapper::{process_batch_with_reporter, BatchConfig};
use std::collections::HashSet;
use std::path::Path;

pub const MAX_PALETTE_COLORS: usize = 256;

/// Resolve an optional hex palette plus an optional palette PNG into colors.
pub fn resolve_palette_option(
    hex: Option<&str>,
    palette_png: Option<&Path>,
) -> Result<Option<Vec<[u8; 3]>>> {
    if let Some(hex) = hex {
        return parse_palette_hex(hex).map(Some);
    }
    if let Some(palette_path) = palette_png {
        let colors = super::palette::load_palette_colors(palette_path)
            .with_context(|| format!("Failed to load palette {}", palette_path.display()))?;
        ensure!(
            colors.len() <= MAX_PALETTE_COLORS,
            "Palette PNG must contain at most {MAX_PALETTE_COLORS} distinct colors (found {})",
            colors.len()
        );
        return Ok(Some(colors));
    }
    Ok(None)
}

/// Parse comma-separated 6-digit hex colors (`"0d2b45,ffecd6"`).
/// A leading `#` per entry is allowed; duplicates are removed.
pub fn parse_palette_hex(value: &str) -> Result<Vec<[u8; 3]>> {
    ensure!(
        !value.trim().is_empty(),
        "Palette must contain at least one color"
    );

    let mut seen = HashSet::new();
    let mut palette = Vec::new();
    for part in value.split(',') {
        let hex = part.trim().trim_start_matches('#');
        ensure!(
            hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()),
            "Invalid palette color '{}', expected a 6-digit hex code",
            part.trim()
        );
        let color = [
            u8::from_str_radix(&hex[0..2], 16).unwrap(),
            u8::from_str_radix(&hex[2..4], 16).unwrap(),
            u8::from_str_radix(&hex[4..6], 16).unwrap(),
        ];
        if seen.insert(color) {
            palette.push(color);
        }
    }

    ensure!(
        palette.len() <= MAX_PALETTE_COLORS,
        "Palette must contain at most {MAX_PALETTE_COLORS} distinct colors"
    );
    Ok(palette)
}

/// Snap one image (`PNG`/`JPEG`) to its implicit pixel grid, writing a PNG.
pub fn snap_file(
    input_path: &Path,
    output_path: &Path,
    colors: usize,
    pixel_size: Option<f64>,
    palette: Option<&[[u8; 3]]>,
) -> Result<()> {
    ensure!(colors > 0, "Number of colors must be greater than 0");
    if let Some(pixel_size) = pixel_size {
        ensure!(
            pixel_size.is_finite() && pixel_size > 0.0,
            "Pixel size must be a positive number"
        );
    }
    if let Some(palette) = palette {
        ensure!(
            !palette.is_empty(),
            "Palette must contain at least one color"
        );
        ensure!(
            palette.len() <= MAX_PALETTE_COLORS,
            "Palette must contain at most {MAX_PALETTE_COLORS} distinct colors"
        );
    }
    ensure!(
        output_path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png")),
        "Output must have a .png extension: {}",
        output_path.display()
    );

    let input_bytes = std::fs::read(input_path)
        .with_context(|| format!("Failed to read input {}", input_path.display()))?;

    let extension = input_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("png");
    ensure!(
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg"
        ),
        "Input must be a PNG or JPEG file: {}",
        input_path.display()
    );

    let input_dir = tempfile::tempdir().context("Failed to create temporary input directory")?;
    let staged_input = input_dir.path().join(format!("input.{extension}"));
    std::fs::write(&staged_input, &input_bytes)
        .with_context(|| format!("Failed to stage {}", input_path.display()))?;
    let output_dir = tempfile::tempdir().context("Failed to create temporary output directory")?;

    let config = BatchConfig {
        input_dir: input_dir.path().to_path_buf(),
        output_dir: output_dir.path().to_path_buf(),
        k_colors: colors,
        pixel_size_override: pixel_size,
        palette: palette.map(|colors| colors.to_vec()),
    };
    process_batch_with_reporter(&config, |_| {})
        .map_err(|error| anyhow::anyhow!("Pixel Snapper failed: {error}"))?;

    let staged_output = output_dir.path().join("input.png");
    let snapped = std::fs::read(&staged_output).with_context(|| {
        format!(
            "Pixel Snapper produced no output for {}",
            input_path.display()
        )
    })?;

    if let Some(parent) = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }
    std::fs::write(output_path, &snapped)
        .with_context(|| format!("Failed to write output {}", output_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba, RgbaImage};

    #[test]
    fn palette_hex_parsing_accepts_hash_prefix_and_dedupes() {
        let colors = parse_palette_hex("#0d2b45, ffecd6,0D2B45").unwrap();
        assert_eq!(colors, vec![[0x0d, 0x2b, 0x45], [0xff, 0xec, 0xd6]]);
    }

    #[test]
    fn palette_hex_parsing_rejects_bad_entries() {
        assert!(parse_palette_hex("").is_err());
        assert!(parse_palette_hex("red,ffecd6").is_err());
        assert!(parse_palette_hex("12345,ffecd6").is_err());
    }

    #[test]
    fn snap_rejects_zero_colors_and_non_png_output() {
        let input = Path::new("input.png");
        let output = Path::new("output.png");
        assert!(snap_file(input, output, 0, None, None).is_err());
        assert!(snap_file(input, Path::new("output.jpg"), 16, None, None).is_err());
        assert!(snap_file(input, output, 16, Some(0.0), None).is_err());
        assert!(snap_file(input, output, 16, None, Some(&[])).is_err());
    }

    #[test]
    fn snap_roundtrip_produces_a_png() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("sprite.png");
        let output = dir.path().join("sprite-snapped.png");

        let mut source: RgbaImage = ImageBuffer::from_pixel(16, 16, Rgba([13, 43, 69, 255]));
        for y in 0..16 {
            for x in 0..16 {
                if (x + y) % 2 == 0 {
                    source.put_pixel(x, y, Rgba([255, 236, 214, 255]));
                }
            }
        }
        source.save(&input).unwrap();

        snap_file(&input, &output, 2, Some(1.0), None).unwrap();

        let bytes = std::fs::read(&output).unwrap();
        assert_eq!(&bytes[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let snapped = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(snapped.dimensions(), (16, 16));
    }
}
