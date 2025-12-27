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
    #[arg(
        long,
        default_value = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~"
    )]
    pub charset: String,

    /// Atlas size in pixels as WxH (e.g. 1024x1024)
    #[arg(long, default_value = "1024x1024", value_name = "WxH")]
    pub size: String,

    /// Output Luau metadata module path. Defaults to OUTPUT_PNG with .luau extension.
    #[arg(long, value_name = "OUTPUT_LUAU")]
    pub luau: Option<PathBuf>,

    /// Output TypeScript declaration file for the Luau module. Defaults to OUTPUT_PNG with .d.ts extension.
    #[arg(long, value_name = "OUTPUT_D_TS")]
    pub dts: Option<PathBuf>,
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

    let mut rasterized = Vec::with_capacity(charset_len);
    let mut min_ymin = i32::MAX;
    let mut max_ymax = i32::MIN;

    for ch in args.charset.chars() {
        let (metrics, bitmap) = font.rasterize(ch, px);
        if metrics.width > 0 && metrics.height > 0 {
            min_ymin = min_ymin.min(metrics.ymin);
            max_ymax = max_ymax.max(metrics.ymin + metrics.height as i32);
        }
        rasterized.push((ch, metrics, bitmap));
    }

    let baseline_in_inner = if min_ymin == i32::MAX { 0 } else { -min_ymin };
    let baseline = args.padding + baseline_in_inner.max(0) as u32;

    let mut glyph_metas = Vec::with_capacity(charset_len);
    for (i, (ch, metrics, bitmap)) in rasterized.into_iter().enumerate() {
        // Some glyphs may rasterize to empty; keep cell empty.
        let col = (i as u32) % cols;
        let row = (i as u32) / cols;

        let cell_x0 = col * args.cell;
        let cell_y0 = row * args.cell;

        let gw = metrics.width as u32;
        let gh = metrics.height as u32;

        let mut draw_x = cell_x0 + args.padding;
        let mut draw_y = cell_y0 + args.padding;

        if gw > 0 && gh > 0 && gw <= inner && gh <= inner {
            let xoff = args.padding + (inner - gw) / 2;
            draw_x = cell_x0 + xoff;
            draw_y = (cell_y0 as i32 + args.padding as i32 + baseline_in_inner + metrics.ymin)
                .max(0) as u32;

            blit_alpha_white(&mut atlas, draw_x, draw_y, gw, gh, &bitmap);
        }

        glyph_metas.push(GlyphMeta {
            ch,
            index: i as u32,
            col,
            row,
            cell_x: cell_x0,
            cell_y: cell_y0,
            cell_w: args.cell,
            cell_h: args.cell,
            draw_x,
            draw_y,
            draw_w: gw,
            draw_h: gh,
            // fontdue provides an advance width in px
            advance: metrics.advance_width,
        });
    }

    atlas
        .save(&args.output_png)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", args.output_png.display()))?;

    let luau_path = args.luau.clone().unwrap_or_else(|| {
        let mut p = args.output_png.clone();
        p.set_extension("luau");
        p
    });
    let dts_path = args.dts.clone().unwrap_or_else(|| {
        let mut p = args.output_png.clone();
        p.set_extension("d.ts");
        p
    });

    let kerning =
        compute_kerning_table(&fs::read(&args.input_ttf)?, &args.charset, px).unwrap_or_default();

    let meta = FontAtlasMeta {
        atlas_w,
        atlas_h,
        cell: args.cell,
        padding: args.padding,
        inner,
        px,
        baseline,
        charset: args.charset.clone(),
        glyphs: glyph_metas,
        kerning,
    };

    fs::write(&luau_path, render_font_luau_module(&meta)).map_err(|e| {
        anyhow::anyhow!("failed to write Luau metadata {}: {e}", luau_path.display())
    })?;
    fs::write(&dts_path, render_font_dts_module()).map_err(|e| {
        anyhow::anyhow!(
            "failed to write TypeScript declarations {}: {e}",
            dts_path.display()
        )
    })?;
    println!(
        "[font] Wrote metadata: {} and {}",
        luau_path.display(),
        dts_path.display()
    );

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

struct FontAtlasMeta {
    atlas_w: u32,
    atlas_h: u32,
    cell: u32,
    padding: u32,
    inner: u32,
    px: f32,
    baseline: u32,
    charset: String,
    glyphs: Vec<GlyphMeta>,
    /// Kerng adjustments in pixels for pairs within the charset.
    kerning: Vec<KerningPair>,
}

