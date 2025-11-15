use super::model::{AssetMeta, AssetValue};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub trait ImageMetadataReader: Send + Sync {
    fn dimensions(&self, path: &Path) -> Option<(u32, u32)>;
}

pub struct FsImageMetadata;

impl ImageMetadataReader for FsImageMetadata {
    fn dimensions(&self, path: &Path) -> Option<(u32, u32)> {
        let decoder = png::Decoder::new(std::fs::File::open(path).ok()?);
        let reader = decoder.read_info().ok()?;
        let info = reader.info();
        Some((info.width, info.height))
    }
}

pub fn augment_assets(
    assets: &BTreeMap<String, AssetValue>,
    images_folder: &Path,
    reader: &dyn ImageMetadataReader,
) -> BTreeMap<String, AssetValue> {
    let mut augmented = BTreeMap::new();
    for (category, node) in assets {
        augmented.insert(
            category.clone(),
            augment_node(
                node.clone(),
                assets,
                std::slice::from_ref(category),
                images_folder,
                reader,
            ),
        );
    }
    augmented
}

fn augment_node(
    node: AssetValue,
    assets: &BTreeMap<String, AssetValue>,
    path_segments: &[String],
    images_folder: &Path,
    reader: &dyn ImageMetadataReader,
) -> AssetValue {
    let id_str = match &node {
        AssetValue::String(s) => Some(s.clone()),
        AssetValue::Number(n) => Some(n.to_string()),
        _ => None,
    };

    match node {
        AssetValue::String(_) | AssetValue::Number(_) => {
            let id_str = id_str.unwrap();
            let image_path = build_image_path(images_folder, path_segments);
            let (width, height) = reader.dimensions(&image_path).unwrap_or((0, 0));

            if width == 0 && height == 0 {
                println!(
                    "[sync] WARN: {} is not a PNG or is unreadable – skipping size metadata.",
                    image_path.display()
                );
            }

            let mut meta = AssetMeta {
                id: id_str,
                width: Some(width),
                height: Some(height),
                highlight_id: None,
            };

            if let Some(highlight_id) = get_highlight_asset_id(assets, path_segments) {
                meta.highlight_id = Some(highlight_id);
            }

            AssetValue::Object(meta)
        }
        AssetValue::Object(mut meta) => {
            let image_path = build_image_path(images_folder, path_segments);
            let (width, height) = reader
                .dimensions(&image_path)
                .unwrap_or((meta.width.unwrap_or(0), meta.height.unwrap_or(0)));

            if width == 0 && height == 0 && meta.width.is_none() {
                println!(
                    "[sync] WARN: {} is not a PNG or is unreadable – skipping size metadata.",
                    image_path.display()
                );
            }

            meta.width = Some(width);
            meta.height = Some(height);

            if meta.highlight_id.is_none() {
                if let Some(highlight_id) = get_highlight_asset_id(assets, path_segments) {
                    meta.highlight_id = Some(highlight_id);
                }
            }

            AssetValue::Object(meta)
        }
        AssetValue::Table(map) => {
            let mut result = BTreeMap::new();
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();

            for key in keys {
                let mut child_path = path_segments.to_vec();
                child_path.push(key.clone());
                result.insert(
                    key.clone(),
                    augment_node(
                        map[&key].clone(),
                        assets,
                        &child_path,
                        images_folder,
                        reader,
                    ),
                );
            }

            AssetValue::Table(result)
        }
    }
}

fn build_image_path(images_folder: &Path, segments: &[String]) -> PathBuf {
    let relative = segments.join("/");
    images_folder.join(relative)
}

fn get_highlight_asset_id(
    assets: &BTreeMap<String, AssetValue>,
    path_segments: &[String],
) -> Option<String> {
    let last_segment = path_segments.last()?;
    if last_segment.ends_with("-highlight.png") {
        return None;
    }

    let mut highlight_path = path_segments.to_vec();
    if let Some(last) = highlight_path.last_mut() {
        *last = last.replace(".png", "-highlight.png");
    }

    let mut node = Some(AssetValue::Table(assets.clone()));
    for segment in &highlight_path {
        node = match node? {
            AssetValue::Table(map) => map.get(segment).cloned(),
            _ => None,
        };
    }

    match node? {
        AssetValue::String(s) => Some(s),
        AssetValue::Number(n) => Some(n.to_string()),
        AssetValue::Object(meta) => Some(meta.id),
        _ => None,
    }
}
