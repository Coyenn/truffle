use image::{ImageBuffer, Rgba, RgbaImage};
use std::path::Path;

const DEFAULT_GRASS_COLORS: [[u8; 3]; 4] = [
    [0x31, 0x7d, 0x2d],
    [0x43, 0x9a, 0x3d],
    [0x56, 0xb2, 0x4b],
    [0x25, 0x5e, 0x27],
];

pub fn default_grass_colors() -> Vec<[u8; 3]> {
    DEFAULT_GRASS_COLORS.to_vec()
}

pub fn load_sample_colors(sample_path: &Path) -> Result<Vec<[u8; 3]>, String> {
    let sample = image::open(sample_path)
        .map_err(|e| format!("Failed to read sample {}: {}", sample_path.display(), e))?
        .to_rgba8();
    let colors = collect_visible_colors(&sample);

    if colors.is_empty() {
        return Err(format!(
            "Sample image contains no visible colors: {}",
            sample_path.display()
        ));
    }

    Ok(colors)
}

pub fn generate_grass_variant(
    input_path: &Path,
    output_path: &Path,
    colors: &[[u8; 3]],
) -> Result<(), String> {
    if colors.is_empty() {
        return Err("Terrain palette contains no colors".into());
    }

    let source = image::open(input_path)
        .map_err(|e| format!("Failed to read image {}: {}", input_path.display(), e))?
        .to_rgba8();
    let seed = input_path.to_string_lossy();
    let output = build_grass_variant(&source, colors, seed.as_ref());

    output
        .save(output_path)
        .map_err(|e| format!("Failed to write image {}: {}", output_path.display(), e))
}

fn collect_visible_colors(image: &RgbaImage) -> Vec<[u8; 3]> {
    image
        .pixels()
        .filter(|pixel| pixel[3] > 0)
        .map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect()
}

fn build_grass_variant(source: &RgbaImage, colors: &[[u8; 3]], seed: &str) -> RgbaImage {
    let (width, height) = source.dimensions();
    let mut output = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    let bottom_by_column = lowest_visible_by_column(source);

    for (x, bottom_y) in bottom_by_column.into_iter().enumerate() {
        let Some(bottom_y) = bottom_y else {
            continue;
        };

        let band_height = 1 + (hash_for(seed, x as u32, bottom_y, 0) % 5) as u32;
        for offset in 0..band_height {
            let Some(y) = bottom_y.checked_sub(offset) else {
                break;
            };
            let source_pixel = source.get_pixel(x as u32, y).0;
            if source_pixel[3] == 0 {
                continue;
            }

            let color = pick_color(colors, seed, x as u32, y, 1);
            output.put_pixel(
                x as u32,
                y,
                Rgba([color[0], color[1], color[2], source_pixel[3]]),
            );
        }
    }

    output
}

fn lowest_visible_by_column(image: &RgbaImage) -> Vec<Option<u32>> {
    let (width, height) = image.dimensions();
    let mut bottoms = vec![None; width as usize];

    for x in 0..width {
        for y in 0..height {
            if image.get_pixel(x, y)[3] > 0 {
                bottoms[x as usize] = Some(y);
            }
        }
    }

    bottoms
}

fn pick_color(colors: &[[u8; 3]], seed: &str, x: u32, y: u32, salt: u64) -> [u8; 3] {
    let index = hash_for(seed, x, y, salt) as usize % colors.len();
    colors[index]
}

fn hash_for(seed: &str, x: u32, y: u32, salt: u64) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;

    for byte in seed.as_bytes() {
        hash = fnv1a(hash, *byte);
    }
    for byte in b"grass" {
        hash = fnv1a(hash, *byte);
    }
    for byte in x.to_le_bytes() {
        hash = fnv1a(hash, byte);
    }
    for byte in y.to_le_bytes() {
        hash = fnv1a(hash, byte);
    }
    for byte in salt.to_le_bytes() {
        hash = fnv1a(hash, byte);
    }

    hash
}

fn fnv1a(hash: u64, byte: u8) -> u64 {
    (hash ^ byte as u64).wrapping_mul(0x100000001b3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_colors() -> Vec<[u8; 3]> {
        vec![[10, 110, 20], [20, 120, 30], [30, 130, 40]]
    }

    #[test]
    fn per_column_bottom_detection_handles_uneven_shapes() {
        let mut source = ImageBuffer::from_pixel(4, 5, Rgba([0, 0, 0, 0]));
        source.put_pixel(0, 1, Rgba([1, 1, 1, 255]));
        source.put_pixel(1, 3, Rgba([1, 1, 1, 255]));
        source.put_pixel(2, 2, Rgba([1, 1, 1, 255]));

        let bottoms = lowest_visible_by_column(&source);

        assert_eq!(bottoms, vec![Some(1), Some(3), Some(2), None]);
    }

    #[test]
    fn grass_only_recolors_existing_visible_bottom_pixels() {
        let mut source = ImageBuffer::from_pixel(3, 5, Rgba([0, 0, 0, 0]));
        source.put_pixel(1, 2, Rgba([200, 20, 20, 80]));
        source.put_pixel(1, 3, Rgba([200, 20, 20, 180]));
        source.put_pixel(1, 4, Rgba([200, 20, 20, 255]));

        let output = build_grass_variant(&source, &test_colors(), "asset.png");

        assert_eq!(output.dimensions(), source.dimensions());
        assert_eq!(output.get_pixel(0, 4).0, [0, 0, 0, 0]);
        assert_eq!(output.get_pixel(1, 4)[3], 255);
        assert_ne!(output.get_pixel(1, 4).0, source.get_pixel(1, 4).0);
        assert_eq!(output.get_pixel(2, 4).0, [0, 0, 0, 0]);
    }

    #[test]
    fn grass_removes_original_subject_pixels_outside_overlay() {
        let mut source = ImageBuffer::from_pixel(3, 5, Rgba([0, 0, 0, 0]));
        source.put_pixel(1, 0, Rgba([200, 20, 20, 255]));
        source.put_pixel(1, 4, Rgba([200, 20, 20, 255]));

        let output = build_grass_variant(&source, &test_colors(), "asset.png");

        assert_eq!(output.get_pixel(1, 0).0, [0, 0, 0, 0]);
    }

    #[test]
    fn deterministic_output_is_stable() {
        let source = ImageBuffer::from_pixel(3, 3, Rgba([200, 20, 20, 255]));

        let first = build_grass_variant(&source, &test_colors(), "asset.png");
        let second = build_grass_variant(&source, &test_colors(), "asset.png");

        assert_eq!(first.as_raw(), second.as_raw());
    }

    #[test]
    fn empty_sample_palette_errors_clearly() {
        let path = std::env::temp_dir().join(format!(
            "truffle-empty-terrain-sample-{}.png",
            std::process::id()
        ));
        let source = ImageBuffer::from_pixel(2, 2, Rgba([10u8, 20, 30, 0]));
        source.save(&path).unwrap();

        let err = load_sample_colors(&path).unwrap_err();
        fs::remove_file(&path).unwrap();

        assert!(err.contains("Sample image contains no visible colors"));
    }
}