struct GlyphMeta {
    ch: char,
    index: u32,
    col: u32,
    row: u32,
    cell_x: u32,
    cell_y: u32,
    cell_w: u32,
    cell_h: u32,
    draw_x: u32,
    draw_y: u32,
    draw_w: u32,
    draw_h: u32,
    /// Advance width in pixels at `px` size.
    advance: f32,
}

struct KerningPair {
    left: char,
    right: char,
    /// Kerning adjustment in pixels at `px` size (add to advance).
    kern: f32,
}

fn render_font_luau_module(meta: &FontAtlasMeta) -> String {
    format!(
        "-- This file is automatically @generated by truffle.\n\
         -- DO NOT EDIT MANUALLY.\n\n\
         local font = {}\n\
         return {{\n\
         \tfont = font\n\
         }}\n",
        serialize_font_luau(meta, 0)
    )
}

fn render_font_dts_module() -> String {
    // This is intentionally simple: the Luau module returns `{ font = ... }`.
    // TS consumers can use the declared shape to read widths/kerning later.
    "// This file is automatically @generated by truffle.\n\
     // DO NOT EDIT MANUALLY.\n\n\
     export interface FontGlyph {\n\
     \tch: string;\n\
     \tindex: number;\n\
     \tcol: number;\n\
     \trow: number;\n\
     \tcellX: number;\n\
     \tcellY: number;\n\
     \tcellW: number;\n\
     \tcellH: number;\n\
     \tdrawX: number;\n\
     \tdrawY: number;\n\
     \tdrawW: number;\n\
     \tdrawH: number;\n\
     \tadvance: number;\n\
     }\n\n\
     export interface FontKerningPair {\n\
     \tleft: string;\n\
     \tright: string;\n\
     \tkern: number;\n\
     }\n\n\
     export interface FontAtlasMeta {\n\
     \tatlasW: number;\n\
     \tatlasH: number;\n\
     \tcell: number;\n\
     \tpadding: number;\n\
     \tinner: number;\n\
     \tpx: number;\n\
     \tbaseline: number;\n\
     \tcharset: string;\n\
     \tglyphs: Record<string, FontGlyph>;\n\
     \tkerning: FontKerningPair[];\n\
     }\n\n\
     declare const font: FontAtlasMeta;\n\
     export { font };\n"
        .to_string()
}

fn serialize_font_luau(meta: &FontAtlasMeta, indent: usize) -> String {
    let indent_str = "\t".repeat(indent);
    let inner_indent = format!("{}\t", indent_str);
    let first_level = indent == 0;

    let mut parts = vec!["{".to_string()];
    parts.push(format!("{}atlasW = {},", inner_indent, meta.atlas_w));
    parts.push(format!("{}atlasH = {},", inner_indent, meta.atlas_h));
    parts.push(format!("{}cell = {},", inner_indent, meta.cell));
    parts.push(format!("{}padding = {},", inner_indent, meta.padding));
    parts.push(format!("{}inner = {},", inner_indent, meta.inner));
    parts.push(format!("{}px = {},", inner_indent, float_luau(meta.px)));
    parts.push(format!("{}baseline = {},", inner_indent, meta.baseline));
    parts.push(format!(
        "{}charset = {},",
        inner_indent,
        serde_json::to_string(&meta.charset).unwrap()
    ));

    // Glyphs as a dictionary keyed by character for easy lookup later.
    parts.push(format!("{}glyphs = {{", inner_indent));
    for g in &meta.glyphs {
        let key = serde_json::to_string(&g.ch.to_string()).unwrap();
        parts.push(format!("{}\t[{}] = {{", inner_indent, key));
        parts.push(format!("{}\t\tch = {},", inner_indent, key));
        parts.push(format!("{}\t\tindex = {},", inner_indent, g.index));
        parts.push(format!("{}\t\tcol = {},", inner_indent, g.col));
        parts.push(format!("{}\t\trow = {},", inner_indent, g.row));
        parts.push(format!("{}\t\tcellX = {},", inner_indent, g.cell_x));
        parts.push(format!("{}\t\tcellY = {},", inner_indent, g.cell_y));
        parts.push(format!("{}\t\tcellW = {},", inner_indent, g.cell_w));
        parts.push(format!("{}\t\tcellH = {},", inner_indent, g.cell_h));
        parts.push(format!("{}\t\tdrawX = {},", inner_indent, g.draw_x));
        parts.push(format!("{}\t\tdrawY = {},", inner_indent, g.draw_y));
        parts.push(format!("{}\t\tdrawW = {},", inner_indent, g.draw_w));
        parts.push(format!("{}\t\tdrawH = {},", inner_indent, g.draw_h));
        parts.push(format!(
            "{}\t\tadvance = {},",
            inner_indent,
            float_luau(g.advance)
        ));
        parts.push(format!("{}\t}},", inner_indent));
    }
    parts.push(format!("{}}},", inner_indent));

    // Kerning pairs as a list.
    parts.push(format!("{}kerning = {{", inner_indent));
    for k in &meta.kerning {
        let left = serde_json::to_string(&k.left.to_string()).unwrap();
        let right = serde_json::to_string(&k.right.to_string()).unwrap();
        parts.push(format!(
            "{}\t{{ left = {}, right = {}, kern = {} }},",
            inner_indent,
            left,
            right,
            float_luau(k.kern)
        ));
    }
    parts.push(format!("{}}},", inner_indent));

    parts.push(format!("{}}}", indent_str));
    let result = parts.join("\n");
    if first_level {
        format!("{}\n", result)
    } else {
        result
    }
}

