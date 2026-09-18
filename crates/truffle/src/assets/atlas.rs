use super::model::{AssetMeta, AssetValue};
use super::pack::{self, Rect as PackRect, SeedRect};
use anyhow::{Context, Result};
use asphalt::glob::Glob;
use image::{GenericImageView, ImageBuffer, Rgba};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MAX_ATLAS_SIZE: u32 = 4096;
const MIN_ATLAS_SIZE: u32 = 256;

#[derive(Debug, Clone)]
pub struct AtlasOptions {
    pub padding: u32,
    pub size: u32,
    pub exclude: AtlasExclude,
}

impl Default for AtlasOptions {
    fn default() -> Self {
        Self {
            padding: 4,
            size: 1024,
            exclude: AtlasExclude::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AtlasExclude {
    pub exact: HashSet<String>,
    pub globs: Vec<Glob>,
}

impl AtlasExclude {
    pub fn is_match(&self, key: &str) -> bool {
        if self.exact.contains(key) {
            return true;
        }

        self.globs.iter().any(|glob| glob.is_match(key))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Persisted packing state used to keep atlases stable across syncs.
///
/// A sprite that kept its size and page is pinned to its previous rect, so an
/// added/removed sprite only disturbs the free space it actually touches.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AtlasState {
    atlas_size: u32,
    padding: u32,
    placements: BTreeMap<String, StoredPlacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPlacement {
    page: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

pub fn build_atlases(
    images_folder: &Path,
    output_dir: &Path,
    state_path: &Path,
    options: AtlasOptions,
) -> Result<BTreeMap<String, SpritePlacement>> {
    let atlas_size = validate_atlas_size(options.size)?;

    let previous = match load_state(state_path) {
        Ok(state) if state.atlas_size == atlas_size && state.padding == options.padding => {
            state.placements
        }
        _ => BTreeMap::new(),
    };

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

    let sprites = scan_pngs(images_folder, &options.exclude)?;
    let placed = plan_atlas(&sprites, &previous, options.padding, atlas_size)?;

    write_atlas_images(&placed, output_dir, options.padding, atlas_size)?;

    let mut placements = BTreeMap::new();
    let mut state = BTreeMap::new();
    for sprite in placed {
        placements.insert(
            sprite.key.clone(),
            SpritePlacement {
                atlas_file_name: atlas_file_name(sprite.atlas_index),
                rect: sprite.rect,
            },
        );
        state.insert(
            sprite.key,
            StoredPlacement {
                page: sprite.atlas_index as u32,
                x: sprite.rect.x,
                y: sprite.rect.y,
                w: sprite.rect.w,
                h: sprite.rect.h,
            },
        );
    }

    save_state(
        state_path,
        AtlasState {
            atlas_size,
            padding: options.padding,
            placements: state,
        },
    )?;

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

fn scan_pngs(images_folder: &Path, exclude: &AtlasExclude) -> Result<Vec<PendingSprite>> {
    let mut sprites = Vec::new();
    for entry in WalkDir::new(images_folder).follow_links(false).into_iter() {
        let entry = entry
            .with_context(|| format!("failed to read entry under {}", images_folder.display()))?;
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

        if exclude.is_match(&key) {
            continue;
        }

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

    sprites.sort_by(|a, b| a.key.cmp(&b.key));

    Ok(sprites)
}

/// Pin previously placed sprites and pack only new/changed ones into leftover
/// free space, so an added sprite does not disturb existing atlases.
fn plan_atlas(
    sprites: &[PendingSprite],
    previous: &BTreeMap<String, StoredPlacement>,
    padding: u32,
    atlas_size: u32,
) -> Result<Vec<PlacedSprite>> {
    let gutter = padding.saturating_mul(2);

    // Fixed sprites keep their page + rect; everything else is repacked.
    let mut fixed_by_page: BTreeMap<u32, Vec<PackRect>> = BTreeMap::new();
    let mut fixed_placed: Vec<PlacedSprite> = Vec::new();
    let mut to_pack: Vec<&PendingSprite> = Vec::new();

    for sprite in sprites {
        match previous.get(&sprite.key) {
            Some(prev) if prev.w == sprite.w && prev.h == sprite.h => {
                let alloc = PackRect {
                    x: prev.x.saturating_sub(padding),
                    y: prev.y.saturating_sub(padding),
                    w: sprite.w.saturating_add(gutter),
                    h: sprite.h.saturating_add(gutter),
                };
                fixed_by_page.entry(prev.page).or_default().push(alloc);
                fixed_placed.push(PlacedSprite {
                    key: sprite.key.clone(),
                    src_path: sprite.src_path.clone(),
                    atlas_index: prev.page as usize,
                    rect: AtlasRect {
                        x: prev.x,
                        y: prev.y,
                        w: prev.w,
                        h: prev.h,
                    },
                });
            }
            _ => to_pack.push(sprite),
        }
    }

    // Reconstruct free space on each page that has fixed sprites.
    let mut seed: Vec<SeedRect> = Vec::new();
    for (page, allocs) in &fixed_by_page {
        seed.extend(
            pack::free_space(atlas_size, allocs)
                .into_iter()
                .map(|rect| SeedRect { page: *page, rect }),
        );
    }

    let sizes: Vec<(u32, u32)> = to_pack.iter().map(|s| (s.w, s.h)).collect();
    let packed = pack::pack(&sizes, padding, atlas_size, &seed)?;

    let mut placed = fixed_placed;
    for (i, sprite) in to_pack.into_iter().enumerate() {
        let p = packed[i];
        placed.push(PlacedSprite {
            key: sprite.key.clone(),
            src_path: sprite.src_path.clone(),
            atlas_index: p.page as usize,
            rect: AtlasRect {
                x: p.rect.x,
                y: p.rect.y,
                w: p.rect.w,
                h: p.rect.h,
            },
        });
    }

    Ok(placed)
}

fn load_state(path: &Path) -> Result<AtlasState> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return Ok(AtlasState::default()),
    };
    match toml::from_str(&content) {
        Ok(state) => Ok(state),
        Err(_) => Ok(AtlasState::default()),
    }
}

fn save_state(path: &Path, state: AtlasState) -> Result<()> {
    let mut content = toml::to_string(&state).context("failed to serialize atlas state")?;
    content.insert_str(
        0,
        "# This file is automatically @generated by Truffle.\n# It is not intended for manual editing.\n",
    );
    std::fs::write(path, content)
        .with_context(|| format!("failed to write atlas state to {}", path.display()))
}

fn write_atlas_images(
    placed: &[PlacedSprite],
    output_dir: &Path,
    padding: u32,
    atlas_size: u32,
) -> Result<()> {
    let mut per_atlas: HashMap<usize, Vec<&PlacedSprite>> = HashMap::new();
    for s in placed {
        per_atlas.entry(s.atlas_index).or_default().push(s);
    }

    let mut atlas_indices: Vec<usize> = per_atlas.keys().cloned().collect();
    atlas_indices.sort();

    for atlas_index in atlas_indices {
        let sprites = per_atlas.get(&atlas_index).unwrap();
        let mut atlas: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(atlas_size, atlas_size, Rgba([0, 0, 0, 0]));

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
    _padding: u32,
) {
    let w = src.width();
    let h = src.height();

    for dy in 0..h {
        for dx in 0..w {
            let p = src.get_pixel(dx, dy);
            let tx = inner_x + dx;
            let ty = inner_y + dy;
            if tx < dst.width() && ty < dst.height() {
                dst.put_pixel(tx, ty, *p);
            }
        }
    }
}

fn atlas_file_name(atlas_index: usize) -> String {
    format!("atlas_{:03}.png", atlas_index)
}

fn validate_atlas_size(size: u32) -> Result<u32> {
    if !(MIN_ATLAS_SIZE..=MAX_ATLAS_SIZE).contains(&size) {
        anyhow::bail!(
            "atlas size must be between {} and {}",
            MIN_ATLAS_SIZE,
            MAX_ATLAS_SIZE
        );
    }

    if !size.is_power_of_two() {
        anyhow::bail!("atlas size must be a power of two");
    }

    Ok(size)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(key: &str, w: u32, h: u32) -> PendingSprite {
        PendingSprite {
            key: key.to_string(),
            src_path: PathBuf::from(key),
            w,
            h,
        }
    }

    fn overlaps(a: &AtlasRect, b: &AtlasRect) -> bool {
        let a_r = a.x + a.w;
        let a_b = a.y + a.h;
        let b_r = b.x + b.w;
        let b_b = b.y + b.h;
        a.x < b_r && b.x < a_r && a.y < b_b && b.y < a_b
    }

    #[test]
    fn added_sprite_does_not_move_existing_sprites() {
        let padding = 4u32;
        let size = 1024u32;

        let mut previous = BTreeMap::new();
        previous.insert(
            "a.png".to_string(),
            StoredPlacement {
                page: 0,
                x: 4,
                y: 4,
                w: 64,
                h: 64,
            },
        );
        previous.insert(
            "b.png".to_string(),
            StoredPlacement {
                page: 0,
                x: 76,
                y: 4,
                w: 64,
                h: 64,
            },
        );

        let sprites = vec![
            pending("a.png", 64, 64),
            pending("b.png", 64, 64),
            pending("c.png", 64, 64),
        ];

        let placed = plan_atlas(&sprites, &previous, padding, size).unwrap();
        let by_key: BTreeMap<_, _> = placed.iter().map(|p| (p.key.as_str(), p)).collect();

        let a = by_key["a.png"];
        let b = by_key["b.png"];
        assert_eq!(
            a.rect,
            AtlasRect {
                x: 4,
                y: 4,
                w: 64,
                h: 64
            }
        );
        assert_eq!(
            b.rect,
            AtlasRect {
                x: 76,
                y: 4,
                w: 64,
                h: 64
            }
        );

        // The new sprite landed on the same page without disturbing a or b.
        let c = by_key["c.png"];
        assert_eq!(c.atlas_index, 0);
        assert!(!overlaps(&c.rect, &a.rect));
        assert!(!overlaps(&c.rect, &b.rect));
    }

    #[test]
    fn removed_sprite_frees_space_and_keeps_others() {
        let padding = 4u32;
        let size = 1024u32;

        let mut previous = BTreeMap::new();
        previous.insert(
            "a.png".to_string(),
            StoredPlacement {
                page: 0,
                x: 4,
                y: 4,
                w: 64,
                h: 64,
            },
        );
        previous.insert(
            "gone.png".to_string(),
            StoredPlacement {
                page: 0,
                x: 76,
                y: 4,
                w: 64,
                h: 64,
            },
        );

        let sprites = vec![pending("a.png", 64, 64), pending("c.png", 64, 64)];
        let placed = plan_atlas(&sprites, &previous, padding, size).unwrap();
        let by_key: BTreeMap<_, _> = placed.iter().map(|p| (p.key.as_str(), p)).collect();

        assert_eq!(
            by_key["a.png"].rect,
            AtlasRect {
                x: 4,
                y: 4,
                w: 64,
                h: 64
            }
        );
        assert_eq!(by_key["a.png"].atlas_index, 0);
        // c may reuse the freed slot, but must not overlap a.
        assert!(!overlaps(&by_key["c.png"].rect, &by_key["a.png"].rect));
    }

    #[test]
    fn resized_sprite_is_repacked_not_pinned() {
        let padding = 4u32;
        let size = 1024u32;

        let mut previous = BTreeMap::new();
        previous.insert(
            "a.png".to_string(),
            StoredPlacement {
                page: 0,
                x: 4,
                y: 4,
                w: 64,
                h: 64,
            },
        );

        // a.png grew, so its old placement is stale.
        let sprites = vec![pending("a.png", 128, 128)];
        let placed = plan_atlas(&sprites, &previous, padding, size).unwrap();
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].rect.w, 128);
        assert_eq!(placed[0].rect.h, 128);
    }

    #[test]
    fn state_roundtrips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("atlas-state.toml");

        let mut placements = BTreeMap::new();
        placements.insert(
            "a.png".to_string(),
            StoredPlacement {
                page: 2,
                x: 10,
                y: 20,
                w: 30,
                h: 40,
            },
        );
        save_state(
            &path,
            AtlasState {
                atlas_size: 1024,
                padding: 4,
                placements,
            },
        )
        .unwrap();

        let loaded = load_state(&path).unwrap();
        assert_eq!(loaded.atlas_size, 1024);
        assert_eq!(loaded.padding, 4);
        assert_eq!(loaded.placements["a.png"].page, 2);
        assert_eq!(loaded.placements["a.png"].w, 30);
    }

    #[test]
    fn missing_state_defaults_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_state(&dir.path().join("nope.toml")).unwrap();
        assert!(loaded.placements.is_empty());
    }
}
