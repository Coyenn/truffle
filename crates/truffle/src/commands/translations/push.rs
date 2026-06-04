use super::api::{self, EntryOperationLimits, LocalizationClient, PatchEntry, RemoteEntry};
use super::lexicon::{LexiconEntry, LoadedLexicon};
use super::ui::{DedupeSummary, DiffSummary, PushPhase, PushUi, VerifyResult};
use clap::Parser;
use dotenvy::dotenv;
use log::{error, warn};
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Parser)]
#[command(about = "Push a Lexi lexicon to Roblox cloud localization")]
pub struct PushArgs {
    /// Preview changes without pushing to Roblox
    #[arg(long)]
    pub dry_run: bool,

    /// Remove duplicate remote keys (lexicon path optional)
    #[arg(long)]
    pub dedupe: bool,

    /// Roblox universe/game ID (defaults to MANTLE_UNIVERSE_ID)
    pub game_id: Option<u64>,

    /// Path to lexicon .luau file
    pub lexicon_path: Option<PathBuf>,
}

pub fn run(args: PushArgs) -> bool {
    let _ = dotenv();

    match run_impl(args) {
        Ok(()) => true,
        Err(error) => {
            error!("{error:#}");
            false
        }
    }
}

fn run_impl(args: PushArgs) -> anyhow::Result<()> {
    let api_key = env::var("LEXI_AUTH_TOKEN").ok();
    let game_id = args.game_id.or_else(|| {
        env::var("MANTLE_UNIVERSE_ID")
            .ok()
            .and_then(|v| v.parse().ok())
    });

    let lexicon_loaded = if let Some(path) = &args.lexicon_path {
        Some(super::lexicon::load_lexicon(path)?)
    } else {
        None
    };

    if game_id.is_none() || api_key.is_none() {
        anyhow::bail!(
            "missing game id or LEXI_AUTH_TOKEN\n\
             set MANTLE_UNIVERSE_ID when game id is omitted"
        );
    }

    if !args.dedupe && lexicon_loaded.is_none() {
        anyhow::bail!("lexicon file required unless --dedupe is set");
    }

    let game_id = game_id.unwrap();
    let api_key = api_key.unwrap();

    let ui = PushUi::new()?;
    ui.print_header(
        game_id,
        args.lexicon_path.as_deref(),
        lexicon_loaded.as_ref(),
        args.dry_run,
        args.dedupe,
    );

    let client = LocalizationClient::new(api_key, game_id)?;
    let (remote_entries, remote_entry_count) =
        ui.with_fetch_spinner(|| client.fetch_remote_entries())?;
    let remote_before = remote_entry_count;

    let mut delete_entries: Vec<PatchEntry> = Vec::new();
    let mut create_entries: Vec<PatchEntry> = Vec::new();
    let mut skipped_count = 0usize;
    let mut lexicon_count = 0usize;
    let mut lexicon_required: Option<HashMap<String, LexiconEntry>> = None;

    if let Some(loaded) = lexicon_loaded {
        let diff = build_lexicon_diff(&loaded, &remote_entries);
        lexicon_count = diff.lexicon_count;
        skipped_count = diff.skipped_count;
        delete_entries = diff.delete_entries;
        create_entries = diff.create_entries;
        lexicon_required = Some(diff.lexicon_required);
    }

    if args.dedupe {
        let dedupe_summary = DedupeSummary::from_remote_entries(&remote_entries);
        ui.print_dedupe(&dedupe_summary);

        if dedupe_summary.rows_to_remove == 0 {
            ui.print_dedupe_empty();
            return Ok(());
        }

        let (dedupe_deletes, _) = build_dedupe_deletes(&remote_entries);
        if args.dry_run {
            ui.print_dry_run_result(0, dedupe_deletes.len());
            return Ok(());
        }

        delete_entries = dedupe_deletes;
        create_entries.clear();
    } else {
        ui.print_diff(&DiffSummary::from_patch_entries(
            skipped_count,
            &delete_entries,
            &create_entries,
        ));

        if args.dry_run {
            ui.print_dry_run_result(create_entries.len(), delete_entries.len());
            return Ok(());
        }
    }

    let (entry_limits, max_entries_per_update) = client.fetch_limits();
    let max_create_entries_per_update = client.max_create_entries_per_update();
    warn_if_context_too_long(&create_entries, &entry_limits);

    let create_entries = partition_create_batches(&create_entries, &entry_limits);
    let mut failed_keys: HashSet<String> = HashSet::new();
    let mut modified_count = 0usize;

    do_batched_requests(
        &ui,
        &client,
        &delete_entries,
        PushPhase::Delete,
        false,
        max_entries_per_update,
        max_create_entries_per_update,
        &mut PushState {
            failed_keys: &mut failed_keys,
            modified_count: &mut modified_count,
        },
    )?;
    do_batched_requests(
        &ui,
        &client,
        &create_entries,
        PushPhase::Create,
        true,
        max_entries_per_update,
        max_create_entries_per_update,
        &mut PushState {
            failed_keys: &mut failed_keys,
            modified_count: &mut modified_count,
        },
    )?;

    if !failed_keys.is_empty() {
        anyhow::bail!("failed to push {} keys", failed_keys.len());
    }

    let verify = if lexicon_required.is_some() {
        let (remote_after_entries, remote_after) =
            ui.with_fetch_spinner(|| client.fetch_remote_entries())?;
        Some(verify_push(
            lexicon_required.as_ref().unwrap(),
            &remote_after_entries,
            remote_after,
            remote_before,
            lexicon_count,
            modified_count,
        ))
    } else {
        None
    };

    ui.print_push_result(modified_count, verify);
    Ok(())
}

