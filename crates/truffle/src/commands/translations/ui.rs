use super::api::PatchEntry;
use super::lexicon::LoadedLexicon;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use indicatif_log_bridge::LogWrapper;
use log::LevelFilter;
use owo_colors::OwoColorize;
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::{Once, OnceLock};
use std::time::Duration;

const KEY_DISPLAY_LIMIT: usize = 8;

static INIT_LOGGER: Once = Once::new();
static UI_MULTI: OnceLock<MultiProgress> = OnceLock::new();

pub struct PushUi {
    multi: MultiProgress,
    tty: bool,
}

pub struct DiffSummary {
    pub unchanged: usize,
    pub update: Vec<String>,
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

pub struct DedupeSummary {
    pub duplicate_keys: usize,
    pub rows_to_remove: usize,
    pub keys: Vec<(String, usize)>,
}

pub struct VerifyResult {
    pub missing_count: usize,
    pub leftover_keys: usize,
}

#[derive(Copy, Clone)]
pub enum PushPhase {
    Delete,
    Create,
}

pub struct PushProgress<'a> {
    ui: &'a PushUi,
    bar: ProgressBar,
}

impl PushUi {
    pub fn new() -> anyhow::Result<Self> {
        let tty = io::stderr().is_terminal();
        let multi = UI_MULTI.get_or_init(MultiProgress::new).clone();

        INIT_LOGGER.call_once(|| {
            let logger =
                env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
                    .filter_level(LevelFilter::Info)
                    .format_timestamp(None)
                    .format_module_path(false)
                    .format_target(false)
                    .format(|buf, record| {
                        let level_style = buf.default_level_style(record.level());
                        writeln!(
                            buf,
                            "[{level_style}{}{level_style:#}] {}",
                            record.level(),
                            record.args()
                        )
                    })
                    .build();

            let level = logger.filter();
            let log_multi = UI_MULTI.get().expect("UI_MULTI initialized").clone();
            let _ = LogWrapper::new(log_multi, logger).try_init();
            log::set_max_level(level);
        });

        Ok(Self { multi, tty })
    }

    pub fn print_header(
        &self,
        game_id: u64,
        lexicon_path: Option<&Path>,
        lexicon: Option<&LoadedLexicon>,
        dry_run: bool,
        dedupe: bool,
    ) {
        self.suspend(|| {
            println!("{}", "translations push".bold());
            println!("  game     {game_id}");
            if let Some(path) = lexicon_path {
                println!("  lexicon  {}", path.display());
            }
            if let Some(loaded) = lexicon {
                println!(
                    "  locale   {} ({} keys)",
                    loaded.locale,
                    loaded.entries.len()
                );
            }
            let mut modes = Vec::new();
            if dry_run {
                modes.push("dry-run");
            }
            if dedupe {
                modes.push("dedupe");
            }
            if !modes.is_empty() {
                println!("  mode     {}", modes.join(" · "));
            }
            println!();
        });
    }

    pub fn with_fetch_spinner<T, F: FnOnce() -> anyhow::Result<(T, usize)>>(
        &self,
        f: F,
    ) -> anyhow::Result<(T, usize)> {
        if !self.tty {
            return f();
        }

        let bar = self.multi.add(ProgressBar::new_spinner());
        bar.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        bar.set_message("Fetching remote entries …");
        bar.enable_steady_tick(Duration::from_millis(80));

        let result = f();
        match &result {
            Ok((_, count)) => {
                bar.finish_with_message(format!("✓ Remote entries: {count}"));
            }
            Err(_) => {
                bar.abandon_with_message("✗ Fetch failed");
            }
        }
        self.multi.remove(&bar);
        result
    }

    pub fn print_diff(&self, summary: &DiffSummary) {
        self.suspend(|| {
            println!("{}", "Changes".bold());
            println!("  unchanged   {}", summary.unchanged);

            print_key_section("update", '~', "yellow", &summary.update);
            print_key_section("add", '+', "green", &summary.add);
            print_key_section("remove", '-', "red", &summary.remove);
            println!();
        });
    }

