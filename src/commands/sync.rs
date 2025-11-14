use clap::Parser;
use full_moon::{ast, tokenizer::TokenType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Parser)]
#[command(about = "Sync assets and augment metadata with image dimensions")]
pub struct SyncArgs {
    /// Path to the Luau assets module file
    #[arg(long, default_value = "src/shared/data/assets/assets.luau")]
    pub assets_input: PathBuf,

    /// Path to write the augmented Luau assets module
    #[arg(long, default_value = "src/shared/data/assets/assets.luau")]
    pub assets_output: PathBuf,

    /// Path to write the TypeScript declaration file
    #[arg(long, default_value = "src/shared/data/assets/assets.d.ts")]
    pub dts_output: PathBuf,

    /// Path to the raw assets images folder
    #[arg(long, default_value = "assets/images")]
    pub images_folder: PathBuf,

    /// ASPHALT_API_KEY environment variable (or read from .env file)
    #[arg(long)]
    pub asphalt_api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum AssetValue {
    String(String),
    Number(f64),
    Object(AssetMeta),
    Table(BTreeMap<String, AssetValue>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AssetMeta {
    id: String,
    width: Option<u32>,
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    highlight_id: Option<String>,
}

fn check_asphalt() -> Result<(), String> {
    let output = Command::new("asphalt").arg("--version").output();

    if output.is_err() {
        let output2 = Command::new("asphalt").output();
        if output2.is_err() {
            return Err("asphalt command is not available. Please install asphalt.".to_string());
        }
    }

    Ok(())
}

fn get_asphalt_api_key(provided: Option<String>) -> Result<String, String> {
    if let Some(key) = provided {
        return Ok(key);
    }

    if let Ok(key) = std::env::var("ASPHALT_API_KEY") {
        return Ok(key);
    }

    if let Ok(env_content) = fs::read_to_string(".env") {
        for line in env_content.lines() {
            if let Some(key) = line.strip_prefix("ASPHALT_API_KEY=") {
                return Ok(key.trim().to_string());
            }
        }
    }

    Err("ASPHALT_API_KEY environment variable is not set. Not syncing assets.".to_string())
}

fn get_png_dimensions(png_path: &Path) -> Option<(u32, u32)> {
    let decoder = png::Decoder::new(match fs::File::open(png_path) {
        Ok(f) => f,
        Err(_) => return None,
    });

    let reader = match decoder.read_info() {
        Ok(r) => r,
        Err(_) => return None,
    };

    let info = reader.info();
    Some((info.width, info.height))
}

fn load_assets(path: &Path) -> Result<BTreeMap<String, AssetValue>, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read assets file: {}", e))?;

    // If file is JSON, parse as JSON
    if path.extension().and_then(|s| s.to_str()) == Some("json") {
        let json_value: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse JSON: {}", e))?;
        return parse_json_value(json_value);
    }

    // Otherwise try to parse as Luau using full-moon parser
    // The file structure can be:
    //   - return { assets = { ... } }
    //   - local assets = { ... }; return { assets = assets }
    parse_luau_assets_module(&content)
}

/// Parse Luau assets module using full-moon parser.
///
/// Supports two module formats:
/// 1. `return { assets = { ... } }` - direct return with assets table
/// 2. `local assets = { ... }; return { assets = assets }` - local variable pattern
fn parse_luau_assets_module(content: &str) -> Result<BTreeMap<String, AssetValue>, String> {
    let ast = full_moon::parse(content).map_err(|errors| {
        let details = errors
            .iter()
            .map(|e| format!("{:?}", e))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Failed to parse Luau: {}", details)
    })?;

    let block = ast.nodes();

    if let Some(table) = find_local_assets_table(block) {
        return convert_table_to_asset_value(table);
    }

    if let Some(table) = find_assets_table_in_return(block) {
        return convert_table_to_asset_value(table);
    }

    Err("Could not find assets table in Luau file".to_string())
}

fn find_local_assets_table<'a>(block: &'a ast::Block) -> Option<&'a ast::TableConstructor> {
    for stmt in block.stmts() {
        if let ast::Stmt::LocalAssignment(local_assign) = stmt {
            for (name, expr) in local_assign
                .names()
                .iter()
                .zip(local_assign.expressions().iter())
            {
                if name.to_string().trim() == "assets" {
                    if let ast::Expression::TableConstructor(table) = expr {
                        return Some(table);
                    }
                }
            }
        }
    }
    None
}

