use clap::Parser;
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about = "Generate an image atlas from a .ttf font")]
pub struct FontArgs {
    /// Input .ttf font file
    #[arg(value_name = "INPUT_TTF")]
    pub input_ttf: PathBuf,

    /// Output PNG atlas path
    #[arg(value_name = "OUTPUT_PNG")]
    pub output_png: PathBuf,

    /// Cell size in pixels (cell x cell)
    #[arg(long, default_value = "16")]
    pub cell: u32,

    /// Padding in pixels inside each cell (applied on all sides)
    #[arg(long, default_value = "1")]
    pub padding: u32,

    /// Charset string; glyphs are packed in this order (left-to-right, top-to-bottom)
    #[arg(long, default_value = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~")]
    pub charset: String,

    /// Atlas size in pixels as WxH (e.g. 1024x1024)
    #[arg(long, default_value = "1024x1024", value_name = "WxH")]
    pub size: String,
}

pub fn run(args: FontArgs) -> bool {
    match run_impl(args) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("[font] ERROR: {e}");
            false
        }
    }
}

fn run_impl(args: FontArgs) -> anyhow::Result<()> {
    let (atlas_w, atlas_h) = parse_size(&args.size)?;

    if args.cell == 0 {
        anyhow::bail!("--cell must be > 0");
    }
    if args.cell <= args.padding.saturating_mul(2) {
        anyhow::bail!("--cell must be > 2*--padding");
    }
    if atlas_w == 0 || atlas_h == 0 {
        anyhow::bail!("--size must be > 0x0");
    }
    if atlas_w % args.cell != 0 || atlas_h % args.cell != 0 {
        anyhow::bail!(
            "--size must be divisible by --cell (got size {}x{}, cell {})",
            atlas_w,
            atlas_h,
            args.cell
        );
    }

    let cols = atlas_w / args.cell;
    let rows = atlas_h / args.cell;
    let capacity = (cols as usize) * (rows as usize);
    let charset_len = args.charset.chars().count();
    if charset_len == 0 {
        anyhow::bail!("--charset must not be empty");
    }
    if charset_len > capacity {
        anyhow::bail!(
            "charset has {charset_len} glyph(s) but atlas capacity is {capacity} cell(s) ({}x{} cells)",
            cols,
            rows
        );
    }

    let inner = args
        .cell
        .checked_sub(args.padding.saturating_mul(2))
        .ok_or_else(|| anyhow::anyhow!("--cell must be > 2*--padding"))?;

    let font_bytes = fs::read(&args.input_ttf).map_err(|e| {
        anyhow::anyhow!(
            "failed to read input font {}: {e}",
            args.input_ttf.display()
        )
    })?;

    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!("failed to parse font: {e:?}"))?;

    let mut atlas = image::RgbaImage::from_pixel(atlas_w, atlas_h, image::Rgba([0, 0, 0, 0]));

    // Choose a single pixel size that makes all glyph bitmaps fit within the inner box.
    let mut px = inner.max(1) as f32;
    px = fit_pixel_size(&font, args.charset.chars(), px, inner)?;

    for (i, ch) in args.charset.chars().enumerate() {
        let (metrics, bitmap) = font.rasterize(ch, px);

        // Some glyphs may rasterize to empty; keep cell empty.
        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }

        let col = (i as u32) % cols;
        let row = (i as u32) / cols;

        let cell_x0 = col * args.cell;
        let cell_y0 = row * args.cell;

        let gw = metrics.width as u32;
        let gh = metrics.height as u32;

        if gw > inner || gh > inner {
            // Should not happen due to fit_pixel_size, but keep it safe.
            continue;
        }

        let xoff = args.padding + (inner - gw) / 2;
        let yoff = args.padding + (inner - gh) / 2;

        blit_alpha_white(
            &mut atlas,
            cell_x0 + xoff,
            cell_y0 + yoff,
            gw,
            gh,
            &bitmap,
        );
    }

    atlas
        .save(&args.output_png)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", args.output_png.display()))?;

    println!(
        "[font] ✅ Wrote {} ({}x{}, cell {}, padding {}, glyphs {})",
        args.output_png.display(),
        atlas_w,
        atlas_h,
        args.cell,
        args.padding,
        charset_len
    );

    Ok(())
}

fn fit_pixel_size(
    font: &fontdue::Font,
    charset: impl Iterator<Item = char> + Clone,
    initial_px: f32,
    inner: u32,
) -> anyhow::Result<f32> {
    let mut px = initial_px.max(1.0);

    // Iterate a couple times to converge if needed.
    for _ in 0..4 {
        let mut max_w = 0u32;
        let mut max_h = 0u32;

        for ch in charset.clone() {
            let (m, _) = font.rasterize(ch, px);
            max_w = max_w.max(m.width as u32);
            max_h = max_h.max(m.height as u32);
        }

        let max_dim = max_w.max(max_h);
        if max_dim == 0 {
            // Entire charset rasterizes to nothing; keep something valid.
            return Ok(px.max(1.0));
        }

        if max_w <= inner && max_h <= inner {
            return Ok(px);
        }

        let scale = (inner as f32) / (max_dim as f32);
        let next_px = (px * scale).floor().max(1.0);
        if (next_px - px).abs() < f32::EPSILON {
            return Ok(px.max(1.0));
        }
        px = next_px;
    }

    Ok(px.max(1.0))
}

fn blit_alpha_white(
    dst: &mut image::RgbaImage,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    alpha: &[u8],
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
            dst.put_pixel(dx, dy, image::Rgba([255, 255, 255, out_a]));
        }
    }
}

fn parse_size(s: &str) -> anyhow::Result<(u32, u32)> {
    let (w_s, h_s) = s
        .split_once('x')
        .ok_or_else(|| anyhow::anyhow!("invalid --size (expected WxH): {s}"))?;
    let w: u32 = w_s
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid --size width: {w_s}"))?;
    let h: u32 = h_s
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid --size height: {h_s}"))?;
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_ok() {
        assert_eq!(parse_size("1024x1024").unwrap(), (1024, 1024));
        assert_eq!(parse_size("1x2").unwrap(), (1, 2));
    }

    #[test]
    fn parse_size_err() {
        assert!(parse_size("1024").is_err());
        assert!(parse_size("axb").is_err());
        assert!(parse_size("10x").is_err());
    }

    #[test]
    fn capacity_math() {
        let atlas_w = 64u32;
        let atlas_h = 32u32;
        let cell = 16u32;
        let cols = atlas_w / cell;
        let rows = atlas_h / cell;
        let capacity = (cols as usize) * (rows as usize);
        assert_eq!(cols, 4);
        assert_eq!(rows, 2);
        assert_eq!(capacity, 8);
    }
}