    pub fn print_dedupe(&self, summary: &DedupeSummary) {
        self.suspend(|| {
            println!("{}", "Dedupe".bold());
            println!("  duplicate keys   {}", summary.duplicate_keys);
            println!("  rows to remove     {}", summary.rows_to_remove);

            let mut shown = 0usize;
            for (key, extra) in &summary.keys {
                if shown >= KEY_DISPLAY_LIMIT {
                    let remaining = summary.keys.len() - shown;
                    println!("  … and {remaining} more duplicate keys");
                    break;
                }
                println!("    {}", format!("- {key} (×{extra})").red());
                shown += 1;
            }
            println!();
        });
    }

    pub fn print_dedupe_empty(&self) {
        self.suspend(|| {
            println!("{}", "✓ No duplicate keys found".green());
            println!();
        });
    }

    pub fn begin_push(&self, phase: PushPhase, total: usize) -> Option<PushProgress<'_>> {
        if total == 0 {
            return None;
        }

        if !self.tty {
            return None;
        }

        let label = match phase {
            PushPhase::Delete => "Deleting",
            PushPhase::Create => "Creating",
        };

        let bar = self.multi.add(ProgressBar::new(total as u64));
        bar.set_style(
            ProgressStyle::with_template("{prefix:.bold} [{bar:30.cyan/blue}] {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );
        bar.set_prefix(label);
        bar.set_message("");

        Some(PushProgress { ui: self, bar })
    }

    pub fn finish_push_phase(&self, phase: PushPhase, total: usize) {
        if total == 0 || !self.tty {
            return;
        }
        let message = match phase {
            PushPhase::Delete => format!("✓ Deleted {total} entries"),
            PushPhase::Create => format!("✓ Created {total} entries"),
        };
        self.suspend(|| println!("{}", message.green()));
    }

    pub fn print_dry_run_result(&self, create: usize, delete: usize) {
        self.suspend(|| {
            println!(
                "{}",
                format!("✓ Dry run · would create {create}, delete {delete} (no changes pushed)")
                    .green()
            );
            println!();
        });
    }

    pub fn print_push_result(&self, modified_count: usize, verify: Option<VerifyResult>) {
        self.suspend(|| {
            println!(
                "{}",
                format!("✓ Push complete · {modified_count} entries modified").green()
            );
            if let Some(result) = verify {
                if result.missing_count > 0 {
                    println!(
                        "{}",
                        format!(
                            "⚠ {} lexicon keys still mismatched on remote",
                            result.missing_count
                        )
                        .yellow()
                    );
                }
                if result.leftover_keys > 0 {
                    println!(
                        "{}",
                        format!(
                            "⚠ {} remote keys not in lexicon (leftovers)",
                            result.leftover_keys
                        )
                        .yellow()
                    );
                }
            }
            println!();
        });
    }

    fn suspend<F: FnOnce()>(&self, f: F) {
        if self.tty {
            self.multi.suspend(f);
        } else {
            f();
        }
    }
}

impl PushProgress<'_> {
    pub fn set(&self, done: usize, msg: &str) {
        self.bar.set_position(done as u64);
        self.bar.set_message(msg.to_string());
    }

    pub fn finish(self, message: &str) {
        self.bar.finish_with_message(message.to_string());
        self.ui.multi.remove(&self.bar);
    }
}

impl DiffSummary {
    pub fn from_patch_entries(
        skipped: usize,
        deletes: &[PatchEntry],
        creates: &[PatchEntry],
    ) -> Self {
        let delete_keys: HashSet<String> = deletes
            .iter()
            .map(|entry| entry.identifier.key.clone())
            .collect();
        let create_keys: HashSet<String> = creates
            .iter()
            .map(|entry| entry.identifier.key.clone())
            .collect();

        let mut update = Vec::new();
        let mut add = Vec::new();
        let mut remove = Vec::new();

        for key in &delete_keys {
            if create_keys.contains(key) {
                update.push(key.clone());
            } else {
                remove.push(key.clone());
            }
        }
        for key in &create_keys {
            if !delete_keys.contains(key) {
                add.push(key.clone());
            }
        }

        update.sort();
        add.sort();
        remove.sort();

        Self {
            unchanged: skipped,
            update,
            add,
            remove,
        }
    }
}

