use crate::assets::AtlasExclude;
use crate::assets::{model::AssetValue, render_dts_module, render_luau_module};
use anyhow::Context;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};

pub(super) fn atlas_file_ids_from_assets(
    assets: &std::collections::BTreeMap<String, crate::assets::model::AssetValue>,
) -> HashMap<String, String> {
    fn walk(out: &mut HashMap<String, String>, node: &crate::assets::model::AssetValue) {
        let crate::assets::model::AssetValue::Table(map) = node else {
            return;
        };

        for (k, v) in map {
            match v {
                crate::assets::model::AssetValue::String(s) => {
                    if k.ends_with(".png") {
                        out.insert(k.clone(), s.clone());
                    }
                }
                crate::assets::model::AssetValue::Object(meta) => {
                    if k.ends_with(".png") {
                        out.insert(k.clone(), meta.id.clone());
                    }
                }
                crate::assets::model::AssetValue::Table(_) => walk(out, v),
                _ => {}
            }
        }
    }

    let mut out = HashMap::new();
    for (k, v) in assets {
        match v {
            crate::assets::model::AssetValue::String(s) => {
                if k.ends_with(".png") {
                    out.insert(k.clone(), s.clone());
                }
            }
            crate::assets::model::AssetValue::Object(meta) => {
                if k.ends_with(".png") {
                    out.insert(k.clone(), meta.id.clone());
                }
            }
            crate::assets::model::AssetValue::Table(_) => walk(&mut out, v),
            _ => {}
        }
    }
    out
}

pub(super) fn merge_asset_values(
    dest: &mut BTreeMap<String, crate::assets::model::AssetValue>,
    src: &BTreeMap<String, crate::assets::model::AssetValue>,
) {
    use crate::assets::model::AssetValue;

    for (key, value) in src {
        match (dest.get_mut(key), value) {
            (Some(AssetValue::Table(dest_table)), AssetValue::Table(src_table)) => {
                merge_asset_values(dest_table, src_table);
            }
            _ => {
                dest.insert(key.clone(), value.clone());
            }
        }
    }
}

pub(super) fn nest_assets_under_path(
    assets: BTreeMap<String, crate::assets::model::AssetValue>,
    prefix: &[String],
) -> BTreeMap<String, crate::assets::model::AssetValue> {
    use crate::assets::model::AssetValue;

    let mut nested = assets;
    for segment in prefix.iter().rev() {
        let mut wrapped = BTreeMap::new();
        wrapped.insert(segment.clone(), AssetValue::Table(nested));
        nested = wrapped;
    }
    nested
}

pub(super) fn filter_assets_by_exclude(
    assets: &BTreeMap<String, crate::assets::model::AssetValue>,
    exclude: &AtlasExclude,
) -> BTreeMap<String, crate::assets::model::AssetValue> {
    let mut out = BTreeMap::new();
    let mut path = Vec::new();
    walk_asset_values(assets, exclude, &mut path, &mut out);
    out
}

pub(super) fn walk_asset_values(
    assets: &BTreeMap<String, crate::assets::model::AssetValue>,
    exclude: &AtlasExclude,
    path: &mut Vec<String>,
    out: &mut BTreeMap<String, crate::assets::model::AssetValue>,
) {
    use crate::assets::model::AssetValue;

    for (key, value) in assets {
        path.push(key.clone());
        match value {
            AssetValue::Table(map) => {
                walk_asset_values(map, exclude, path, out);
            }
            _ => {
                if key.ends_with(".png") {
                    let joined = path.join("/");
                    if exclude.is_match(&joined) {
                        insert_asset_value(out, path, value.clone());
                    }
                }
            }
        }
        path.pop();
    }
}

pub(super) fn insert_asset_value(
    root: &mut BTreeMap<String, crate::assets::model::AssetValue>,
    path: &[String],
    value: crate::assets::model::AssetValue,
) {
    use crate::assets::model::AssetValue;

    if path.is_empty() {
        return;
    }

    if path.len() == 1 {
        root.insert(path[0].clone(), value);
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

    insert_asset_value(map, &path[1..], value);
}

pub(super) fn publish(
    assets: &BTreeMap<String, AssetValue>,
    luau: &Path,
    dts: &Path,
) -> anyhow::Result<()> {
    write_changed(luau, &render_luau_module(assets))?;
    write_changed(dts, &render_dts_module(assets))?;
    println!("[sync] Done");
    Ok(())
}

fn write_changed(path: &Path, contents: &str) -> anyhow::Result<()> {
    match fs::read(path) {
        Ok(previous) if previous == contents.as_bytes() => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", path.display()))
        }
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("Failed to write {}", path.display()))
}
