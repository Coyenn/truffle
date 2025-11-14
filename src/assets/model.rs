use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum AssetValue {
    String(String),
    Number(f64),
    Object(AssetMeta),
    Table(BTreeMap<String, AssetValue>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssetMeta {
    pub id: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight_id: Option<String>,
}

pub(crate) fn asset_value_to_string(value: &AssetValue) -> Option<String> {
    match value {
        AssetValue::String(s) => Some(s.clone()),
        AssetValue::Number(n) => Some(n.to_string()),
        AssetValue::Object(meta) => Some(meta.id.clone()),
        _ => None,
    }
}

pub(crate) fn value_as_u32(value: &AssetValue) -> Option<u32> {
    match value {
        AssetValue::Number(n) if *n >= 0.0 => Some(*n as u32),
        AssetValue::String(s) => s.parse::<u32>().ok(),
        AssetValue::Object(meta) => meta.width.or(meta.height),
        _ => None,
    }
}

pub(crate) fn convert_map_to_asset_meta(map: &BTreeMap<String, AssetValue>) -> Option<AssetMeta> {
    let id = asset_value_to_string(map.get("id")?)?;

    let width = map.get("width").and_then(value_as_u32);
    let height = map.get("height").and_then(value_as_u32);

    let highlight_id = map
        .get("highlightId")
        .or_else(|| map.get("highlight_id"))
        .and_then(asset_value_to_string);

    Some(AssetMeta {
        id,
        width,
        height,
        highlight_id,
    })
}
