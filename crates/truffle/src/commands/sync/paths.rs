use crate::assets::AtlasExclude;
use anyhow::Context;
use asphalt::glob::Glob;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const SCRATCH_ATLAS_PNG_DIR: &str = "atlases";
const SCRATCH_SYNC_DIR: &str = "sync";
const SCRATCH_UNATLASED_DIR: &str = "unatlased";
const SCRATCH_SUBSET_DIR: &str = "subset";
/// Packing state (tracked via gitignore exception) so atlases stay stable
/// across machines and syncs.
pub(super) const ATLAS_STATE_FILE: &str = "truffle-atlases.toml";

pub(super) fn scratch_atlas_png_dir(scratch_dir: &Path) -> PathBuf {
    scratch_dir.join(SCRATCH_ATLAS_PNG_DIR)
}

pub(super) fn scratch_sync_dir(scratch_dir: &Path) -> PathBuf {
    scratch_dir.join(SCRATCH_SYNC_DIR)
}

pub(super) fn scratch_unatlased_dir(scratch_dir: &Path) -> PathBuf {
    scratch_sync_dir(scratch_dir).join(SCRATCH_UNATLASED_DIR)
}

pub(super) fn scratch_subset_dir(scratch_dir: &Path) -> PathBuf {
    scratch_sync_dir(scratch_dir).join(SCRATCH_SUBSET_DIR)
}

pub(super) fn prepare_scratch_dir(scratch_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(scratch_dir)
        .with_context(|| format!("Failed to create scratch dir {}", scratch_dir.display()))?;

    for legacy in ["asphalt", "subset-sync"] {
        let legacy_path = scratch_dir.join(legacy);
        if legacy_path.is_dir() {
            fs::remove_dir_all(&legacy_path).with_context(|| {
                format!("Failed to remove legacy dir {}", legacy_path.display())
            })?;
        }
    }
    Ok(())
}

pub(super) fn remove_scratch_subset(scratch_dir: &Path) -> anyhow::Result<()> {
    let subset_dir = scratch_subset_dir(scratch_dir);
    if subset_dir.is_dir() {
        fs::remove_dir_all(&subset_dir)
            .with_context(|| format!("Failed to remove {}", subset_dir.display()))?;
    }
    Ok(())
}

pub(super) fn resolve_atlas_exclude(
    cli: &[String],
    config: &[String],
    images_folder: &Path,
) -> Vec<String> {
    let raw = if !cli.is_empty() { cli } else { config };
    let mut out: Vec<String> = raw
        .iter()
        .filter_map(|item| normalize_atlas_key(item, images_folder))
        .collect();
    out.retain(|item| !item.is_empty());
    out.sort();
    out.dedup();
    out
}

fn normalize_atlas_key(value: &str, images_folder: &Path) -> Option<String> {
    let mut key = value.replace('\\', "/");
    while let Some(stripped) = key.strip_prefix("./") {
        key = stripped.to_string();
    }
    while let Some(stripped) = key.strip_prefix('/') {
        key = stripped.to_string();
    }

    let images_folder = normalize_path_for_compare(images_folder);
    if !images_folder.is_empty() {
        let with_sep = format!("{}/", images_folder);
        if key.starts_with(&with_sep) {
            key = key[with_sep.len()..].to_string();
        } else if key == images_folder {
            return None;
        } else if let Some(images_root) = images_folder.split('/').next() {
            let root_prefix = format!("{}/", images_root);
            if key.starts_with(&root_prefix) {
                return None;
            }
        }
    }

    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

pub(super) fn build_exclude_glob(images_folder: &Path, keys: &[String]) -> Option<String> {
    let mut patterns = Vec::new();
    for key in keys {
        patterns.extend(build_exclude_patterns(key));
    }

    if patterns.is_empty() {
        return None;
    }

    patterns.sort();
    patterns.dedup();

    let images_folder = normalize_path_for_compare(images_folder);
    if images_folder.is_empty() {
        return Some(format!("{{{}}}", patterns.join(",")));
    }

    // Keep the image root as the backend's key prefix even for one exact file.
    Some(format!("{images_folder}/{{{}}}", patterns.join(",")))
}

fn build_exclude_patterns(value: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    let raw = value.trim().trim_matches('/').to_string();
    if raw.is_empty() {
        return patterns;
    }

    let has_glob = raw
        .chars()
        .any(|c| matches!(c, '*' | '?' | '{' | '}' | '[' | ']'));
    let is_file = raw.to_ascii_lowercase().contains(".png");

    let file_pattern = if !has_glob && !is_file {
        format!("{}/**", raw)
    } else {
        raw.clone()
    };
    patterns.push(file_pattern);

    let prefix = glob_prefix(&raw);
    let prefix = prefix.trim_end_matches('/');
    let dir = if is_file || prefix.to_ascii_lowercase().ends_with(".png") {
        prefix
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_string())
    } else if prefix.is_empty() {
        None
    } else {
        Some(prefix.to_string())
    };

    if let Some(dir) = dir {
        patterns.extend(path_ancestors(&dir));
    }

    patterns
}

fn glob_prefix(value: &str) -> &str {
    match value.find(|c| ['*', '?', '{', '}', '[', ']'].contains(&c)) {
        Some(index) => &value[..index],
        None => value,
    }
}

