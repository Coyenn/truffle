use super::model::{convert_map_to_asset_meta, AssetValue};
use full_moon::{ast, tokenizer::TokenType};
use serde_json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub fn load_assets(path: &Path) -> Result<BTreeMap<String, AssetValue>, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read assets file: {}", e))?;

    if path.extension().and_then(|s| s.to_str()) == Some("json") {
        let json_value: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse JSON: {}", e))?;
        return parse_json_value(json_value);
    }

    parse_luau_assets_module(&content)
}

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
            ast::Field::NameKey { key, value, .. } => (key.to_string().trim().to_string(), value),
            ast::Field::ExpressionKey { key, value, .. } => {
                let key_str = match key {
                    ast::Expression::String(_) => extract_string_value(key)
                        .unwrap_or_else(|_| key.to_string().trim().to_string()),
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
        let assets = parse_json_value(serde_json::json!({
            "category": {
                "item": "foo"
            }
        }))
        .unwrap();
        if let AssetValue::Table(category) = &assets["category"] {
            assert_eq!(category["item"], AssetValue::String("foo".into()));
        } else {
            panic!("Expected table");
        }
    }
}
