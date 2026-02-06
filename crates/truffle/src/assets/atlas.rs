use super::model::{AssetMeta, AssetValue};
use anyhow::{Context, Result};
use image::{GenericImageView, ImageBuffer, Rgba};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MAX_ATLAS_SIZE: u32 = 4096;

#[derive(Debug, Clone)]
pub struct AtlasOptions {
    pub padding: u32,
}

impl Default for AtlasOptions {
    fn default() -> Self {
        Self { padding: 4 }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AtlasRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone)]
pub struct SpritePlacement {
    pub atlas_file_name: String,
    pub rect: AtlasRect,
}

#[derive(Debug, Clone)]
struct PendingSprite {
    key: String,
    src_path: PathBuf,
    w: u32,
    h: u32,
}

#[derive(Debug, Clone)]
struct PlacedSprite {
    key: String,
    src_path: PathBuf,
    atlas_index: usize,
    rect: AtlasRect,
}

pub fn build_atlases(
    images_folder: &Path,
    output_dir: &Path,
    options: AtlasOptions,
) -> Result<BTreeMap<String, SpritePlacement>> {
    if output_dir.exists() {
        std::fs::remove_dir_all(output_dir).with_context(|| {
            format!("failed to clean atlas output dir: {}", output_dir.display())
        })?;
    }
    std::fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "failed to create atlas output dir: {}",
            output_dir.display()
        )
    })?;

    let sprites = scan_pngs(images_folder)?;
    let placed = pack_sprites(&sprites, options.padding)?;

    write_atlas_images(&placed, output_dir, options.padding)?;

    let mut placements = BTreeMap::new();
    for sprite in placed {
        placements.insert(
            sprite.key,
            SpritePlacement {
                atlas_file_name: atlas_file_name(sprite.atlas_index),
                rect: sprite.rect,
            },
        );
    }
    Ok(placements)
}

pub fn build_atlased_assets(
    placements: &BTreeMap<String, SpritePlacement>,
    atlas_ids: &HashMap<String, String>,
) -> Result<BTreeMap<String, AssetValue>> {
    let mut root = BTreeMap::new();

    for (key, placement) in placements {
        let atlas_id = atlas_ids
            .get(&placement.atlas_file_name)
            .cloned()
            .with_context(|| format!("missing atlas id for {}", placement.atlas_file_name))?;

        let mut meta = AssetMeta {
            id: atlas_id,
            width: Some(placement.rect.w),
            height: Some(placement.rect.h),
            rect_x: Some(placement.rect.x),
            rect_y: Some(placement.rect.y),
            rect_w: Some(placement.rect.w),
            rect_h: Some(placement.rect.h),
            highlight_id: None,
            highlight_rect_x: None,
            highlight_rect_y: None,
            highlight_rect_w: None,
            highlight_rect_h: None,
        };

        if !key.ends_with("-highlight.png") {
            let highlight_key = key.replace(".png", "-highlight.png");
            if let Some(highlight) = placements.get(&highlight_key) {
                if let Some(h_id) = atlas_ids.get(&highlight.atlas_file_name) {
                    meta.highlight_id = Some(h_id.clone());
                    meta.highlight_rect_x = Some(highlight.rect.x);
                    meta.highlight_rect_y = Some(highlight.rect.y);
                    meta.highlight_rect_w = Some(highlight.rect.w);
                    meta.highlight_rect_h = Some(highlight.rect.h);
                }
            }
        }

        insert_meta(&mut root, &split_key(key), meta);
    }

    Ok(root)
}

fn scan_pngs(images_folder: &Path) -> Result<Vec<PendingSprite>> {
    let mut sprites = Vec::new();
    for entry in WalkDir::new(images_folder)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("png") {
            continue;
        }

        let rel = path
            .strip_prefix(images_folder)
            .with_context(|| format!("failed to get relative path for {}", path.display()))?;

        let key = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        let img = image::open(path)
            .with_context(|| format!("failed to decode png: {}", path.display()))?;
        let (w, h) = img.dimensions();

        sprites.push(PendingSprite {
            key,
            src_path: path.to_path_buf(),
            w,
            h,
        });
    }

    sprites.sort_by(|a, b| {
        b.h.cmp(&a.h)
            .then_with(|| b.w.cmp(&a.w))
            .then_with(|| a.key.cmp(&b.key))
    });

    Ok(sprites)
}

