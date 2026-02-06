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
    pub rect_x: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect_y: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect_w: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect_h: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight_rect_x: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight_rect_y: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight_rect_w: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight_rect_h: Option<u32>,
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
        AssetValue::Object(_) => None,
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

    let rect_x = map
        .get("rectX")
        .or_else(|| map.get("rect_x"))
        .and_then(value_as_u32);
    let rect_y = map
        .get("rectY")
        .or_else(|| map.get("rect_y"))
        .and_then(value_as_u32);
    let rect_w = map
        .get("rectW")
        .or_else(|| map.get("rect_w"))
        .and_then(value_as_u32);
    let rect_h = map
        .get("rectH")
        .or_else(|| map.get("rect_h"))
        .and_then(value_as_u32);

    let highlight_rect_x = map
        .get("highlightRectX")
        .or_else(|| map.get("highlight_rect_x"))
        .and_then(value_as_u32);
    let highlight_rect_y = map
        .get("highlightRectY")
        .or_else(|| map.get("highlight_rect_y"))
        .and_then(value_as_u32);
    let highlight_rect_w = map
        .get("highlightRectW")
        .or_else(|| map.get("highlight_rect_w"))
        .and_then(value_as_u32);
    let highlight_rect_h = map
        .get("highlightRectH")
        .or_else(|| map.get("highlight_rect_h"))
        .and_then(value_as_u32);

    Some(AssetMeta {
        id,
        width,
        height,
        rect_x,
        rect_y,
        rect_w,
        rect_h,
        highlight_id,
        highlight_rect_x,
        highlight_rect_y,
        highlight_rect_w,
        highlight_rect_h,
    })
}