struct LexiconDiff {
    lexicon_count: usize,
    skipped_count: usize,
    delete_entries: Vec<PatchEntry>,
    create_entries: Vec<PatchEntry>,
    lexicon_required: HashMap<String, LexiconEntry>,
}

fn build_lexicon_diff(loaded: &LoadedLexicon, remote_entries: &[RemoteEntry]) -> LexiconDiff {
    let mut lexicon_entries = loaded.entries.clone();
    let lexicon_count = lexicon_entries.len();
    let lexicon_required = lexicon_entries.clone();

    let mut delete_entries = Vec::new();
    let mut create_entries = Vec::new();
    let mut skipped_count = 0usize;

    for entry in remote_entries {
        let key = entry.identifier.key.clone();
        let lexicon_value = lexicon_entries.remove(&key);
        if lexicon_value
            .as_ref()
            .is_some_and(|value| lexicon_matches_remote(&key, value, &entry.identifier))
        {
            skipped_count += 1;
            continue;
        }

        delete_entries.push(PatchEntry {
            identifier: entry.identifier.clone(),
            translations: entry.translations.clone(),
            metadata: entry.metadata.clone(),
            delete: Some(true),
        });

        if let Some(value) = lexicon_value {
            create_entries.push(make_create_entry(
                &key,
                &value,
                entry.translations.clone(),
                entry.metadata.clone(),
            ));
        }
    }

    for (key, value) in lexicon_entries {
        create_entries.push(make_create_entry(&key, &value, None, None));
    }

    LexiconDiff {
        lexicon_count,
        skipped_count,
        delete_entries,
        create_entries,
        lexicon_required,
    }
}

fn build_dedupe_deletes(remote_entries: &[RemoteEntry]) -> (Vec<PatchEntry>, usize) {
    let mut entries_by_key: HashMap<String, Vec<&RemoteEntry>> = HashMap::new();
    for entry in remote_entries {
        entries_by_key
            .entry(entry.identifier.key.clone())
            .or_default()
            .push(entry);
    }

    let mut dedupe_deletes = Vec::new();
    let mut dedupe_count = 0usize;

    for (_key, entries) in entries_by_key {
        if entries.len() > 1 {
            for entry in entries.iter().skip(1) {
                dedupe_deletes.push(PatchEntry {
                    identifier: entry.identifier.clone(),
                    translations: entry.translations.clone(),
                    metadata: entry.metadata.clone(),
                    delete: Some(true),
                });
                dedupe_count += 1;
            }
        }
    }

    (dedupe_deletes, dedupe_count)
}

pub fn cloud_context(key: &str, context: &str) -> String {
    if context.is_empty() {
        key.to_string()
    } else {
        format!("{context}\0{key}")
    }
}

pub fn lexicon_matches_remote(
    key: &str,
    lexicon_value: &LexiconEntry,
    remote_identifier: &api::EntryIdentifier,
) -> bool {
    lexicon_value.source == remote_identifier.source
        && cloud_context(key, &lexicon_value.context) == remote_identifier.context
}

