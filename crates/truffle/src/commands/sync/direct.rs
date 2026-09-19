use super::{
    catalog::{merge_asset_values, nest_assets_under_path, publish},
    paths::{
        is_images_input, remove_scratch_subset, scratch_subset_dir, scratch_sync_dir, subset_glob,
        sync_subset_nested_prefix,
    },
    resolve_api_key, EffectiveSync,
};
use crate::assets::{augment_assets, load_assets, FsImageMetadata};
use anyhow::Context;
use asphalt::{
    cli::{SyncArgs as AsphaltSyncArgs, SyncTarget},
    config::Input as AsphaltInput,
    lockfile::RawLockfile,
    sync_with_config,
};
use indicatif::MultiProgress;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
};
use truffle_config::TruffleConfig;

pub(super) async fn run(args: &EffectiveSync, config: &TruffleConfig) -> anyhow::Result<()> {
    let scratch_dir = &args.scratch_dir;
    // Run Asphalt sync
    // Resolve API key: --api-key flag, TRUFFLE_API_KEY env (.env included), truffle.toml.
    let api_key = resolve_api_key(args.api_key.clone(), config.api_key.clone())?;
    println!("[sync] Running backend sync …");
    let multi_progress = MultiProgress::new();
    let sync_args = AsphaltSyncArgs {
        api_key: Some(api_key),
        target: Some(SyncTarget::Cloud { dry_run: false }),
        expected_price: None,
        project: PathBuf::from("."),
    };

    if let Some(sync_only) = &args.sync_only {
        // Reuse the already-parsed config instead of re-reading truffle.toml.
        let mut asphalt_config = config.asphalt.clone();
        remove_scratch_subset(scratch_dir)?;
        let subset_output = scratch_subset_dir(scratch_dir);
        let include = subset_glob(sync_only)?;
        let prefix = sync_subset_nested_prefix(&include, &args.images_folder)?;
        asphalt_config.inputs = HashMap::from([(
            "assets".to_string(),
            AsphaltInput {
                include,
                output_path: subset_output.clone(),
                bleed: asphalt_config
                    .inputs
                    .get("assets")
                    .map(|input| input.bleed)
                    .unwrap_or(true),
                web: HashMap::new(),
            },
        )]);
        let project_dir = PathBuf::from(".");
        let full_lockfile = RawLockfile::read_from(&project_dir)
            .await
            .context("Failed to read lockfile before subset sync")?
            .into_lockfile()?;

        sync_with_config(asphalt_config, sync_args, multi_progress)
            .await
            .context("Failed to sync subset with Asphalt")?;

        println!("[sync] Merging synced subset into existing lockfile …");
        let subset_lockfile = RawLockfile::read_from(&project_dir)
            .await
            .context("Failed to read lockfile after subset sync")?
            .into_lockfile()?;
        let mut merged_lockfile = full_lockfile;
        merged_lockfile.absorb(subset_lockfile);
        merged_lockfile
            .write_to(&project_dir)
            .await
            .context("Failed to write merged lockfile")?;

        println!("[sync] Merging synced subset into existing assets module …");
        let synced_subset = load_assets(&subset_output.join("assets.luau"))
            .map_err(|e| anyhow::anyhow!("Failed to load synced subset assets: {}", e))?;
        let synced_subset = nest_assets_under_path(synced_subset, &prefix);
        let synced_subset = augment_assets(&synced_subset, &args.images_folder, &FsImageMetadata);
        let mut assets = if args.assets_input.exists() {
            load_assets(&args.assets_input)
                .map_err(|e| anyhow::anyhow!("Failed to load assets: {e}"))?
        } else {
            BTreeMap::new()
        };
        merge_asset_values(&mut assets, &synced_subset);
        // Only the uploaded subset has new source dimensions. Updating every
        // entry here would publish local edits whose texture was not uploaded.
        let augmented_assets = assets;

        publish(&augmented_assets, &args.assets_output, &args.dts_output)?;

        remove_scratch_subset(scratch_dir)?;
        return Ok(());
    }

    let mut asphalt_config = config.asphalt.clone();
    let direct_output = scratch_sync_dir(scratch_dir).join("direct");
    if direct_output.exists() {
        fs::remove_dir_all(&direct_output).context("Failed to clear direct sync staging")?;
    }
    let (name, input) = asphalt_config
        .inputs
        .iter_mut()
        .find(|(_, input)| is_images_input(&args.images_folder, &input.include.get_prefix()))
        .context("Failed to find images input matching images_folder")?;
    input.output_path = direct_output.clone();
    let staged_catalog = direct_output.join(format!("{name}.luau"));
    sync_with_config(asphalt_config, sync_args, multi_progress)
        .await
        .context("Failed to sync assets with Asphalt")?;

    // Augment with image dimensions
    println!("[sync] Augmenting with image dimensions …");
    let assets = load_assets(&staged_catalog)
        .map_err(|e| anyhow::anyhow!("Failed to load assets: {}", e))?;

    let augmented_assets = augment_assets(&assets, &args.images_folder, &FsImageMetadata);

    publish(&augmented_assets, &args.assets_output, &args.dts_output)
}
