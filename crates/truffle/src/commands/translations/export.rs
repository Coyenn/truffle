use super::lexicon;
use super::push::cloud_context;
use anyhow::{Context, Result};
use clap::Parser;
use log::{error, info, warn};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

const MAX_ENTRIES_PER_UPLOAD: usize = 1000;

#[derive(Parser)]
#[command(about = "Export a Lexi lexicon to a Roblox localization CSV for manual upload")]
pub struct ExportArgs {
    /// Path to lexicon .luau file
    pub lexicon_path: PathBuf,

    /// Output CSV path (defaults to the lexicon path with a .csv extension; use - for stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

pub fn run(args: ExportArgs) -> bool {
    match run_impl(args) {
        Ok(()) => true,
        Err(error) => {
            error!("{error:#}");
            false
        }
    }
}

fn run_impl(args: ExportArgs) -> Result<()> {
    let loaded = lexicon::load_lexicon(&args.lexicon_path)?;

    let mut entries: Vec<_> = loaded.entries.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    let mut csv = String::from("Key,Source,Context,Example\r\n");
    for &(key, entry) in &entries {
        let context = cloud_context(key, &entry.context);
        csv.push_str(&csv_record(&[key, &entry.source, &context, ""]));
    }

    let output = args
        .output
        .unwrap_or_else(|| args.lexicon_path.with_extension("csv"));

    if output.as_os_str() == "-" {
        io::stdout()
            .write_all(csv.as_bytes())
            .context("failed to write CSV to stdout")?;
    } else {
        fs::write(&output, csv.as_bytes())
            .with_context(|| format!("failed to write CSV to {}", output.display()))?;
        info!("Wrote {} entries to {}", entries.len(), output.display());
        info!(
            "Manual upload: Creator Hub -> Localization -> Delete Table, then Upload CSV (upload alone never removes entries)"
        );
    }

    if entries.len() > MAX_ENTRIES_PER_UPLOAD {
        warn!(
            "{} entries exceeds Roblox's {MAX_ENTRIES_PER_UPLOAD} per-upload limit; split the file before uploading",
            entries.len()
        );
    }

    Ok(())
}

fn csv_record(fields: &[&str]) -> String {
    let mut record = fields
        .iter()
        .map(|field| csv_field(field))
        .collect::<Vec<_>>()
        .join(",");
    record.push_str("\r\n");
    record
}

fn csv_field(value: &str) -> String {
    if value.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_field_passes_plain_text_through() {
        assert_eq!(csv_field("plain text"), "plain text");
    }

    #[test]
    fn csv_field_quotes_and_escapes_specials() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn csv_record_joins_with_commas_and_crlf() {
        assert_eq!(csv_record(&["k", "Hello", "k", ""]), "k,Hello,k,\r\n");
    }
}