impl DedupeSummary {
    pub fn from_remote_entries(entries: &[super::api::RemoteEntry]) -> Self {
        let mut entries_by_key: HashMap<String, usize> = HashMap::new();
        for entry in entries {
            *entries_by_key
                .entry(entry.identifier.key.clone())
                .or_default() += 1;
        }

        let mut keys: Vec<(String, usize)> = entries_by_key
            .into_iter()
            .filter_map(|(key, count)| {
                if count > 1 {
                    Some((key, count - 1))
                } else {
                    None
                }
            })
            .collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0));

        let duplicate_keys = keys.len();
        let rows_to_remove: usize = keys.iter().map(|(_, extra)| *extra).sum();

        Self {
            duplicate_keys,
            rows_to_remove,
            keys,
        }
    }
}

fn print_key_section(label: &str, marker: char, color: &str, keys: &[String]) {
    if keys.is_empty() {
        return;
    }

    println!("  {label:<11} {}", keys.len());
    let shown = keys.len().min(KEY_DISPLAY_LIMIT);
    for key in &keys[..shown] {
        let line = format!("    {marker} {key}");
        let formatted = match color {
            "green" => line.green().to_string(),
            "red" => line.red().to_string(),
            "yellow" => line.yellow().to_string(),
            _ => line,
        };
        println!("{formatted}");
    }
    if keys.len() > KEY_DISPLAY_LIMIT {
        let remaining = keys.len() - KEY_DISPLAY_LIMIT;
        println!("  … and {remaining} more {label}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::translations::api::{EntryIdentifier, PatchEntry};

    fn patch(key: &str, delete: bool) -> PatchEntry {
        PatchEntry {
            identifier: EntryIdentifier {
                key: key.into(),
                source: key.into(),
                context: key.into(),
            },
            translations: None,
            metadata: None,
            delete: if delete { Some(true) } else { None },
        }
    }

    #[test]
    fn diff_summary_categorizes_update_add_remove() {
        let summary = DiffSummary::from_patch_entries(
            2,
            &[patch("hello", true), patch("gone", true)],
            &[patch("hello", false), patch("new", false)],
        );

        assert_eq!(summary.unchanged, 2);
        assert_eq!(summary.update, vec!["hello".to_string()]);
        assert_eq!(summary.add, vec!["new".to_string()]);
        assert_eq!(summary.remove, vec!["gone".to_string()]);
    }

    #[test]
    fn diff_summary_add_only() {
        let summary =
            DiffSummary::from_patch_entries(0, &[], &[patch("a", false), patch("b", false)]);

        assert!(summary.update.is_empty());
        assert!(summary.remove.is_empty());
        assert_eq!(summary.add.len(), 2);
    }

    #[test]
    fn dedupe_summary_counts_duplicate_rows() {
        use crate::commands::translations::api::RemoteEntry;

        let entries = vec![
            RemoteEntry {
                identifier: EntryIdentifier {
                    key: "a".into(),
                    source: "1".into(),
                    context: "a".into(),
                },
                translations: None,
                metadata: None,
            },
            RemoteEntry {
                identifier: EntryIdentifier {
                    key: "a".into(),
                    source: "2".into(),
                    context: "a".into(),
                },
                translations: None,
                metadata: None,
            },
            RemoteEntry {
                identifier: EntryIdentifier {
                    key: "b".into(),
                    source: "1".into(),
                    context: "b".into(),
                },
                translations: None,
                metadata: None,
            },
            RemoteEntry {
                identifier: EntryIdentifier {
                    key: "b".into(),
                    source: "2".into(),
                    context: "b".into(),
                },
                translations: None,
                metadata: None,
            },
            RemoteEntry {
                identifier: EntryIdentifier {
                    key: "b".into(),
                    source: "3".into(),
                    context: "b".into(),
                },
                translations: None,
                metadata: None,
            },
        ];

        let summary = DedupeSummary::from_remote_entries(&entries);
        assert_eq!(summary.duplicate_keys, 2);
        assert_eq!(summary.rows_to_remove, 3);
    }
}
