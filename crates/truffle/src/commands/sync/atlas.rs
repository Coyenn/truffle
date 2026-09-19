use super::{
    catalog::{atlas_file_ids_from_assets, filter_assets_by_exclude, merge_asset_values, publish},
    paths::{
        build_atlas_exclude, build_exclude_glob, is_images_input, resolve_atlas_exclude,
        scratch_atlas_png_dir, scratch_sync_dir, scratch_unatlased_dir, ATLAS_STATE_FILE,
    },
    resolve_api_key, EffectiveSync,
};
use crate::assets::{
    augment_assets, build_atlased_assets, build_atlases, load_assets, AtlasOptions, FsImageMetadata,
};
use anyhow::Context;
use asphalt::{
    cli::{SyncArgs as AsphaltSyncArgs, SyncTarget},
    config::Input as AsphaltInput,
    glob::Glob,
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
    println!("[sync] Building image atlases …");
    let atlas_dir = scratch_atlas_png_dir(scratch_dir);
    let sync_codegen_dir = scratch_sync_dir(scratch_dir);
    // Asphalt codegen writes `{input_name}.luau`. Our atlas input is named `atlases`.
    let atlas_assets_output = sync_codegen_dir.join("atlases.luau");
    let atlas_padding = args.atlas_padding;
    let atlas_size = args.atlas_size;
    let atlas_exclude = resolve_atlas_exclude(
        &args.atlas_exclude,
        &config.atlas_exclude,
        &args.images_folder,
    );
    let atlas_exclude_matcher = build_atlas_exclude(&atlas_exclude)?;
    let atlas_state_path = scratch_dir.join(ATLAS_STATE_FILE);

    let placements = build_atlases(
        &args.images_folder,
        &atlas_dir,
        &atlas_state_path,
        AtlasOptions {
            padding: atlas_padding,
            size: atlas_size,
            exclude: atlas_exclude_matcher.clone(),
        },
    )
    .context("Failed to build atlases")?;

    std::fs::create_dir_all(&sync_codegen_dir).with_context(|| {
        format!(
            "Failed to create codegen dir {}",
            sync_codegen_dir.display()
        )
    })?;
    let unatlased_codegen_dir = scratch_unatlased_dir(scratch_dir);
    if unatlased_codegen_dir.exists() {
        fs::remove_dir_all(&unatlased_codegen_dir).context("Failed to clear unatlased staging")?;
    }

    // Resolve API key: --api-key flag, TRUFFLE_API_KEY env (.env included), truffle.toml.
    let api_key = resolve_api_key(args.api_key.clone(), config.api_key.clone())?;

    // Reuse the already-parsed config instead of re-reading truffle.toml.
    let mut asphalt_config = config.asphalt.clone();
    asphalt_config.inputs = {
        let mut inputs = HashMap::new();

        let atlas_glob = format!("{}/**/*.png", atlas_dir.display());
        inputs.insert(
            "atlases".to_string(),
            AsphaltInput {
                include: Glob::new(atlas_glob.as_str()).context("Invalid atlas include glob")?,
                output_path: sync_codegen_dir.clone(),
                bleed: false,
                web: HashMap::new(),
            },
        );

        let exclude_glob = if atlas_exclude.is_empty() {
            None
        } else {
            Some(
                build_exclude_glob(&args.images_folder, &atlas_exclude)
                    .context("Atlas exclude list was empty after normalization")?,
            )
        };

        let mut found_images_input = false;
        for (name, input) in asphalt_config.inputs.iter() {
            if is_images_input(&args.images_folder, &input.include.get_prefix()) {
                found_images_input = true;
                if let Some(exclude_glob) = &exclude_glob {
                    let mut updated = input.clone();
                    updated.include =
                        Glob::new(exclude_glob.as_str()).context("Invalid atlas exclude glob")?;
                    updated.output_path = unatlased_codegen_dir.clone();
                    inputs.insert(name.clone(), updated);
                }
                continue;
            }

            inputs.insert(name.clone(), input.clone());
        }

        if !atlas_exclude.is_empty() && !found_images_input {
            anyhow::bail!("Failed to find images input matching images_folder");
        }

        inputs
    };

    // Run Asphalt sync on the generated atlas PNGs
    println!("[sync] Running backend sync …");
    let multi_progress = MultiProgress::new();
    let sync_args = AsphaltSyncArgs {
        api_key: Some(api_key),
        target: Some(SyncTarget::Cloud { dry_run: false }),
        expected_price: None,
        project: PathBuf::from("."),
    };

    sync_with_config(asphalt_config, sync_args, multi_progress)
        .await
        .context("Failed to sync atlases with Asphalt")?;

    // Load atlas asset ids produced by Asphalt
    let atlas_ids = if placements.is_empty() {
        HashMap::new()
    } else {
        let atlas_assets = load_assets(&atlas_assets_output)
            .map_err(|e| anyhow::anyhow!("Failed to load uploaded atlas IDs: {e}"))?;
        atlas_file_ids_from_assets(&atlas_assets)
    };

    // Build the final assets tree keyed by original image paths
    let mut final_assets = build_atlased_assets(&placements, &atlas_ids)
        .context("Failed to build atlased asset metadata")?;

    if !atlas_exclude.is_empty() {
        let unatlased_assets_path = unatlased_codegen_dir.join("assets.luau");
        let excluded_assets = if unatlased_assets_path.exists() {
            load_assets(&unatlased_assets_path)
                .map_err(|e| anyhow::anyhow!("Failed to load unatlased assets: {}", e))?
        } else {
            BTreeMap::new()
        };
        let filtered_excluded = filter_assets_by_exclude(&excluded_assets, &atlas_exclude_matcher);
        let augmented_excluded =
            augment_assets(&filtered_excluded, &args.images_folder, &FsImageMetadata);
        merge_asset_values(&mut final_assets, &augmented_excluded);
    }

    publish(&final_assets, &args.assets_output, &args.dts_output)
}