fn path_ancestors(path: &str) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut current = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if current.is_empty() {
            current = segment.to_string();
        } else {
            current.push('/');
            current.push_str(segment);
        }
        ancestors.push(current.clone());
    }
    ancestors
}

pub(super) fn is_images_input(images_folder: &Path, input_prefix: &Path) -> bool {
    normalize_path_for_compare(images_folder) == normalize_path_for_compare(input_prefix)
}

fn normalize_path_for_compare(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('\\', "/");
    while let Some(stripped) = value.strip_prefix("./") {
        value = stripped.to_string();
    }
    while let Some(stripped) = value.strip_prefix('/') {
        value = stripped.to_string();
    }
    while let Some(stripped) = value.strip_suffix('/') {
        value = stripped.to_string();
    }
    value
}

pub(super) fn build_atlas_exclude(keys: &[String]) -> anyhow::Result<AtlasExclude> {
    let mut exact = HashSet::new();
    let mut globs = Vec::new();

    for raw in keys {
        let normalized = raw.trim().to_string();
        if normalized.is_empty() {
            continue;
        }

        let pattern = normalize_exclude_pattern(&normalized);
        if pattern.is_glob {
            globs.push(
                Glob::new(pattern.pattern.as_str())
                    .with_context(|| format!("Invalid atlas exclude glob: {}", pattern.pattern))?,
            );
        } else {
            exact.insert(pattern.pattern);
        }
    }

    Ok(AtlasExclude { exact, globs })
}

fn normalize_exclude_pattern(value: &str) -> ExcludePattern {
    let trimmed = value.trim_matches('/');
    let mut pattern = trimmed.to_string();
    let has_glob = pattern
        .chars()
        .any(|c| ['*', '?', '{', '}', '[', ']'].contains(&c));

    if !has_glob {
        if pattern.ends_with('/') {
            pattern = format!("{}**/*.png", pattern);
        } else if !pattern.contains('.') {
            pattern = format!("{}/**/*.png", pattern);
        }
    }

    let is_glob = pattern
        .chars()
        .any(|c| ['*', '?', '{', '}', '[', ']'].contains(&c));

    ExcludePattern { pattern, is_glob }
}

struct ExcludePattern {
    pattern: String,
    is_glob: bool,
}

/// Exact filenames need a directory prefix, just like wildcard selections.
/// Asphalt's walker emits keys relative to this prefix.
pub(super) fn subset_glob(pattern: &str) -> anyhow::Result<Glob> {
    let pattern = pattern.replace('\\', "/");
    let pattern = pattern.trim_start_matches("./");
    if !pattern.chars().any(|c| "*?{}[]".contains(c)) {
        let path = Path::new(pattern);
        let parent = path.parent().context("Selected file has no parent")?;
        let name = path
            .file_name()
            .context("Selected file has no filename")?
            .to_string_lossy();
        return Glob::new(&format!("{}/{{{name}}}", parent.display()))
            .context("Invalid --sync-only glob");
    }
    Glob::new(pattern).context("Invalid --sync-only glob")
}

pub(super) fn sync_subset_nested_prefix(
    include: &Glob,
    images_folder: &Path,
) -> anyhow::Result<Vec<String>> {
    let images = normalize_path_for_compare(images_folder);
    let prefix = normalize_path_for_compare(&include.get_prefix());
    let relative = Path::new(&prefix)
        .strip_prefix(&images)
        .context("--sync-only must select files inside images_folder")?;
    Ok(relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect())
}

pub(super) fn validate_selection(pattern: &str, images_folder: &Path) -> anyhow::Result<usize> {
    let include = subset_glob(pattern)?;
    sync_subset_nested_prefix(&include, images_folder)?;
    let mut count = 0;
    for entry in walkdir::WalkDir::new(images_folder) {
        let entry = entry.context("Failed to inspect sync selection")?;
        if entry.file_type().is_file()
            && entry.path().extension().is_some_and(|ext| ext == "png")
            && include.is_match(entry.path().to_string_lossy().trim_start_matches("./"))
        {
            count += 1;
        }
    }
    anyhow::ensure!(count > 0, "--sync-only matched no PNG files: {pattern}");
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::{subset_glob, sync_subset_nested_prefix};
    use std::path::Path;

    #[test]
    fn subset_keys_use_the_backend_glob_prefix() {
        for pattern in [
            "assets/images/interface/store-bundles/*.png",
            "assets/images/interface/store-bundles/**/*.png",
            "assets/images/interface/store-bundles/grow-all.png",
            "./assets/images/interface/store-bundles/{grow-all,potion}.png",
        ] {
            let include = subset_glob(pattern).unwrap();
            assert!(include.is_match("assets/images/interface/store-bundles/grow-all.png"));
            assert_eq!(
                sync_subset_nested_prefix(&include, Path::new("assets/images")).unwrap(),
                ["interface", "store-bundles"]
            );
        }
        let include = subset_glob("assets/images/{interface,items}/**/*.png").unwrap();
        assert!(
            sync_subset_nested_prefix(&include, Path::new("assets/images"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn subset_outside_image_root_is_rejected() {
        let include = subset_glob("other/*.png").unwrap();
        assert!(sync_subset_nested_prefix(&include, Path::new("assets/images")).is_err());
    }
}
