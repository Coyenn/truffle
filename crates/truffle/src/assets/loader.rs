use super::model::{convert_map_to_asset_meta, AssetValue};
use anyhow::Context;
use full_moon::{ast, tokenizer::TokenType};
use serde_json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub fn load_assets(path: &Path) -> anyhow::Result<BTreeMap<String, AssetValue>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read assets file {}", path.display()))?;

    if path.extension().and_then(|s| s.to_str()) == Some("json") {
        let assets: BTreeMap<String, AssetValue> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON assets {}", path.display()))?;
        return Ok(assets);
    }

    parse_luau_assets_module(&content)
}

fn parse_luau_assets_module(content: &str) -> anyhow::Result<BTreeMap<String, AssetValue>> {
    let ast = full_moon::parse(content).map_err(|errors| {
        let details = errors
            .iter()
            .map(|e| format!("{:?}", e))
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::anyhow!("Failed to parse Luau: {}", details)
    })?;

    let block = ast.nodes();

    if let Some(table) = find_local_assets_table(block) {
        return convert_table_to_asset_value(table);
    }

    if let Some(table) = find_assets_table_in_return(block) {
        return convert_table_to_asset_value(table);
    }

    if let Some(table) = find_direct_return_table(block) {
        return convert_table_to_asset_value(table);
    }

    anyhow::bail!("Could not find assets table in Luau file")
}

