mod kerning;
mod meta_v2;
mod pack;
mod raster;
mod runtime;

use clap::Parser;
use fontdue::Metrics;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use self::kerning::build_kerning_classes;
use self::meta_v2::{AtlasPageMeta, float_luau};
use self::meta_v2::{FontMetaV2, GlyphLayerMeta, render_font_meta_dts, serialize_font_meta_luau};
use self::pack::{PackRect, pack_glyphs, validate_atlas_size, write_atlas_pages};
use self::raster::{
    InkProfile, binarize_alpha, blit_alpha_color, blit_alpha_white, dilate_alpha_with_border,
    ink_profile_from_alpha,
};

#[derive(Parser, Debug)]
#[command(about = "Generate an image atlas from a .ttf font")]
pub struct FontArgs {
    /// Input .ttf font file
    #[arg(value_name = "INPUT_TTF")]
    pub input_ttf: PathBuf,

    /// Output PNG atlas path (page 0; additional pages use _N suffix)
    #[arg(value_name = "OUTPUT_PNG")]
    pub output_png: PathBuf,

    /// Padding in pixels around each glyph in the atlas
    #[arg(long, default_value = "1")]
    pub padding: u32,

    /// Charset string; glyphs are packed in this order
    #[arg(
        long,
        default_value = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~"
    )]
    pub charset: String,

    /// Path to a UTF-8 text file containing the charset (overrides --charset when set)
    #[arg(long, value_name = "PATH")]
    pub charset_file: Option<PathBuf>,

    /// Rasterization pixel size (fontdue px)
    #[arg(long)]
    pub px: Option<f32>,

    /// Design line height in pixels (layout reference size)
    #[arg(long, default_value = "95")]
    pub line_height: u32,

    /// Maximum atlas page size (square, power of two)
    #[arg(long, default_value = "1024")]
    pub max_atlas_size: u32,

    /// Target minimum ink gap for kerning class generation
    #[arg(long, default_value = "6")]
    pub kerning_gap: u32,

    /// Output Luau metadata module path. Defaults to OUTPUT_PNG with .luau extension.
    #[arg(long, value_name = "OUTPUT_LUAU")]
    pub luau: Option<PathBuf>,

    /// Output TypeScript declaration file for the Luau module. Defaults to OUTPUT_PNG with .d.ts extension.
    #[arg(long, value_name = "OUTPUT_D_TS")]
    pub dts: Option<PathBuf>,

    /// Copy the truffle-text runtime into this directory
    #[arg(long, value_name = "DIR")]
    pub runtime_out: Option<PathBuf>,

    /// Generate an outline (thicker fill) variant by dilating glyph alpha by this many pixels.
    /// 0 disables outline generation.
    #[arg(long, default_value = "0", value_name = "PX")]
    pub outline: u32,

    /// Output PNG atlas path for the outline variant. Defaults to OUTPUT_PNG with `_outline` suffix.
    #[arg(long, value_name = "OUTPUT_OUTLINE_PNG")]
    pub outline_png: Option<PathBuf>,

    /// Disable anti-aliasing by converting rasterized glyph alpha to hard 0/255.
    #[arg(long, default_value_t = false)]
    pub no_antialias: bool,
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

struct RasterizedGlyph {
    ch: char,
    metrics: Metrics,
    bitmap: Vec<u8>,
    w: u32,
    h: u32,
}

struct PlacedGlyph {
    ch: char,
    pack: PackRect,
    offset_x: f32,
    offset_y: f32,
    advance: f32,
}

