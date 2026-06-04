use anyhow::{Context, Result};
use mlua::{Function, Lua, Table, UserData, UserDataFields, Value};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexiconEntry {
    pub source: String,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedLexicon {
    pub locale: String,
    pub entries: HashMap<String, LexiconEntry>,
}

struct LexiContext {
    source: String,
    context: String,
}

impl UserData for LexiContext {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("source", |_, this| Ok(this.source.clone()));
        fields.add_field_method_get("context", |_, this| Ok(this.context.clone()));
    }
}

fn lua_err(error: mlua::Error, context: &str) -> anyhow::Error {
    anyhow::anyhow!("{context}: {error}")
}

fn make_fake_instance(lua: &Lua) -> mlua::Result<Table> {
    let instance = lua.create_table()?;
    let mt = lua.create_table()?;

    let index_target = instance.clone();
    mt.raw_set(
        "__index",
        lua.create_function(move |_, ()| Ok(index_target.clone()))?,
    )?;

    let call_target = instance.clone();
    mt.raw_set(
        "__call",
        lua.create_function(move |_, ()| Ok(call_target.clone()))?,
    )?;

    instance.set_metatable(Some(mt));
    Ok(instance)
}

fn make_lexi_package(lua: &Lua) -> mlua::Result<(Table, Function)> {
    let pkg = lua.create_table()?;

    pkg.set(
        "lexicon",
        lua.create_function(|lua, (locale, entries): (String, Table)| {
            let result = lua.create_table()?;
            result.set("locale", locale)?;
            result.set("entries", entries)?;
            Ok(result)
        })?,
    )?;

    pkg.set(
        "context",
        lua.create_function(|lua, source: String| {
            lua.create_function(move |_, context: String| {
                Ok(LexiContext {
                    source: source.clone(),
                    context,
                })
            })
        })?,
    )?;

    let pkg_for_require = pkg.clone();
    let require_fn = lua.create_function(move |_, (): ()| Ok(pkg_for_require.clone()))?;

    pkg.set("import", require_fn.clone())?;

    Ok((pkg, require_fn))
}

fn flatten_entries(
    entries: &Table,
    prefix: &str,
    out: &mut HashMap<String, LexiconEntry>,
) -> mlua::Result<()> {
    for pair in entries.pairs::<Value, Value>() {
        let (key_val, entry) = pair?;
        let key_str = match key_val {
            Value::String(s) => s.to_str()?.to_string(),
            _ => continue,
        };
        let full_key = if prefix.is_empty() {
            key_str
        } else {
            format!("{prefix}.{key_str}")
        };

        match entry {
            Value::UserData(ud) => {
                if let Ok(ctx) = ud.borrow::<LexiContext>() {
                    out.insert(
                        full_key,
                        LexiconEntry {
                            source: ctx.source.clone(),
                            context: ctx.context.clone(),
                        },
                    );
                }
            }
            Value::Table(table) => {
                flatten_entries(&table, &full_key, out)?;
            }
            Value::String(s) => {
                out.insert(
                    full_key,
                    LexiconEntry {
                        source: s.to_str()?.to_string(),
                        context: String::new(),
                    },
                );
            }
            _ => {}
        }
    }

    Ok(())
}

pub fn load_lexicon(path: &Path) -> Result<LoadedLexicon> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read lexicon file {}", path.display()))?;
    load_lexicon_from_source(&contents)
}

pub fn load_lexicon_from_source(contents: &str) -> Result<LoadedLexicon> {
    let lua = Lua::new();
    let (_, require_fn) =
        make_lexi_package(&lua).map_err(|e| lua_err(e, "failed to create lexi mock"))?;
    let fake_instance =
        make_fake_instance(&lua).map_err(|e| lua_err(e, "failed to create fake instance"))?;

    let env = lua
        .create_table()
        .map_err(|e| lua_err(e, "failed to create environment"))?;
    env.set("require", require_fn)
        .map_err(|e| lua_err(e, "failed to set require"))?;
    env.set("script", fake_instance.clone())
        .map_err(|e| lua_err(e, "failed to set script"))?;
    env.set("game", fake_instance)
        .map_err(|e| lua_err(e, "failed to set game"))?;

    let result: Table = lua
        .load(contents)
        .set_environment(env)
        .eval()
        .map_err(|e| lua_err(e, "failed to execute lexicon module"))?;

    let locale: String = result
        .get("locale")
        .map_err(|e| lua_err(e, "lexicon missing locale"))?;
    let entries_table: Table = result
        .get("entries")
        .map_err(|e| lua_err(e, "lexicon missing entries"))?;

    let mut entries = HashMap::new();
    flatten_entries(&entries_table, "", &mut entries)
        .map_err(|e| lua_err(e, "failed to flatten lexicon entries"))?;

    Ok(LoadedLexicon { locale, entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_sample_lexicon() {
        let source = r#"
return (function()
    local lexi = require(game)
    local lexicon = lexi.lexicon
    local context = lexi.context
    return lexicon("en-us", {
        hello = 'Hello there',
        pets = {
            hamster = context('This is my ugly old hamster')('The speaker loves the hamster.'),
        },
        goodbye_player = 'Goodbye {player}, it is sad to leave you at {when:datetime}',
    })
end)()
"#;

        let loaded = load_lexicon_from_source(source).unwrap();
        assert_eq!(loaded.locale, "en-us");
        assert_eq!(
            loaded.entries.get("hello"),
            Some(&LexiconEntry {
                source: "Hello there".into(),
                context: String::new(),
            })
        );
        assert_eq!(
            loaded.entries.get("pets.hamster"),
            Some(&LexiconEntry {
                source: "This is my ugly old hamster".into(),
                context: "The speaker loves the hamster.".into(),
            })
        );
        assert_eq!(
            loaded
                .entries
                .get("goodbye_player")
                .map(|e| e.source.as_str()),
            Some("Goodbye {player}, it is sad to leave you at {when:datetime}")
        );
    }
}