fn find_assets_table_in_return<'a>(block: &'a ast::Block) -> Option<&'a ast::TableConstructor> {
    match block.last_stmt()? {
        ast::LastStmt::Return(ret) => {
            for expr in ret.returns().iter() {
                match expr {
                    ast::Expression::TableConstructor(table) => {
                        if let Some(inner) = find_assets_table_in_table(block, table) {
                            return Some(inner);
                        }
                    }
                    ast::Expression::Var(var) => {
                        if let Some(table) = resolve_assets_var(block, var) {
                            return Some(table);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}

fn find_assets_table_in_table<'a>(
    block: &'a ast::Block,
    table: &'a ast::TableConstructor,
) -> Option<&'a ast::TableConstructor> {
    for field in table.fields() {
        if let ast::Field::NameKey { key, value, .. } = field {
            if key.to_string().trim() == "assets" {
                return match value {
                    ast::Expression::TableConstructor(inner) => Some(inner),
                    ast::Expression::Var(var) => resolve_assets_var(block, var),
                    _ => None,
                };
            }
        }
    }
    None
}

fn resolve_assets_var<'a>(
    block: &'a ast::Block,
    var: &'a ast::Var,
) -> Option<&'a ast::TableConstructor> {
    if let ast::Var::Name(name_ref) = var {
        if name_ref.to_string().trim() == "assets" {
            return find_local_assets_table(block);
        }
    }
    None
}

fn convert_table_to_asset_value(
    table: &ast::TableConstructor,
) -> Result<BTreeMap<String, AssetValue>, String> {
    let mut result = BTreeMap::new();

    for field in table.fields() {
        let (key, value_expr) = match field {
            ast::Field::NameKey {
                key,
                value,
                equal: _,
            } => (key.to_string().trim().to_string(), value),
            ast::Field::ExpressionKey {
                key,
                value,
                brackets: _,
                equal: _,
            } => {
                // Handle [key] = value syntax
                let key_str = match key {
                    ast::Expression::String(_) => extract_string_value(key)
                        .unwrap_or_else(|_| key.to_string().trim().to_string()),
                    _ => key.to_string().trim().to_string(),
                };
                (key_str, value)
            }
            ast::Field::NoKey(_value) => {
                // Array-style entries - skip for now as assets use named keys
                continue;
            }
            _ => {
                // Handle any other field types
                continue;
            }
        };

        let asset_value = convert_expr_to_asset_value(value_expr)?;
        result.insert(key, asset_value);
    }

    Ok(result)
}

fn asset_value_to_string(value: &AssetValue) -> Option<String> {
    match value {
        AssetValue::String(s) => Some(s.clone()),
        AssetValue::Number(n) => Some(n.to_string()),
        AssetValue::Object(meta) => Some(meta.id.clone()),
        _ => None,
    }
}

fn convert_map_to_asset_meta(map: &BTreeMap<String, AssetValue>) -> Option<AssetMeta> {
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

fn value_as_u32(value: &AssetValue) -> Option<u32> {
    match value {
        AssetValue::Number(n) if *n >= 0.0 => Some(*n as u32),
        AssetValue::String(s) => s.parse::<u32>().ok(),
        AssetValue::Object(meta) => meta.width.or(meta.height),
        _ => None,
    }
}

/// Extract the string value from a full-moon string literal expression.
fn extract_string_value(expr: &ast::Expression) -> Result<String, String> {
    if let ast::Expression::String(token_ref) = expr {
        if let TokenType::StringLiteral { literal, .. } = token_ref.token().token_type() {
            return Ok(literal.to_string());
        }
        return Ok(token_ref
            .to_string()
            .trim_start_matches('"')
            .trim_end_matches('"')
            .trim_start_matches('\'')
            .trim_end_matches('\'')
            .to_string());
    }

    Err("Expression is not a string literal".to_string())
}

/// Extract the numeric value from a full-moon number literal expression.
fn extract_number_value(expr: &ast::Expression) -> Result<f64, String> {
    if let ast::Expression::Number(token_ref) = expr {
        if let TokenType::Number { text } = token_ref.token().token_type() {
            let numeric_text = text.to_string();
            return numeric_text
                .parse::<f64>()
                .map_err(|e| format!("Failed to parse number '{}': {}", numeric_text, e));
        }

        let fallback = token_ref.to_string();
        return fallback
            .trim()
            .parse::<f64>()
            .map_err(|e| format!("Failed to parse number '{}': {}", fallback.trim(), e));
    }

    Err("Expression is not a numeric literal".to_string())
}

fn convert_expr_to_asset_value(expr: &ast::Expression) -> Result<AssetValue, String> {
    match expr {
        ast::Expression::String(_) => {
            let unquoted = extract_string_value(expr)?;
            Ok(AssetValue::String(unquoted))
        }
        ast::Expression::Number(_) => {
            let num = extract_number_value(expr)?;
            Ok(AssetValue::Number(num))
        }
        ast::Expression::TableConstructor(table) => {
            let map = convert_table_to_asset_value(table)?;
            if let Some(meta) = convert_map_to_asset_meta(&map) {
                Ok(AssetValue::Object(meta))
            } else {
                Ok(AssetValue::Table(map))
            }
        }
        _ => Err(format!("Unsupported expression type: {:?}", expr)),
    }
}

fn parse_json_value(value: serde_json::Value) -> Result<BTreeMap<String, AssetValue>, String> {
    match value {
        serde_json::Value::Object(map) => {
            let mut result = BTreeMap::new();
            for (k, v) in map {
                result.insert(k, parse_json_value_to_asset(v)?);
            }
            Ok(result)
        }
        _ => Err("Expected object at root".to_string()),
    }
}

fn parse_json_value_to_asset(value: serde_json::Value) -> Result<AssetValue, String> {
    match value {
        serde_json::Value::String(s) => Ok(AssetValue::String(s)),
        serde_json::Value::Number(n) => Ok(AssetValue::Number(n.as_f64().unwrap_or(0.0))),
        serde_json::Value::Object(map) => {
            if map.contains_key("id") {
                Ok(AssetValue::Object(
                    serde_json::from_value(serde_json::Value::Object(map))
                        .map_err(|e| format!("Failed to parse AssetMeta: {}", e))?,
                ))
            } else {
                let mut result = BTreeMap::new();
                for (k, v) in map {
                    result.insert(k, parse_json_value_to_asset(v)?);
                }
                Ok(AssetValue::Table(result))
            }
        }
        _ => Err("Unsupported value type".to_string()),
    }
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

fn augment_node(
    node: AssetValue,
    assets: &BTreeMap<String, AssetValue>,
    path_segments: &[String],
    images_folder: &Path,
) -> AssetValue {
    let id_str = match &node {
        AssetValue::String(s) => Some(s.clone()),
        AssetValue::Number(n) => Some(n.to_string()),
        _ => None,
    };

    match node {
        AssetValue::String(_) | AssetValue::Number(_) => {
            let id_str = id_str.unwrap();

            let image_path = images_folder.join(path_segments.join("/"));
            let (width, height) = get_png_dimensions(&image_path).unwrap_or((0, 0));

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
            let image_path = images_folder.join(path_segments.join("/"));
            let (width, height) = get_png_dimensions(&image_path)
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
                let key_clone = key.clone();
                child_path.push(key_clone.clone());
                result.insert(
                    key_clone,
                    augment_node(map[&key].clone(), assets, &child_path, images_folder),
                );
            }

            AssetValue::Table(result)
        }
    }
}

fn serialize_luau(value: &AssetValue, indent: usize) -> String {
    let indent_str = "\t".repeat(indent);
    let inner_indent = format!("{}\t", indent_str);
    let first_level = indent == 0;

    match value {
        AssetValue::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")),
        AssetValue::Number(n) => n.to_string(),
        AssetValue::Object(meta) => {
            let mut parts = vec!["{".to_string()];
            parts.push(format!("{}id = \"{}\",", inner_indent, meta.id));
            if let Some(w) = meta.width {
                parts.push(format!("{}width = {},", inner_indent, w));
            }
            if let Some(h) = meta.height {
                parts.push(format!("{}height = {},", inner_indent, h));
            }
            if let Some(ref h_id) = meta.highlight_id {
                parts.push(format!("{}highlightId = \"{}\",", inner_indent, h_id));
            }
            parts.push(format!("{}}}", indent_str));
            let result = parts.join("\n");
            if first_level {
                format!("{}\n", result)
            } else {
                result
            }
        }
        AssetValue::Table(map) => {
            let mut parts = vec!["{".to_string()];
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();

            for key in keys {
                let key_str = if key.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && key
                        .chars()
                        .next()
                        .map(|c| c.is_alphabetic())
                        .unwrap_or(false)
                {
                    format!("{}{} = ", inner_indent, key)
                } else {
                    format!(
                        "{}[{}] = ",
                        inner_indent,
                        serde_json::to_string(&key).unwrap()
                    )
                };
                let value_str = serialize_luau(&map[&key], indent + 1);
                parts.push(format!("{}{},", key_str, value_str));
            }
            parts.push(format!("{}}}", indent_str));
            let result = parts.join("\n");
            if first_level {
                format!("{}\n", result)
            } else {
                result
            }
        }
    }
}

fn serialize_dts(value: &AssetValue, indent: usize) -> String {
    let indent_str = " ".repeat(indent);
    let inner_indent = format!("{}    ", indent_str);

    match value {
        AssetValue::String(_) | AssetValue::Number(_) => "AssetMeta;".to_string(),
        AssetValue::Object(_) => "AssetMeta;".to_string(),
        AssetValue::Table(map) => {
            let mut parts = vec!["{".to_string()];
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();

            for key in keys {
                let value = &map[&key];
                let key_str = if key.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && key
                        .chars()
                        .next()
                        .map(|c| c.is_alphabetic())
                        .unwrap_or(false)
                {
                    format!("{}{}: ", inner_indent, key)
                } else {
                    format!("{}{}: ", inner_indent, serde_json::to_string(&key).unwrap())
                };

                // Check if this is an AssetMeta object
                let value_str = match value {
                    AssetValue::Object(_) => "AssetMeta;".to_string(),
                    AssetValue::Table(_) => serialize_dts(value, indent + 4),
                    _ => "AssetMeta;".to_string(),
                };
                parts.push(format!("{}{}", key_str, value_str));
            }
            parts.push(format!("{}}}", indent_str));
            parts.join("\n")
        }
    }
}

fn sync_asphalt() -> Result<(), String> {
    let output = Command::new("asphalt")
        .arg("sync")
        .output()
        .map_err(|e| format!("Failed to run asphalt sync: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("asphalt sync failed: {}", stderr));
    }

    Ok(())
}

pub fn run(args: SyncArgs) -> bool {
    if let Err(e) = check_asphalt() {
        eprintln!("[sync] ERROR: {}", e);
        return false;
    }

    let api_key = match get_asphalt_api_key(args.asphalt_api_key) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("[sync] WARN: {}", e);
            return false;
        }
    };

    // Set the API key in environment for asphalt
    std::env::set_var("ASPHALT_API_KEY", &api_key);

    println!("[sync] Running asphalt sync …");
    if let Err(e) = sync_asphalt() {
        eprintln!("[sync] ERROR: {}", e);
        return false;
    }

    println!("[sync] Augmenting with image dimensions …");

    // Load assets - supports both JSON and Luau formats
    let assets = match load_assets(&args.assets_input) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[sync] ERROR: Failed to load assets: {}", e);
            return false;
        }
    };

    let mut augmented_assets = BTreeMap::new();
    for (category, node) in &assets {
        augmented_assets.insert(
            category.clone(),
            augment_node(
                node.clone(),
                &assets,
                &[category.clone()],
                &args.images_folder,
            ),
        );
    }

    println!("[sync] Writing augmented Luau module …");
    let luau_content = format!(
        "-- This file is automatically @generated by truffle.\n\
         -- DO NOT EDIT MANUALLY.\n\n\
         local assets = {}\n\
         return {{\n\
         \tassets = assets\n\
         }}\n",
        serialize_luau(&AssetValue::Table(augmented_assets.clone()), 0)
    );

    if let Err(e) = fs::write(&args.assets_output, luau_content) {
        eprintln!("[sync] ERROR: Failed to write Luau file: {}", e);
        return false;
    }

    println!("[sync] Writing TypeScript declaration …");
    let dts_content = format!(
        "// This file is automatically @generated by truffle.\n\
         // DO NOT EDIT MANUALLY.\n\n\
         export interface AssetMeta {{\n\
         \tid: string;\n\
         \twidth: number;\n\
         \theight: number;\n\
         \thighlightId?: string;\n\
         }}\n\n\
         declare const assets: {}\n\n\
         export {{ assets }};\n",
        serialize_dts(&AssetValue::Table(augmented_assets), 0)
    );

    if let Err(e) = fs::write(&args.dts_output, dts_content) {
        eprintln!("[sync] ERROR: Failed to write TypeScript file: {}", e);
        return false;
    }

    println!("[sync] Done ✅");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rain_entries() -> Vec<(&'static str, &'static str)> {
        vec![
            ("rain01.png", "rbxassetid://128376042917456"),
            ("rain02.png", "rbxassetid://102656535284897"),
            ("rain03.png", "rbxassetid://78303757743023"),
            ("rain04.png", "rbxassetid://76750515139891"),
            ("rain05.png", "rbxassetid://93643748181395"),
            ("rain06.png", "rbxassetid://96428226450022"),
            ("rain07.png", "rbxassetid://129889187543237"),
            ("rain08.png", "rbxassetid://120458672769343"),
            ("rain09.png", "rbxassetid://72971830980986"),
            ("rain10.png", "rbxassetid://123796605796568"),
        ]
    }

    fn sample_assets_table() -> BTreeMap<String, AssetValue> {
        fn meta(id: &str) -> AssetValue {
            AssetValue::Object(AssetMeta {
                id: id.to_string(),
                width: Some(1536),
                height: Some(864),
                highlight_id: None,
            })
        }

        let mut rain = BTreeMap::new();
        for (name, id) in sample_rain_entries() {
            rain.insert(name.to_string(), meta(id));
        }

        let mut ambience = BTreeMap::new();
        ambience.insert("rain".to_string(), AssetValue::Table(rain));

        let mut root = BTreeMap::new();
        root.insert("ambience".to_string(), AssetValue::Table(ambience));
        root
    }

    fn expected_luau_output() -> &'static str {
        r#"-- This file is automatically @generated by truffle.