fn make_create_entry(
    key: &str,
    value: &LexiconEntry,
    translations: Option<serde_json::Value>,
    metadata: Option<serde_json::Value>,
) -> PatchEntry {
    PatchEntry {
        identifier: api::EntryIdentifier {
            key: key.to_string(),
            context: cloud_context(key, &value.context),
            source: value.source.clone(),
        },
        translations,
        metadata,
        delete: None,
    }
}

fn push_entries(
    client: &LocalizationClient,
    entries: &[PatchEntry],
    failed_keys: &mut HashSet<String>,
    modified_count: &mut usize,
) -> anyhow::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let body = client.patch_entries(entries)?;
    count_modified(entries, &body, modified_count);

    if body.failed_entries_and_translations.is_empty() {
        return Ok(());
    }

    if entries.len() == 1 {
        let reason = failure_reason(&body.failed_entries_and_translations[0]);
        mark_failed(&entries[0].identifier.key, failed_keys, reason);
        return Ok(());
    }

    let batch_failed_keys: HashSet<String> = body
        .failed_entries_and_translations
        .iter()
        .filter_map(|failure| failure.identifier.as_ref().map(|id| id.key.clone()))
        .collect();

    if batch_failed_keys.is_empty() {
        for entry in entries {
            push_entries(
                client,
                std::slice::from_ref(entry),
                failed_keys,
                modified_count,
            )?;
        }
        return Ok(());
    }

    for entry in entries {
        if batch_failed_keys.contains(&entry.identifier.key) {
            push_entries(
                client,
                std::slice::from_ref(entry),
                failed_keys,
                modified_count,
            )?;
        }
    }

    Ok(())
}

fn count_modified(entries: &[PatchEntry], body: &api::PatchResponse, modified_count: &mut usize) {
    if !body.modified_entries_and_translations.is_empty() {
        *modified_count += body.modified_entries_and_translations.len();
    } else if body.failed_entries_and_translations.is_empty() {
        *modified_count += entries.len();
    }
}

fn failure_reason(failure: &api::FailedEntry) -> &str {
    failure
        .error
        .as_ref()
        .and_then(|error| error.error_message.as_deref())
        .unwrap_or("unknown error")
}

fn exceeds_context_limit(entry: &PatchEntry, limits: &EntryOperationLimits) -> bool {
    entry.identifier.context.len() > limits.max_context_length
        || entry.identifier.key.len() > limits.max_key_length
        || entry.identifier.source.len() > limits.max_source_length
}

fn warn_if_context_too_long(entries: &[PatchEntry], limits: &EntryOperationLimits) {
    let mut too_long: Vec<String> = entries
        .iter()
        .filter(|entry| exceeds_context_limit(entry, limits))
        .map(|entry| entry.identifier.key.clone())
        .collect();

    if too_long.is_empty() {
        return;
    }

    too_long.sort();
    warn!(
        "{} keys exceed Roblox limits (context max {}, key max {}, source max {}) and cannot be pushed",
        too_long.len(),
        limits.max_context_length,
        limits.max_key_length,
        limits.max_source_length,
    );
    for key in too_long.iter().take(8) {
        warn!("  context too long: {key}");
    }
    if too_long.len() > 8 {
        warn!("  … and {} more", too_long.len() - 8);
    }
}

fn partition_create_batches(
    entries: &[PatchEntry],
    limits: &EntryOperationLimits,
) -> Vec<PatchEntry> {
    entries
        .iter()
        .filter(|entry| !exceeds_context_limit(entry, limits))
        .cloned()
        .collect()
}

struct PushState<'a> {
    failed_keys: &'a mut HashSet<String>,
    modified_count: &'a mut usize,
}