fn find_direct_return_table(block: &ast::Block) -> Option<&ast::TableConstructor> {
    match block.last_stmt()? {
        ast::LastStmt::Return(ret) => {
            for expr in ret.returns().iter() {
                match expr {
                    ast::Expression::TableConstructor(table) => {
                        if looks_like_asset_table(table) {
                            return Some(table);
                        }
                    }
                    ast::Expression::Var(variable) => {
                        if let Some(table) = resolve_local_table(block, variable) {
                            if looks_like_asset_table(table) {
                                return Some(table);
                            }
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

fn looks_like_asset_table(table: &ast::TableConstructor) -> bool {
    // `clippy::manual_ok_err` is allowed below: spelling the key parse as
    // `.ok()` would trip the `no_discarded_error` gate, which this repo
    // also enforces.
    #[allow(clippy::manual_ok_err)]
    fn string_key(key: &ast::Expression) -> Option<String> {
        match extract_string_value(key) {
            Ok(key) => Some(key),
            // Skip keys with unparseable quotes; the table scan is best-effort.
            Err(_) => None,
        }
    }

    // Asphalt-generated Luau returns a table whose keys are file names.
    // We only accept this format if it contains at least one key that looks like an asset file.
    const EXTENSIONS: [&str; 5] = [".png", ".jpg", ".jpeg", ".webp", ".svg"];

    for field in table.fields() {
        let key = match field {
            ast::Field::NameKey { key, .. } => Some(key.to_string().trim().to_string()),
            ast::Field::ExpressionKey { key, .. } => match key {
                ast::Expression::String(_) => string_key(key),
                _ => None,
            },
            _ => None,
        };

        let Some(key) = key else {
            continue;
        };

        if key.contains('/') || EXTENSIONS.iter().any(|ext| key.ends_with(ext)) {
            return true;
        }
    }

    false
}

fn find_local_assets_table(block: &ast::Block) -> Option<&ast::TableConstructor> {
    find_local_table_named(block, "assets")
}

fn find_local_table_named<'a>(
    block: &'a ast::Block,
    name: &str,
) -> Option<&'a ast::TableConstructor> {
    for stmt in block.stmts() {
        if let ast::Stmt::LocalAssignment(local_assign) = stmt {
            for (n, expr) in local_assign
                .names()
                .iter()
                .zip(local_assign.expressions().iter())
            {
                if n.to_string().trim() == name {
                    if let ast::Expression::TableConstructor(table) = expr {
                        return Some(table);
                    }
                }
            }
        }
    }
    None
}

fn find_assets_table_in_return(block: &ast::Block) -> Option<&ast::TableConstructor> {
    match block.last_stmt()? {
        ast::LastStmt::Return(ret) => {
            for expr in ret.returns().iter() {
                match expr {
                    ast::Expression::TableConstructor(table) => {
                        if let Some(inner) = find_assets_table_in_table(block, table) {
                            return Some(inner);
                        }
                    }
                    ast::Expression::Var(variable) => {
                        if let Some(table) = resolve_assets_var(block, variable) {
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
                    ast::Expression::Var(variable) => resolve_assets_var(block, variable),
                    _ => None,
                };
            }
        }
    }
    None
}

fn resolve_assets_var<'a>(
    block: &'a ast::Block,
    variable: &'a ast::Var,
) -> Option<&'a ast::TableConstructor> {
    if let ast::Var::Name(name_ref) = variable {
        if name_ref.to_string().trim() == "assets" {
            return find_local_assets_table(block);
        }
    }
    None
}

fn resolve_local_table<'a>(
    block: &'a ast::Block,
    variable: &'a ast::Var,
) -> Option<&'a ast::TableConstructor> {
    if let ast::Var::Name(name_ref) = variable {
        let name = name_ref.to_string();
        return find_local_table_named(block, name.trim());
    }
    None
}

fn convert_table_to_asset_value(
    table: &ast::TableConstructor,
) -> anyhow::Result<BTreeMap<String, AssetValue>> {
    let mut result = BTreeMap::new();

    for field in table.fields() {
        let (key, value_expr) = match field {
            ast::Field::NameKey { key, value, .. } => (key.to_string().trim().to_string(), value),
            ast::Field::ExpressionKey { key, value, .. } => {
                let key_str = match key {
                    ast::Expression::String(_) => match extract_string_value(key) {
                        Ok(key_str) => key_str,
                        // Fall back to the raw token text when quote parsing fails.
                        Err(_) => key.to_string().trim().to_string(),
                    },
                    _ => key.to_string().trim().to_string(),
                };
                (key_str, value)
            }
            ast::Field::NoKey(_) => continue,
            _ => continue,
        };

        let asset_value = convert_expr_to_asset_value(value_expr)?;
        result.insert(key, asset_value);
    }

    Ok(result)
}

fn extract_string_value(expr: &ast::Expression) -> anyhow::Result<String> {
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

    anyhow::bail!("Expression is not a string literal")
}

fn extract_number_value(expr: &ast::Expression) -> anyhow::Result<f64> {
    if let ast::Expression::Number(token_ref) = expr {
        if let TokenType::Number { text } = token_ref.token().token_type() {
            let numeric_text = text.to_string();
            return numeric_text
                .parse::<f64>()
                .with_context(|| format!("Failed to parse number '{}'", numeric_text));
        }

        let fallback = token_ref.to_string();
        return fallback
            .trim()
            .parse::<f64>()
            .with_context(|| format!("Failed to parse number '{}'", fallback.trim()));
    }

    anyhow::bail!("Expression is not a numeric literal")
}

fn convert_expr_to_asset_value(expr: &ast::Expression) -> anyhow::Result<AssetValue> {
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
        _ => anyhow::bail!("Unsupported expression type: {:?}", expr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sample_luau(content: &str) -> BTreeMap<String, AssetValue> {
        parse_luau_assets_module(content).unwrap()
    }

    #[test]
    fn parse_luau_direct_return() {
        let assets = sample_luau(
            r#"
return {
    assets = {
        category1 = {
            item1 = "asset-id-1",
            item2 = 123
        }
    }
}
"#,
        );
        if let AssetValue::Table(category) = &assets["category1"] {
            assert_eq!(category["item1"], AssetValue::String("asset-id-1".into()));
        } else {
            panic!("Expected table for category1");
        }
    }

    #[test]
    fn parse_luau_local_variable() {
        let assets = sample_luau(
            r#"
local assets = {
    category1 = {
        item1 = "asset-id-1"
    }
}
return {
    assets = assets
}
"#,
        );
        if let AssetValue::Table(category) = &assets["category1"] {
            assert_eq!(category["item1"], AssetValue::String("asset-id-1".into()));
        } else {
            panic!("Expected table for category1");
        }
    }

    #[test]
    fn parse_luau_nested_tables() {
        let assets = sample_luau(
            r#"
return {
    assets = {
        level1 = {
            level2 = {
                level3 = "deep-value"
            }
        }
    }
}
"#,
        );
        if let AssetValue::Table(level1) = &assets["level1"] {
            if let AssetValue::Table(level2) = &level1["level2"] {
                assert_eq!(level2["level3"], AssetValue::String("deep-value".into()));
            } else {
                panic!("Expected table at level2");
            }
        } else {
            panic!("Expected table at level1");
        }
    }

    #[test]
    fn parse_luau_invalid() {
        let result = parse_luau_assets_module("return { other = \"value\" }");
        assert!(result.is_err());
    }

    #[test]
    fn parse_json_assets() {
        let assets: BTreeMap<String, AssetValue> =
            serde_json::from_str(r#"{ "category": { "item": "foo" } }"#).unwrap();
        if let AssetValue::Table(category) = &assets["category"] {
            assert_eq!(category["item"], AssetValue::String("foo".into()));
        } else {
            panic!("Expected table");
        }
    }

    #[test]
    fn parse_json_asset_meta() {
        let assets: BTreeMap<String, AssetValue> = serde_json::from_str(
            r#"{ "logo": { "id": "rbxassetid://1", "width": 16, "height": 16 } }"#,
        )
        .unwrap();
        assert_eq!(
            assets["logo"],
            AssetValue::Object(super::super::model::AssetMeta {
                id: "rbxassetid://1".into(),
                width: Some(16),
                height: Some(16),
                rect_x: None,
                rect_y: None,
                rect_w: None,
                rect_h: None,
                highlight_id: None,
                highlight_rect_x: None,
                highlight_rect_y: None,
                highlight_rect_w: None,
                highlight_rect_h: None,
            })
        );
    }
}