-- DO NOT EDIT MANUALLY.

local assets = {
	ambience = {
		rain = {
			["rain01.png"] = {
				id = "rbxassetid://128376042917456",
				width = 1536,
				height = 864,
			},
			["rain02.png"] = {
				id = "rbxassetid://102656535284897",
				width = 1536,
				height = 864,
			},
			["rain03.png"] = {
				id = "rbxassetid://78303757743023",
				width = 1536,
				height = 864,
			},
			["rain04.png"] = {
				id = "rbxassetid://76750515139891",
				width = 1536,
				height = 864,
			},
			["rain05.png"] = {
				id = "rbxassetid://93643748181395",
				width = 1536,
				height = 864,
			},
			["rain06.png"] = {
				id = "rbxassetid://96428226450022",
				width = 1536,
				height = 864,
			},
			["rain07.png"] = {
				id = "rbxassetid://129889187543237",
				width = 1536,
				height = 864,
			},
			["rain08.png"] = {
				id = "rbxassetid://120458672769343",
				width = 1536,
				height = 864,
			},
			["rain09.png"] = {
				id = "rbxassetid://72971830980986",
				width = 1536,
				height = 864,
			},
			["rain10.png"] = {
				id = "rbxassetid://123796605796568",
				width = 1536,
				height = 864,
			},
		},
	},
}