fn float_luau(v: f32) -> String {
    if v.is_finite() {
        // Keep it reasonably compact but stable-ish.
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        "0".to_string()
    }
}

fn compute_kerning_table(
    font_bytes: &[u8],
    charset: &str,
    px: f32,
) -> anyhow::Result<Vec<KerningPair>> {
    let face = ttf_parser::Face::parse(font_bytes, 0)
        .map_err(|_| anyhow::anyhow!("failed to parse font for kerning"))?;
    let upem = face.units_per_em() as f32;
    let scale = px / upem;

    let chars: Vec<char> = charset.chars().collect();
    let mut gids = Vec::with_capacity(chars.len());
    for &ch in &chars {
        gids.push(face.glyph_index(ch));
    }

    let kern_table = face.tables().kern;
    let mut out = Vec::new();
    for (i, &left) in chars.iter().enumerate() {
        let Some(lgid) = gids[i] else { continue };
        for (j, &right) in chars.iter().enumerate() {
            let Some(rgid) = gids[j] else { continue };
            let mut kern_units: i32 = 0;
            if let Some(kern_table) = kern_table {
                for sub in kern_table.subtables {
                    if !sub.horizontal || sub.has_cross_stream {
                        continue;
                    }
                    if let Some(v) = sub.glyphs_kerning(lgid, rgid) {
                        kern_units += v as i32;
                    }
                }
            }

            if kern_units != 0 {
                out.push(KerningPair {
                    left,
                    right,
                    kern: (kern_units as f32) * scale,
                });
            }
        }
    }

    Ok(out)
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
        let mut min_ymin = i32::MAX;
        let mut max_ymax = i32::MIN;

        for ch in charset.clone() {
            let (m, _) = font.rasterize(ch, px);
            max_w = max_w.max(m.width as u32);
            max_h = max_h.max(m.height as u32);

            if m.width > 0 && m.height > 0 {
                min_ymin = min_ymin.min(m.ymin);
                max_ymax = max_ymax.max(m.ymin + m.height as i32);
            }
        }

        let max_dim = max_w.max(max_h);
        if max_dim == 0 {
            // Entire charset rasterizes to nothing; keep something valid.
            return Ok(px.max(1.0));
        }

        let baseline_span_ok = if min_ymin == i32::MAX || max_ymax == i32::MIN {
            true
        } else {
            (max_ymax - min_ymin) as u32 <= inner
        };

        if max_w <= inner && max_h <= inner && baseline_span_ok {
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

fn blit_alpha_white(dst: &mut image::RgbaImage, x0: u32, y0: u32, w: u32, h: u32, alpha: &[u8]) {
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

    #[test]
    fn dts_contains_expected_exports() {
        let dts = render_font_dts_module();
        assert!(dts.contains("export interface FontAtlasMeta"));
        assert!(dts.contains("declare const font: FontAtlasMeta;"));
        assert!(dts.contains("export { font };"));
    }
}
