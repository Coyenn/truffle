use image::{ImageBuffer, Rgba, RgbaImage};
use std::path::Path;

/// Generate a highlight variant for the provided PNG image.
/// The algorithm mirrors the previous ImageMagick pipeline:
/// 1. Extract the alpha mask.
/// 2. Apply a diamond-shaped erosion to shrink the mask inward.
/// 3. Subtract the eroded mask from the original to obtain the inner outline band.
/// 4. Fill that outline with opaque white pixels and composite it over the original image.
pub fn generate_highlight(
    input_path: &Path,
    output_path: &Path,
    thickness: u32,
) -> Result<(), String> {
    if thickness == 0 {
        return Err("Outline thickness must be >= 1".into());
    }

    let image = image::open(input_path)
        .map_err(|e| format!("Failed to read {}: {}", input_path.display(), e))?;
    let base = image.to_rgba8();
    let highlight = build_highlight(&base, thickness as usize);
    highlight
        .save(output_path)
        .map_err(|e| format!("Failed to write {}: {}", output_path.display(), e))
}

fn build_highlight(original: &RgbaImage, radius: usize) -> RgbaImage {
    let width = original.width() as usize;
    let height = original.height() as usize;

    let alpha = extract_alpha(original);
    let eroded = erode_diamond(&alpha, width, height, radius);
    let outline_mask = subtract_mask(&alpha, &eroded);

    let outline = build_outline_image(width, height, &outline_mask);
    composite_over(&outline, original)
}

fn extract_alpha(image: &RgbaImage) -> Vec<u8> {
    image.pixels().map(|p| p[3]).collect()
}

fn erode_diamond(mask: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    if radius == 0 {
        return mask.to_vec();
    }

    let mut eroded = vec![0u8; mask.len()];
    let radius_i = radius as isize;
    let width_i = width as isize;
    let height_i = height as isize;

    for y in 0..height_i {
        for x in 0..width_i {
            let mut min_val = u8::MAX;
            'outer: for dy in -radius_i..=radius_i {
                let ny = y + dy;
                let dx_limit = radius_i - dy.abs();
                for dx in -dx_limit..=dx_limit {
                    let nx = x + dx;
                    if ny < 0 || ny >= height_i || nx < 0 || nx >= width_i {
                        min_val = 0;
                        break 'outer;
                    }
                    let idx = (ny as usize) * width + (nx as usize);
                    let val = mask[idx];
                    if val < min_val {
                        min_val = val;
                        if min_val == 0 {
                            break 'outer;
                        }
                    }
                }
            }
            eroded[(y as usize) * width + (x as usize)] = min_val;
        }
    }

    eroded
}

fn subtract_mask(a: &[u8], b: &[u8]) -> Vec<u8> {
    a.iter()
        .zip(b.iter())
        .map(|(&lhs, &rhs)| lhs.saturating_sub(rhs))
        .collect()
}

fn build_outline_image(width: usize, height: usize, mask: &[u8]) -> RgbaImage {
    let mut buffer = ImageBuffer::from_pixel(width as u32, height as u32, Rgba([0, 0, 0, 0]));
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let alpha = mask[idx];
            if alpha == 0 {
                continue;
            }
            buffer.put_pixel(x as u32, y as u32, Rgba([255, 255, 255, alpha]));
        }
    }
    buffer
}

fn composite_over(top: &RgbaImage, bottom: &RgbaImage) -> RgbaImage {
    let (width, height) = top.dimensions();
    let mut output = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0]));

    for y in 0..height {
        for x in 0..width {
            let top_px = top.get_pixel(x, y).0;
            let bottom_px = bottom.get_pixel(x, y).0;
            let composed = composite_pixel(top_px, bottom_px);
            output.put_pixel(x, y, Rgba(composed));
        }
    }

    output
}

fn composite_pixel(top: [u8; 4], bottom: [u8; 4]) -> [u8; 4] {
    let ta = top[3] as f32 / 255.0;
    let ba = bottom[3] as f32 / 255.0;

    if ta == 0.0 && ba == 0.0 {
        return [0, 0, 0, 0];
    }

    let out_a = ta + ba * (1.0 - ta);
    let mut out = [0u8; 4];

    if out_a == 0.0 {
        return out;
    }

    for i in 0..3 {
        let tc = top[i] as f32 / 255.0;
        let bc = bottom[i] as f32 / 255.0;
        let premult = tc * ta + bc * ba * (1.0 - ta);
        let value = (premult / out_a).clamp(0.0, 1.0);
        out[i] = (value * 255.0).round() as u8;
    }

    out[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn sample_image() -> RgbaImage {
        let mut img = ImageBuffer::from_pixel(5, 5, Rgba([0, 0, 0, 0]));
        for y in 1..=3 {
            for x in 1..=3 {
                img.put_pixel(x, y, Rgba([200, 20, 20, 255]));
            }
        }
        img
    }

    #[test]
    fn white_outline_stays_inside_original_shape() {
        let base = sample_image();
        let result = build_highlight(&base, 1);

        for y in 0..5 {
            for x in 0..5 {
                let px = result.get_pixel(x, y).0;
                if x == 0 || x == 4 || y == 0 || y == 4 {
                    assert_eq!(px, [0, 0, 0, 0], "outline leaked outside at ({x},{y})");
                }
            }
        }
    }

    #[test]
    fn thin_outline_preserves_core_pixels() {
        let base = sample_image();
        let result = build_highlight(&base, 1);

        assert_eq!(result.get_pixel(2, 2).0, [200, 20, 20, 255]);

        for &(x, y) in &[
            (1, 1),
            (2, 1),
            (3, 1),
            (1, 2),
            (3, 2),
            (1, 3),
            (2, 3),
            (3, 3),
        ] {
            assert_eq!(
                result.get_pixel(x, y).0,
                [255, 255, 255, 255],
                "expected white outline at ({x},{y})"
            );
        }
    }

    #[test]
    fn thicker_outline_can_consume_entire_shape() {
        let base = sample_image();
        let result = build_highlight(&base, 2);

        for y in 1..=3 {
            for x in 1..=3 {
                assert_eq!(
                    result.get_pixel(x, y).0,
                    [255, 255, 255, 255],
                    "expected white fill at ({x},{y}) for thick outline"
                );
            }
        }
    }
}
