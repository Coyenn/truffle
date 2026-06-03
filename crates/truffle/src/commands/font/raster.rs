#[derive(Clone)]
pub struct InkProfile {
    pub ymin: i32,
    pub xmin: i32,
    pub rows: Vec<Option<(u32, u32)>>,
}

pub fn ink_profile_from_alpha(
    alpha: &[u8],
    w: u32,
    h: u32,
    ymin: i32,
    xmin: i32,
    threshold: u8,
) -> InkProfile {
    let mut rows = Vec::with_capacity(h as usize);
    if w == 0 || h == 0 {
        return InkProfile { ymin, xmin, rows };
    }
    for y in (0..h).rev() {
        let mut left: Option<u32> = None;
        let mut right: Option<u32> = None;
        let row_off = (y * w) as usize;
        for x in 0..w {
            let a = alpha[row_off + x as usize];
            if a > threshold {
                left = Some(left.map_or(x, |v| v.min(x)));
                right = Some(right.map_or(x, |v| v.max(x)));
            }
        }
        rows.push(left.zip(right));
    }
    InkProfile { ymin, xmin, rows }
}

pub fn binarize_alpha(alpha: &mut [u8]) {
    for a in alpha.iter_mut() {
        *a = if *a == 0 { 0 } else { 255 };
    }
}

pub fn blit_alpha_white(dst: &mut image::RgbaImage, x0: u32, y0: u32, w: u32, h: u32, alpha: &[u8]) {
    blit_alpha_color(dst, x0, y0, w, h, alpha, [255, 255, 255]);
}

pub fn blit_alpha_color(
    dst: &mut image::RgbaImage,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    alpha: &[u8],
    rgb: [u8; 3],
) {
    let dst_w = dst.width();
    let dst_h = dst.height();

    for y in 0..h {
        for x in 0..w {
            let a = alpha[(y * w + x) as usize];
            if a == 0 {
                continue;
            }
            let dx = x0 + x;
            let dy = y0 + y;
            if dx >= dst_w || dy >= dst_h {
                continue;
            }
            let existing = dst.get_pixel(dx, dy).0;
            let out_a = existing[3].max(a);
            dst.put_pixel(dx, dy, image::Rgba([rgb[0], rgb[1], rgb[2], out_a]));
        }
    }
}

pub fn dilate_alpha_with_border(alpha: &[u8], w: u32, h: u32, r: u32) -> (u32, u32, Vec<u8>) {
    if r == 0 || w == 0 || h == 0 {
        return (w, h, alpha.to_vec());
    }

    let out_w = w + 2 * r;
    let out_h = h + 2 * r;
    let mut expanded = vec![0u8; (out_w * out_h) as usize];

    for y in 0..h {
        let src_row = (y * w) as usize;
        let dst_row = ((y + r) * out_w + r) as usize;
        expanded[dst_row..dst_row + (w as usize)]
            .copy_from_slice(&alpha[src_row..src_row + (w as usize)]);
    }

    let mut dilated = vec![0u8; (out_w * out_h) as usize];
    let r_i = r as i32;
    let ow_i = out_w as i32;
    let oh_i = out_h as i32;

    for y in 0..out_h as i32 {
        for x in 0..out_w as i32 {
            let mut max_a = 0u8;
            let y0 = (y - r_i).max(0);
            let y1 = (y + r_i).min(oh_i - 1);
            let x0 = (x - r_i).max(0);
            let x1 = (x + r_i).min(ow_i - 1);
            for yy in y0..=y1 {
                let row_off = (yy * ow_i) as usize;
                for xx in x0..=x1 {
                    let a = expanded[row_off + (xx as usize)];
                    if a > max_a {
                        max_a = a;
                        if max_a == 255 {
                            break;
                        }
                    }
                }
                if max_a == 255 {
                    break;
                }
            }
            dilated[(y as u32 * out_w + x as u32) as usize] = max_a;
        }
    }

    (out_w, out_h, dilated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binarize_alpha_makes_hard_edges() {
        let mut alpha = vec![0, 1, 127, 128, 254, 255];
        binarize_alpha(&mut alpha);
        assert_eq!(alpha, vec![0, 255, 255, 255, 255, 255]);
    }
}