return {
	assets = assets
}
"#
    }

    fn expected_dts_output() -> &'static str {
        r#"// This file is automatically @generated by truffle.
// DO NOT EDIT MANUALLY.

export interface AssetMeta {
	id: string;
	width: number;
	height: number;
	highlightId?: string;
}

declare const assets: {
    ambience: {
        rain: {
            "rain01.png": AssetMeta;
            "rain02.png": AssetMeta;
            "rain03.png": AssetMeta;
            "rain04.png": AssetMeta;
            "rain05.png": AssetMeta;
            "rain06.png": AssetMeta;
            "rain07.png": AssetMeta;
            "rain08.png": AssetMeta;
            "rain09.png": AssetMeta;
            "rain10.png": AssetMeta;
        }
    }
}

export { assets };
"#
    }

    #[test]
    fn test_parse_luau_direct_return() {
        let content = r#"
return {
    assets = {
        category1 = {
            item1 = "asset-id-1",
            item2 = 123
        }
    }
}
"#;
        let result = parse_luau_assets_module(content).unwrap();
        assert!(result.contains_key("category1"));
        if let AssetValue::Table(category) = &result["category1"] {
            assert!(category.contains_key("item1"));
            if let AssetValue::String(id) = &category["item1"] {
                assert_eq!(id, "asset-id-1");
            } else {
                panic!("Expected String value");
            }
            assert!(category.contains_key("item2"));
            if let AssetValue::Number(num) = &category["item2"] {
                assert_eq!(*num, 123.0);
            } else {
                panic!("Expected Number value");
            }
        } else {
            panic!("Expected Table value");
        }
    }

    #[test]
    fn test_parse_luau_local_variable() {
        let content = r#"
local assets = {
    category1 = {
        item1 = "asset-id-1"
    }
}
return {
    assets = assets
}
"#;
        let result = parse_luau_assets_module(content).unwrap();
        assert!(result.contains_key("category1"));
        if let AssetValue::Table(category) = &result["category1"] {
            assert!(category.contains_key("item1"));
            if let AssetValue::String(id) = &category["item1"] {
                assert_eq!(id, "asset-id-1");
            } else {
                panic!("Expected String value");
            }
        } else {
            panic!("Expected Table value");
        }
    }

    #[test]
    fn test_parse_luau_with_asset_meta() {
        let content = r#"
return {
    assets = {
        category1 = {
            item1 = {
                id = "asset-id-1",
                width = 100,
                height = 200
            }
        }
    }
}
"#;
        let result = parse_luau_assets_module(content).unwrap();
        if let AssetValue::Table(category) = &result["category1"] {
            if let AssetValue::Object(meta) = &category["item1"] {
                assert_eq!(meta.id, "asset-id-1");
                assert_eq!(meta.width, Some(100));
                assert_eq!(meta.height, Some(200));
            } else {
                panic!("Expected Object value");
            }
        } else {
            panic!("Expected Table value");
        }
    }

    #[test]
    fn test_parse_luau_nested_tables() {
        let content = r#"
return {
    assets = {
        level1 = {
            level2 = {
                level3 = "deep-value"
            }
        }
    }
}
"#;
        let result = parse_luau_assets_module(content).unwrap();
        if let AssetValue::Table(level1) = &result["level1"] {
            if let AssetValue::Table(level2) = &level1["level2"] {
                if let AssetValue::String(val) = &level2["level3"] {
                    assert_eq!(val, "deep-value");
                } else {
                    panic!("Expected String at level3");
                }
            } else {
                panic!("Expected Table at level2");
            }
        } else {
            panic!("Expected Table at level1");
        }
    }

    #[test]
    fn test_parse_luau_invalid_syntax() {
        let content = "this is not valid Luau code {";
        let result = parse_luau_assets_module(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_luau_no_assets() {
        let content = r#"
return {
    other = "value"
}
"#;
        let result = parse_luau_assets_module(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_serialize_luau_matches_sample_output() {
        let table = AssetValue::Table(sample_assets_table());
        let luau_output = format!(
            "-- This file is automatically @generated by truffle.\n\
             -- DO NOT EDIT MANUALLY.\n\n\
             local assets = {}\n\
             return {{\n\
             \tassets = assets\n\
             }}\n",
            serialize_luau(&table, 0)
        );
        assert_eq!(luau_output, expected_luau_output());
    }

    #[test]
    fn test_serialize_dts_matches_sample_output() {
        let table = AssetValue::Table(sample_assets_table());
        let dts_output = format!(
            "// This file is automatically @generated by truffle.\n\
             // DO NOT EDIT MANUALLY.\n\n\
             export interface AssetMeta {{\n\
             \tid: string;\n\
             \twidth: number;\n\
             \theight: number;\n\
             \thighlightId?: string;\n\
             }}\n\n\
             declare const assets: {}\n\n\
             export {{ assets }};\n",
            serialize_dts(&table, 0)
        );
        assert_eq!(dts_output, expected_dts_output());
    }
}