fn pack_sprites(sprites: &[PendingSprite], padding: u32) -> Result<Vec<PlacedSprite>> {
    let mut atlas_index: usize = 0;
    let mut cursor_x: u32 = 0;
    let mut cursor_y: u32 = 0;
    let mut row_h: u32 = 0;

    let mut placed = Vec::with_capacity(sprites.len());

    for s in sprites {
        let alloc_w = s.w + padding.saturating_mul(2);
        let alloc_h = s.h + padding.saturating_mul(2);

        if alloc_w > MAX_ATLAS_SIZE || alloc_h > MAX_ATLAS_SIZE {
            anyhow::bail!(
                "{} is too large to pack into a {}x{} atlas ({}x{})",
                s.key,
                MAX_ATLAS_SIZE,
                MAX_ATLAS_SIZE,
                s.w,
                s.h
            );
        }

        if cursor_x.saturating_add(alloc_w) > MAX_ATLAS_SIZE {
            cursor_x = 0;
            cursor_y = cursor_y.saturating_add(row_h);
            row_h = 0;
        }

        if cursor_y.saturating_add(alloc_h) > MAX_ATLAS_SIZE {
            atlas_index += 1;
            cursor_x = 0;
            cursor_y = 0;
            row_h = 0;
        }

        let rect = AtlasRect {
            x: cursor_x + padding,
            y: cursor_y + padding,
            w: s.w,
            h: s.h,
        };

        placed.push(PlacedSprite {
            key: s.key.clone(),
            src_path: s.src_path.clone(),
            atlas_index,
            rect,
        });

        cursor_x = cursor_x.saturating_add(alloc_w);
        row_h = row_h.max(alloc_h);
    }

    Ok(placed)
}

fn write_atlas_images(placed: &[PlacedSprite], output_dir: &Path, padding: u32) -> Result<()> {
    let mut per_atlas: HashMap<usize, Vec<&PlacedSprite>> = HashMap::new();
    for s in placed {
        per_atlas.entry(s.atlas_index).or_default().push(s);
    }

    let mut atlas_indices: Vec<usize> = per_atlas.keys().cloned().collect();
    atlas_indices.sort();

    for atlas_index in atlas_indices {
        let sprites = per_atlas.get(&atlas_index).unwrap();
        let mut atlas: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(MAX_ATLAS_SIZE, MAX_ATLAS_SIZE, Rgba([0, 0, 0, 0]));

        for s in sprites {
            let img = image::open(&s.src_path)
                .with_context(|| format!("failed to decode png: {}", s.src_path.display()))?
                .to_rgba8();
            blit_with_extrude(&mut atlas, &img, s.rect.x, s.rect.y, padding);
        }

        let path = output_dir.join(atlas_file_name(atlas_index));
        image::DynamicImage::ImageRgba8(atlas)
            .save(&path)
            .with_context(|| format!("failed to write atlas png: {}", path.display()))?;
    }

    Ok(())
}

fn blit_with_extrude(
    dst: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    src: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    inner_x: u32,
    inner_y: u32,
    padding: u32,
) {
    let w = src.width();
    let h = src.height();
    let start_x = inner_x.saturating_sub(padding);
    let start_y = inner_y.saturating_sub(padding);
    let out_w = w + padding.saturating_mul(2);
    let out_h = h + padding.saturating_mul(2);

    for dy in 0..out_h {
        for dx in 0..out_w {
            let sx = dx.saturating_sub(padding).min(w.saturating_sub(1));
            let sy = dy.saturating_sub(padding).min(h.saturating_sub(1));
            let p = src.get_pixel(sx, sy);
            let tx = start_x + dx;
            let ty = start_y + dy;
            if tx < dst.width() && ty < dst.height() {
                dst.put_pixel(tx, ty, *p);
            }
        }
    }
}

fn atlas_file_name(atlas_index: usize) -> String {
    format!("atlas_{:03}.png", atlas_index)
}

fn split_key(key: &str) -> Vec<String> {
    key.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn insert_meta(root: &mut BTreeMap<String, AssetValue>, path: &[String], meta: AssetMeta) {
    if path.is_empty() {
        return;
    }

    if path.len() == 1 {
        root.insert(path[0].clone(), AssetValue::Object(meta));
        return;
    }

    let head = path[0].clone();
    let entry = root
        .entry(head)
        .or_insert_with(|| AssetValue::Table(BTreeMap::new()));

    if !matches!(entry, AssetValue::Table(_)) {
        *entry = AssetValue::Table(BTreeMap::new());
    }

    let AssetValue::Table(map) = entry else {
        return;
    };

    insert_meta(map, &path[1..], meta);
}
