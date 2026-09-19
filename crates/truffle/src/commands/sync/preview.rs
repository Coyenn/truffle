use super::{paths, EffectiveSync};
use crate::assets::{build_atlases, AtlasOptions};
use anyhow::Context;
use std::{collections::HashSet, fs};
use truffle_config::TruffleConfig;

/// Build the same packing plan in temporary storage, never in the project.
pub(super) fn run(args: &EffectiveSync, config: &TruffleConfig) -> anyhow::Result<()> {
    if let Some(pattern) = &args.sync_only {
        let count = paths::validate_selection(pattern, &args.images_folder)?;
        println!("[sync] Dry-run: {count} source images selected");
    }
    if config.auto_highlight {
        println!("[sync] Dry-run: configured highlight generation will run during sync");
    }
    if !args.skip_atlas && (args.atlas || config.atlas) {
        let temporary = tempfile::tempdir().context("Failed to create temporary atlas preview")?;
        let state = temporary.path().join(paths::ATLAS_STATE_FILE);
        let existing_state = args.scratch_dir.join(paths::ATLAS_STATE_FILE);
        if existing_state.exists() {
            fs::copy(&existing_state, &state).context("Failed to read existing atlas state")?;
        }
        let exclude = paths::resolve_atlas_exclude(
            &args.atlas_exclude,
            &config.atlas_exclude,
            &args.images_folder,
        );
        let placements = build_atlases(
            &args.images_folder,
            &temporary.path().join("atlases"),
            &state,
            AtlasOptions {
                size: args.atlas_size,
                padding: args.atlas_padding,
                exclude: paths::build_atlas_exclude(&exclude)?,
            },
        )?;
        let pages: HashSet<_> = placements.values().map(|p| &p.atlas_file_name).collect();
        println!(
            "[sync] Dry-run: complete atlas layout has {} sprites on {} pages; only changed pages will upload",
            placements.len(), pages.len()
        );
    } else {
        println!("[sync] Dry-run: source images will sync directly");
    }
    println!("[sync] Dry-run: no uploads or project files changed");
    Ok(())
}