fn run_impl(args: FontArgs) -> anyhow::Result<()> {
    let atlas_size = validate_atlas_size(args.max_atlas_size)?;
    if args.line_height == 0 {
        anyhow::bail!("--line-height must be > 0");
    }
    if args.outline > 0 && args.padding < args.outline {
        anyhow::bail!(
            "--padding must be >= --outline when outline is enabled (got padding {}, outline {})",
            args.padding,
            args.outline
        );
    }

    let charset = load_charset(&args)?;
    let charset_len = charset.chars().count();
    if charset_len == 0 {
        anyhow::bail!("charset must not be empty");
    }

    let px = args.px.unwrap_or_else(|| {
        let pad = args.padding.saturating_mul(2) as f32;
        (args.line_height as f32 - pad).max(1.0)
    });
    if px <= 0.0 {
        anyhow::bail!("--px must be > 0");
    }

    let font_bytes = fs::read(&args.input_ttf).map_err(|e| {
        anyhow::anyhow!(
            "failed to read input font {}: {e}",
            args.input_ttf.display()
        )
    })?;

    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow::anyhow!("failed to parse font: {e:?}"))?;

    let outline_enabled = args.outline > 0;
    let mut rasterized: Vec<RasterizedGlyph> = Vec::with_capacity(charset_len);
    let mut min_ymin = i32::MAX;

    for ch in charset.chars() {
        let (metrics, mut bitmap) = font.rasterize(ch, px);
        let w = metrics.width as u32;
        let h = metrics.height as u32;
        if args.no_antialias {
            binarize_alpha(&mut bitmap);
        }
        if w > 0 && h > 0 {
            min_ymin = min_ymin.min(metrics.ymin);
        }
        rasterized.push(RasterizedGlyph {
            ch,
            metrics,
            bitmap,
            w,
            h,
        });
    }

    let baseline_offset = if min_ymin == i32::MAX {
        0f32
    } else {
        (-min_ymin) as f32
    };
    let baseline = baseline_offset.round().max(0.0) as u32;
    let inner = args.line_height.saturating_sub(2 * args.padding) as f32;
    // Legacy marzipan cell fonts used ~21px from line top at lineHeight 95 / px 121 / padding 5.
    let layout_baseline = layout_baseline_for(args.line_height, args.padding, px);

    let sizes: Vec<(u32, u32)> = rasterized
        .iter()
        .map(|g| {
            if outline_enabled && g.w > 0 && g.h > 0 {
                (g.w + 2 * args.outline, g.h + 2 * args.outline)
            } else {
                (g.w, g.h)
            }
        })
        .collect();

    let packed = pack_glyphs(&sizes, args.padding, atlas_size)?;
    let page_count = packed.iter().map(|p| p.page).max().unwrap_or(0) + 1;

    let mut atlases: Vec<image::RgbaImage> = (0..page_count)
        .map(|_| image::RgbaImage::from_pixel(atlas_size, atlas_size, image::Rgba([0, 0, 0, 0])))
        .collect();
    let mut outline_atlases: Vec<image::RgbaImage> = if outline_enabled {
        (0..page_count)
            .map(|_| {
                image::RgbaImage::from_pixel(atlas_size, atlas_size, image::Rgba([0, 0, 0, 0]))
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut ink_profiles: HashMap<char, InkProfile> = HashMap::new();
    let mut placed: Vec<PlacedGlyph> = Vec::with_capacity(rasterized.len());

    let outline_radius = if outline_enabled { args.outline } else { 0 };

    for (g, pack) in rasterized.into_iter().zip(packed.iter().cloned()) {
        let offset_x = 0f32;
        // Legacy marzipan top bearing is layout_baseline + fontdue ymin (positive for marks above baseline).
        let offset_y = glyph_offset_y(
            args.line_height as f32,
            g.h as f32,
            layout_baseline,
            g.metrics.ymin as f32,
        );

        if g.w > 0 && g.h > 0 {
            // The packed rect reserves room for the outline; ink sits centered inside it.
            let ink_x = pack.x + outline_radius;
            let ink_y = pack.y + outline_radius;
            let atlas = atlases
                .get_mut(pack.page as usize)
                .ok_or_else(|| anyhow::anyhow!("invalid atlas page {}", pack.page))?;
            blit_alpha_white(atlas, ink_x, ink_y, g.w, g.h, &g.bitmap);
            ink_profiles.insert(
                g.ch,
                ink_profile_from_alpha(&g.bitmap, g.w, g.h, g.metrics.ymin, g.metrics.xmin, 0),
            );

            if outline_enabled {
                let r = args.outline;
                let (dw, dh, dilated) = dilate_alpha_with_border(&g.bitmap, g.w, g.h, r);
                let outline_atlas = outline_atlases
                    .get_mut(pack.page as usize)
                    .ok_or_else(|| anyhow::anyhow!("invalid outline atlas page {}", pack.page))?;
                blit_alpha_color(outline_atlas, pack.x, pack.y, dw, dh, &dilated, [0, 0, 0]);
                blit_alpha_white(outline_atlas, ink_x, ink_y, g.w, g.h, &g.bitmap);
            }
        }

        placed.push(PlacedGlyph {
            ch: g.ch,
            pack,
            offset_x,
            offset_y,
            advance: g.metrics.advance_width,
        });
    }

    write_atlas_pages(&atlases, &args.output_png)?;

    let outline_png_path = if outline_enabled {
        Some(
            args.outline_png
                .clone()
                .unwrap_or_else(|| derive_outline_png_path(&args.output_png)),
        )
    } else {
        None
    };
    if outline_enabled {
        if let Some(outline_png_path) = &outline_png_path {
            write_atlas_pages(&outline_atlases, outline_png_path)?;
        }
    }

    let advances: Vec<f32> = placed.iter().map(|g| g.advance).collect();
    let kerning = build_kerning_classes(
        &placed.iter().map(|g| g.ch).collect::<Vec<_>>(),
        &ink_profiles,
        &advances,
        args.kerning_gap,
    );

    let glyphs = build_glyph_layer(&placed, outline_radius);
    let outline_layer = if outline_enabled {
        Some(build_outline_layer(&placed, args.outline))
    } else {
        None
    };

    let pages: Vec<AtlasPageMeta> = (0..page_count)
        .map(|_| AtlasPageMeta {
            w: atlas_size,
            h: atlas_size,
        })
        .collect();

    let meta = FontMetaV2 {
        line_height: args.line_height,
        baseline,
        px,
        charset: charset.clone(),
        pages,
        glyphs,
        kerning,
        outline: outline_layer,
    };

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

    fs::write(&luau_path, serialize_font_meta_luau(&meta)).map_err(|e| {
        anyhow::anyhow!("failed to write Luau metadata {}: {e}", luau_path.display())
    })?;
    fs::write(&dts_path, render_font_meta_dts(outline_enabled)).map_err(|e| {
        anyhow::anyhow!(
            "failed to write TypeScript declarations {}: {e}",
            dts_path.display()
        )
    })?;

    if let Some(runtime_out) = &args.runtime_out {
        runtime::copy_runtime(runtime_out)?;
    }

    println!(
        "[font] Wrote metadata: {} and {}",
        luau_path.display(),
        dts_path.display()
    );
    println!(
        "[font] ✅ Wrote {} ({} page(s), lineHeight {}, padding {}, px {}, glyphs {})",
        args.output_png.display(),
        page_count,
        args.line_height,
        args.padding,
        float_luau(px),
        charset_len
    );
    if let Some(outline_png_path) = outline_png_path {
        println!(
            "[font] ✅ Wrote outline {} (dilate {}px)",
            outline_png_path.display(),
            args.outline
        );
    }

    Ok(())
}

fn build_glyph_layer(placed: &[PlacedGlyph], outline_radius: u32) -> GlyphLayerMeta {
    let mut advances = Vec::with_capacity(placed.len());
    let mut offset_x = Vec::with_capacity(placed.len());
    let mut offset_y = Vec::with_capacity(placed.len());
    let mut rects = Vec::with_capacity(placed.len() * 5);

    for g in placed {
        // The packed rect reserves room for the outline; the ink rect is centered inside it.
        let r = if g.pack.w > 0 { outline_radius } else { 0 };
        advances.push(g.advance);
        offset_x.push(g.offset_x);
        offset_y.push(g.offset_y);
        rects.push(g.pack.page);
        rects.push(g.pack.x + r);
        rects.push(g.pack.y + r);
        rects.push(g.pack.w.saturating_sub(2 * r));
        rects.push(g.pack.h.saturating_sub(2 * r));
    }

    GlyphLayerMeta {
        advances,
        offset_x,
        offset_y,
        rects,
    }
}

fn build_outline_layer(placed: &[PlacedGlyph], outline: u32) -> GlyphLayerMeta {
    let mut advances = Vec::with_capacity(placed.len());
    let mut offset_x = Vec::with_capacity(placed.len());
    let mut offset_y = Vec::with_capacity(placed.len());
    let mut rects = Vec::with_capacity(placed.len() * 5);

    for g in placed {
        advances.push(g.advance);
        offset_x.push(g.offset_x - outline as f32);
        offset_y.push(g.offset_y - outline as f32);
        rects.push(g.pack.page);
        rects.push(g.pack.x);
        rects.push(g.pack.y);
        rects.push(g.pack.w);
        rects.push(g.pack.h);
    }

    GlyphLayerMeta {
        advances,
        offset_x,
        offset_y,
        rects,
    }
}

fn load_charset(args: &FontArgs) -> anyhow::Result<String> {
    if let Some(path) = &args.charset_file {
        let s = fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read charset file {}: {e}", path.display()))?;
        return Ok(s);
    }
    Ok(args.charset.clone())
}

fn layout_baseline_for(line_height: u32, padding: u32, px: f32) -> f32 {
    let inner = line_height.saturating_sub(2 * padding) as f32;
    padding as f32 + (px - inner).max(0.0) / 2.0 - 2.0
}

fn glyph_offset_y(line_height: f32, glyph_height: f32, layout_baseline: f32, ymin: f32) -> f32 {
    line_height - glyph_height - 2.0 * layout_baseline - ymin
}

fn derive_outline_png_path(base_png: &Path) -> PathBuf {
    let parent = base_png.parent().unwrap_or_else(|| Path::new("."));
    let stem = base_png
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("font_atlas");
    let ext = base_png
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("png");
    parent.join(format!("{stem}_outline.{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_layer_rects_are_ink_tight_and_outline_uses_pack_rect() {
        let outline = 5_u32;
        let placed = vec![
            PlacedGlyph {
                ch: 'A',
                pack: PackRect {
                    page: 0,
                    x: 10,
                    y: 20,
                    w: 30 + 2 * outline,
                    h: 40 + 2 * outline,
                },
                offset_x: 0.0,
                offset_y: 12.0,
                advance: 31.0,
            },
            PlacedGlyph {
                ch: ' ',
                pack: PackRect {
                    page: 0,
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                },
                offset_x: 0.0,
                offset_y: 0.0,
                advance: 15.0,
            },
        ];

        let base = build_glyph_layer(&placed, outline);
        assert_eq!(&base.rects[0..5], &[0, 15, 25, 30, 40]);
        assert_eq!(&base.rects[5..10], &[0, 0, 0, 0, 0]);

        let outline_layer = build_outline_layer(&placed, outline);
        assert_eq!(&outline_layer.rects[0..5], &[0, 10, 20, 40, 50]);
        assert_eq!(outline_layer.offset_y[0], 7.0);
    }

    #[test]
    fn derive_outline_path() {
        let p = derive_outline_png_path(Path::new("/tmp/pixolde.png"));
        assert_eq!(p, PathBuf::from("/tmp/pixolde_outline.png"));
    }

    #[test]
    fn glyph_vertical_offset_matches_legacy_marzipan() {
        let px = 121.0_f32;
        let line_height = 95.0_f32;
        let padding = 5_u32;
        let marzipan_baseline_offset = 20.0_f32;
        let layout_baseline = layout_baseline_for(line_height as u32, padding, px);

        // Metrics captured from Pixolde.ttf at px=121 (fontdue rasterize).
        let expected: &[(&str, u32, i32, f32)] = &[
            ("T cap", 61, 0, 12.0),
            ("e lower", 46, 0, 27.0),
            ("g descender", 62, -16, 27.0),
            ("y descender", 62, -16, 27.0),
            ("apostrophe", 24, 37, 12.0),
        ];

        for (label, height, ymin, want) in expected {
            let offset_y =
                glyph_offset_y(line_height, *height as f32, layout_baseline, *ymin as f32);
            assert_eq!(
                offset_y + marzipan_baseline_offset,
                *want,
                "{label} offset mismatch (offset_y={offset_y}, ymin={ymin})"
            );
        }
    }
}
