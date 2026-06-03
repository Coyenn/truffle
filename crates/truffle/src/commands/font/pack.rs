use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy)]
pub struct PackRect {
    pub page: u32,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone)]
struct PendingItem {
    index: usize,
    alloc_w: u32,
    alloc_h: u32,
    inner_w: u32,
    inner_h: u32,
}

/// Pack variable-size rectangles into multiple square atlases using shelf packing.
pub fn pack_glyphs(
    sizes: &[(u32, u32)],
    padding: u32,
    atlas_size: u32,
) -> Result<Vec<PackRect>> {
    if atlas_size == 0 {
        anyhow::bail!("atlas size must be > 0");
    }

    let mut pending: Vec<PendingItem> = sizes
        .iter()
        .enumerate()
        .map(|(index, &(w, h))| {
            let alloc_w = w.saturating_add(padding.saturating_mul(2));
            let alloc_h = h.saturating_add(padding.saturating_mul(2));
            PendingItem {
                index,
                alloc_w,
                alloc_h,
                inner_w: w,
                inner_h: h,
            }
        })
        .collect();

    pending.sort_by(|a, b| {
        b.alloc_h
            .cmp(&a.alloc_h)
            .then_with(|| b.alloc_w.cmp(&a.alloc_w))
            .then_with(|| a.index.cmp(&b.index))
    });

    let mut out = vec![
        PackRect {
            page: 0,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        };
        sizes.len()
    ];

    let mut page: u32 = 0;
    let mut cursor_x: u32 = 0;
    let mut cursor_y: u32 = 0;
    let mut row_h: u32 = 0;

    for item in pending {
        if item.alloc_w > atlas_size || item.alloc_h > atlas_size {
            anyhow::bail!(
                "glyph {}x{} (with padding) exceeds atlas size {}x{}",
                item.inner_w,
                item.inner_h,
                atlas_size,
                atlas_size
            );
        }

        if cursor_x.saturating_add(item.alloc_w) > atlas_size {
            cursor_x = 0;
            cursor_y = cursor_y.saturating_add(row_h);
            row_h = 0;
        }

        if cursor_y.saturating_add(item.alloc_h) > atlas_size {
            page += 1;
            cursor_x = 0;
            cursor_y = 0;
            row_h = 0;
        }

        out[item.index] = PackRect {
            page,
            x: cursor_x + padding,
            y: cursor_y + padding,
            w: item.inner_w,
            h: item.inner_h,
        };

        cursor_x = cursor_x.saturating_add(item.alloc_w);
        row_h = row_h.max(item.alloc_h);
    }

    Ok(out)
}

pub fn validate_atlas_size(size: u32) -> Result<u32> {
    const MIN: u32 = 256;
    const MAX: u32 = 4096;
    if size < MIN || size > MAX {
        anyhow::bail!("atlas size must be between {MIN} and {MAX}");
    }
    if !size.is_power_of_two() {
        anyhow::bail!("atlas size must be a power of two");
    }
    Ok(size)
}

pub fn page_png_path(base: &std::path::Path, page: u32, page_count: u32) -> std::path::PathBuf {
    if page_count <= 1 {
        return base.to_path_buf();
    }
    let parent = base.parent().unwrap_or_else(|| std::path::Path::new("."));
    let stem = base
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("font");
    let ext = base
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("png");
    parent.join(format!("{stem}_{page}.{ext}"))
}

pub fn write_atlas_pages(
    pages: &[image::RgbaImage],
    base_path: &std::path::Path,
) -> Result<()> {
    let page_count = pages.len() as u32;
    for (i, atlas) in pages.iter().enumerate() {
        let path = page_png_path(base_path, i as u32, page_count);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create atlas output dir: {}", parent.display())
            })?;
        }
        atlas
            .save(&path)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_single_glyph() {
        let rects = pack_glyphs(&[(8, 16)], 1, 64).unwrap();
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].page, 0);
        assert_eq!(rects[0].w, 8);
        assert_eq!(rects[0].h, 16);
    }

    #[test]
    fn page_path_single_page_uses_base() {
        let p = std::path::Path::new("/tmp/pixolde.png");
        assert_eq!(page_png_path(p, 0, 1), p);
    }

    #[test]
    fn page_path_multi_page() {
        let p = std::path::Path::new("/tmp/pixolde.png");
        assert_eq!(
            page_png_path(p, 1, 2),
            std::path::PathBuf::from("/tmp/pixolde_1.png")
        );
    }
}
