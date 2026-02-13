use image::{Rgba, RgbaImage};
use std::collections::HashSet;
use std::path::Path;

pub fn load_palette_colors(palette_path: &Path) -> Result<Vec<[u8; 3]>, String> {
    let palette_image = image::open(palette_path)
        .map_err(|e| format!("Failed to read palette {}: {}", palette_path.display(), e))?
        .to_rgba8();
    let colors = collect_palette_colors(&palette_image);

    if colors.is_empty() {
        return Err(format!(
            "Palette image contains no usable colors: {}",
            palette_path.display()
        ));
    }

    Ok(colors)
}

pub fn apply_palette_to_path(image_path: &Path, palette_colors: &[[u8; 3]]) -> Result<(), String> {
    if palette_colors.is_empty() {
        return Err("Palette contains no colors".into());
    }

    let source = image::open(image_path)
        .map_err(|e| format!("Failed to read image {}: {}", image_path.display(), e))?
        .to_rgba8();
    let output = apply_palette(&source, palette_colors);
    output
        .save(image_path)
        .map_err(|e| format!("Failed to write image {}: {}", image_path.display(), e))
}

fn collect_palette_colors(palette_image: &RgbaImage) -> Vec<[u8; 3]> {
    let mut seen = HashSet::new();
    let mut colors = Vec::new();

    for pixel in palette_image.pixels() {
        if pixel[3] == 0 {
            continue;
        }

        let color = [pixel[0], pixel[1], pixel[2]];
        if seen.insert(color) {
            colors.push(color);
        }
    }

    colors
}

fn apply_palette(image: &RgbaImage, palette_colors: &[[u8; 3]]) -> RgbaImage {
    let mut output = image.clone();

    for pixel in output.pixels_mut() {
        if pixel[3] == 0 {
            continue;
        }

        let nearest = nearest_color([pixel[0], pixel[1], pixel[2]], palette_colors);
        *pixel = Rgba([nearest[0], nearest[1], nearest[2], pixel[3]]);
    }

    output
}

fn nearest_color(target: [u8; 3], palette_colors: &[[u8; 3]]) -> [u8; 3] {
    palette_colors
        .iter()
        .copied()
        .min_by_key(|candidate| color_distance_squared(target, *candidate))
        .unwrap_or(target)
}

fn color_distance_squared(lhs: [u8; 3], rhs: [u8; 3]) -> u32 {
    let dr = lhs[0] as i32 - rhs[0] as i32;
    let dg = lhs[1] as i32 - rhs[1] as i32;
    let db = lhs[2] as i32 - rhs[2] as i32;
    (dr * dr + dg * dg + db * db) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};
    use std::path::Path;

    #[test]
    fn nearest_color_remap_uses_expected_entry() {
        let source = ImageBuffer::from_pixel(1, 1, Rgba([250, 10, 10, 255]));
        let output = apply_palette(&source, &[[255, 0, 0], [0, 0, 255]]);

        assert_eq!(output.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn transparent_pixel_is_unchanged() {
        let source = ImageBuffer::from_pixel(1, 1, Rgba([123, 45, 67, 0]));
        let output = apply_palette(&source, &[[255, 0, 0]]);

        assert_eq!(output.get_pixel(0, 0).0, [123, 45, 67, 0]);
    }

    #[test]
    fn non_zero_alpha_is_preserved() {
        let source = ImageBuffer::from_pixel(1, 1, Rgba([40, 210, 40, 77]));
        let output = apply_palette(&source, &[[0, 255, 0]]);

        assert_eq!(output.get_pixel(0, 0).0, [0, 255, 0, 77]);
    }

    #[test]
    fn duplicate_palette_colors_are_deduplicated() {
        let palette = ImageBuffer::from_fn(3, 1, |x, _| match x {
            0 => Rgba([255, 0, 0, 255]),
            1 => Rgba([255, 0, 0, 255]),
            _ => Rgba([0, 255, 0, 255]),
        });

        let colors = collect_palette_colors(&palette);
        assert_eq!(colors, vec![[255, 0, 0], [0, 255, 0]]);
    }

    #[test]
    fn fully_transparent_palette_is_empty() {
        let palette = ImageBuffer::from_pixel(2, 2, Rgba([10, 20, 30, 0]));
        let colors = collect_palette_colors(&palette);

        assert!(colors.is_empty());
    }

    #[test]
    fn empty_palette_validation_errors() {
        let err = apply_palette_to_path(Path::new("ignored.png"), &[]).unwrap_err();
        assert!(err.contains("Palette contains no colors"));
    }
}