fn do_batched_requests(
    ui: &PushUi,
    client: &LocalizationClient,
    entries: &[PatchEntry],
    phase: PushPhase,
    expect_modifications: bool,
    max_entries_per_update: usize,
    max_create_entries_per_update: usize,
    state: &mut PushState<'_>,
) -> anyhow::Result<()> {
    let total_entries = entries.len();
    if total_entries == 0 {
        return Ok(());
    }

    let batch_size = if expect_modifications {
        max_create_entries_per_update
    } else {
        max_entries_per_update
    };
    let batch_count = total_entries.div_ceil(batch_size);
    let progress = ui.begin_push(phase, total_entries);
    let mut pushed_entries = 0usize;

    for (batch_index, index) in (0..total_entries).step_by(batch_size).enumerate() {
        let batch_num = batch_index + 1;
        let end_index = (index + batch_size).min(total_entries);
        let chunk = &entries[index..end_index];
        push_entries(client, chunk, state.failed_keys, state.modified_count)?;
        pushed_entries += chunk.len();

        let msg = chunk
            .last()
            .map(|entry| entry.identifier.key.as_str())
            .unwrap_or("");
        if let Some(bar) = &progress {
            bar.set(
                pushed_entries,
                &format!("batch {batch_num}/{batch_count} · {msg}"),
            );
        }

        if batch_num < batch_count {
            thread::sleep(Duration::from_secs(2));
        }
    }

    if let Some(bar) = progress {
        let label = match phase {
            PushPhase::Delete => "Deleted",
            PushPhase::Create => "Created",
        };
        bar.finish(&format!("✓ {label} {total_entries} entries"));
    } else {
        ui.finish_push_phase(phase, total_entries);
    }

    Ok(())
}

fn verify_push(
    lexicon_required: &HashMap<String, LexiconEntry>,
    remote_after_entries: &[RemoteEntry],
    _remote_after: usize,
    _remote_before: usize,
    _lexicon_count: usize,
    _modified_count: usize,
) -> VerifyResult {
    let mut matched = HashSet::new();
    for entry in remote_after_entries {
        let key = &entry.identifier.key;
        if let Some(required) = lexicon_required.get(key) {
            if lexicon_matches_remote(key, required, &entry.identifier) {
                matched.insert(key.clone());
            }
        }
    }

    let missing_count = lexicon_required
        .keys()
        .filter(|key| !matched.contains(*key))
        .count();

    let mut seen_remote_keys = HashSet::new();
    let mut leftover_keys = 0usize;
    for entry in remote_after_entries {
        let key = &entry.identifier.key;
        if !seen_remote_keys.insert(key.clone()) {
            continue;
        }
        if !lexicon_required.contains_key(key) {
            leftover_keys += 1;
        }
    }

    VerifyResult {
        missing_count,
        leftover_keys,
    }
}

fn mark_failed(key: &str, failed_keys: &mut HashSet<String>, reason: &str) {
    if failed_keys.insert(key.to_string()) {
        warn!("failed to push {key}: {reason}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_context_empty_returns_key() {
        assert_eq!(cloud_context("hello", ""), "hello");
    }

    #[test]
    fn cloud_context_with_context_uses_null_separator() {
        assert_eq!(cloud_context("hello", "ctx"), "ctx\0hello");
    }

    #[test]
    fn lexicon_matches_remote_compares_source_and_context() {
        let lexicon = LexiconEntry {
            source: "Hi".into(),
            context: "note".into(),
        };
        let remote = api::EntryIdentifier {
            key: "greeting".into(),
            source: "Hi".into(),
            context: cloud_context("greeting", "note"),
        };
        assert!(lexicon_matches_remote("greeting", &lexicon, &remote));
    }

    #[test]
    fn build_lexicon_diff_skips_matching_remote_entries() {
        let loaded = LoadedLexicon {
            locale: "en-us".into(),
            entries: HashMap::from([(
                "hello".into(),
                LexiconEntry {
                    source: "Hello".into(),
                    context: String::new(),
                },
            )]),
        };
        let remote = vec![RemoteEntry {
            identifier: api::EntryIdentifier {
                key: "hello".into(),
                source: "Hello".into(),
                context: cloud_context("hello", ""),
            },
            translations: None,
            metadata: None,
        }];

        let diff = build_lexicon_diff(&loaded, &remote);
        assert_eq!(diff.skipped_count, 1);
        assert!(diff.delete_entries.is_empty());
        assert!(diff.create_entries.is_empty());
    }

    #[test]
    fn partition_create_batches_drops_over_limit_entries() {
        let limits = EntryOperationLimits {
            max_context_length: 10,
            max_key_length: 100,
            max_source_length: 100,
        };
        let short = make_create_entry(
            "short",
            &LexiconEntry {
                source: "Hi".into(),
                context: String::new(),
            },
            None,
            None,
        );
        let long = make_create_entry(
            "long",
            &LexiconEntry {
                source: "Hi".into(),
                context: "this context is definitely too long".into(),
            },
            None,
            None,
        );

        let filtered = partition_create_batches(&[long, short], &limits);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].identifier.key, "short");
    }
}
