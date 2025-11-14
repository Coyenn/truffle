use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::fs;

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
    let output = Command::new("asphalt")
        .arg("--version")
        .output();

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
    let decoder = png::Decoder::new(
        match fs::File::open(png_path) {
            Ok(f) => f,
            Err(_) => return None,
        }
    );
    
    let reader = match decoder.read_info() {
        Ok(r) => r,
        Err(_) => return None,
    };

    let info = reader.info();
    Some((info.width, info.height))
}


fn load_assets(path: &Path) -> Result<BTreeMap<String, AssetValue>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read assets file: {}", e))?;
    
    // If file is JSON, parse as JSON
    if path.extension().and_then(|s| s.to_str()) == Some("json") {
        let json_value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;
        return parse_json_value(json_value);
    }
    
    // Otherwise try to parse as Luau
    // Extract the assets table from the Luau module
    // The file structure is: return { assets = { ... } }
    parse_luau_assets_module(&content)
}

fn parse_luau_assets_module(content: &str) -> Result<BTreeMap<String, AssetValue>, String> {
    // Find the assets table in the return statement
    // Pattern: return { assets = { ... } }
    let return_pattern = regex::Regex::new(r"return\s*\{\s*assets\s*=\s*(\{.*\})\s*\}").unwrap();
    
    if let Some(caps) = return_pattern.captures(content) {
        let table_str = &caps[1];
        return parse_luau_table_content(table_str);
    }
    
    // Also try: local assets = { ... }; return { assets = assets }
    let local_pattern = regex::Regex::new(r"local\s+assets\s*=\s*(\{.*?\})\s*return").unwrap();
    if let Some(caps) = local_pattern.captures(content) {
        let table_str = &caps[1];
        return parse_luau_table_content(table_str);
    }
    
    Err("Could not find assets table in Luau file".to_string())
}

fn parse_luau_table_content(content: &str) -> Result<BTreeMap<String, AssetValue>, String> {
    // This is a simplified parser - it handles basic Luau table syntax
    // For production use, consider a proper Luau parser library
    let mut result = BTreeMap::new();
    
    // Remove comments
    let mut cleaned = String::new();
    for line in content.lines() {
        if let Some(comment_pos) = line.find("--") {
            cleaned.push_str(&line[..comment_pos]);
        } else {
            cleaned.push_str(line);
        }
        cleaned.push(' ');
    }
    
    // Parse key-value pairs recursively
    // This is very basic - handles: key = "value", key = number, key = { ... }
    parse_luau_table_recursive(&cleaned, &mut 0, &mut result)?;
    
    Ok(result)
}

fn parse_luau_table_recursive(
    content: &str,
    pos: &mut usize,
    result: &mut BTreeMap<String, AssetValue>,
) -> Result<(), String> {
    // Skip whitespace
    while *pos < content.len() && content[*pos..].chars().next().map(|c| c.is_whitespace()).unwrap_or(false) {
        *pos += 1;
    }
    
    // Check for end of table
    if *pos >= content.len() || content[*pos..].starts_with('}') {
        return Ok(());
    }
    
    // Parse key
    let key_start = *pos;
    while *pos < content.len() && !content[*pos..].starts_with('=') && !content[*pos..].starts_with('}') {
        *pos += 1;
    }
    
    if *pos >= content.len() {
        return Ok(());
    }
    
    let key = content[key_start..*pos].trim().trim_matches('"').trim_matches('[').trim_matches(']').trim().to_string();
    
    // Skip to =
    while *pos < content.len() && content[*pos..].chars().next() != Some('=') {
        *pos += 1;
    }
    *pos += 1; // Skip =
    
    // Skip whitespace
    while *pos < content.len() && content[*pos..].chars().next().map(|c| c.is_whitespace()).unwrap_or(false) {
        *pos += 1;
    }
    
    // Parse value
    if content[*pos..].starts_with('{') {
        // Nested table
        *pos += 1;
        let mut nested = BTreeMap::new();
        parse_luau_table_recursive(content, pos, &mut nested)?;
        result.insert(key, AssetValue::Table(nested));
        // Skip closing }
        while *pos < content.len() && content[*pos..].chars().next() != Some('}') {
            *pos += 1;
        }
        if *pos < content.len() {
            *pos += 1;
        }
    } else if content[*pos..].starts_with('"') {
        // String value
        *pos += 1;
        let value_start = *pos;
        while *pos < content.len() && content[*pos..].chars().next() != Some('"') {
            *pos += 1;
        }
        let value = content[value_start..*pos].to_string();
        *pos += 1; // Skip closing "
        result.insert(key, AssetValue::String(value));
    } else {
        // Number value (simplified)
        let value_start = *pos;
        while *pos < content.len() && (content[*pos..].chars().next().map(|c| c.is_ascii_digit() || c == '.').unwrap_or(false)) {
            *pos += 1;
        }
        if value_start < *pos {
            let num_str = content[value_start..*pos].trim();
            if let Ok(num) = num_str.parse::<f64>() {
                result.insert(key, AssetValue::Number(num));
            }
        }
    }
    
    // Skip comma
    while *pos < content.len() && (content[*pos..].chars().next() == Some(',') || content[*pos..].chars().next().map(|c| c.is_whitespace()).unwrap_or(false)) {
        *pos += 1;
    }
    
    parse_luau_table_recursive(content, pos, result)
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
                Ok(AssetValue::Object(serde_json::from_value(serde_json::Value::Object(map))
                    .map_err(|e| format!("Failed to parse AssetMeta: {}", e))?))
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
            let (width, height) = get_png_dimensions(&image_path).unwrap_or((meta.width.unwrap_or(0), meta.height.unwrap_or(0)));
            
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
                result.insert(key_clone, augment_node(map[&key].clone(), assets, &child_path, images_folder));
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
                let key_str = if key.chars().all(|c| c.is_alphanumeric() || c == '_') && key.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) {
                    format!("{}{} = ", inner_indent, key)
                } else {
                    format!("{}[{}] = ", inner_indent, serde_json::to_string(&key).unwrap())
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
                let key_str = if key.chars().all(|c| c.is_alphanumeric() || c == '_') && key.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) {
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
            augment_node(node.clone(), &assets, &[category.clone()], &args.images_folder),
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

