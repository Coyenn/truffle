use std::fs;
use std::path::Path;

use anyhow::Context;

struct RuntimeFile {
    name: &'static str,
    contents: &'static str,
}

const RUNTIME_FILES: &[RuntimeFile] = &[
    RuntimeFile {
        name: "utf8_util.luau",
        contents: include_str!("../../../runtime/truffle-text/utf8_util.luau"),
    },
    RuntimeFile {
        name: "main.luau",
        contents: include_str!("../../../runtime/truffle-text/main.luau"),
    },
    RuntimeFile {
        name: "font.luau",
        contents: include_str!("../../../runtime/truffle-text/font.luau"),
    },
    RuntimeFile {
        name: "layout.luau",
        contents: include_str!("../../../runtime/truffle-text/layout.luau"),
    },
    RuntimeFile {
        name: "rich_text.luau",
        contents: include_str!("../../../runtime/truffle-text/rich_text.luau"),
    },
    RuntimeFile {
        name: "types.d.ts",
        contents: include_str!("../../../runtime/truffle-text/types.d.ts"),
    },
];

pub fn copy_runtime(out_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(out_dir).with_context(|| {
        format!(
            "failed to create truffle-text output dir: {}",
            out_dir.display()
        )
    })?;

    for file in RUNTIME_FILES {
        let path = out_dir.join(file.name);
        if path.exists() {
            let existing = fs::read_to_string(&path).unwrap_or_default();
            if existing == file.contents {
                continue;
            }
        }
        fs::write(&path, file.contents).with_context(|| {
            format!("failed to write runtime file: {}", path.display())
        })?;
    }

    println!(
        "[font] Wrote truffle-text runtime to {}",
        out_dir.display()
    );
    Ok(())
}
