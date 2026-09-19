use image::{Rgba, RgbaImage};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::{tempdir, TempDir};

fn fixture(atlas: bool) -> TempDir {
    let directory = tempdir().unwrap();
    fs::create_dir_all(directory.path().join("images/store")).unwrap();
    fs::create_dir(directory.path().join("generated")).unwrap();
    fs::write(
        directory.path().join("truffle.toml"),
        format!(
            r#"
atlas = {atlas}
atlas_size = 256
images_folder = "images"
assets_input = "generated/assets.luau"
assets_output = "generated/assets.luau"
dts_output = "generated/assets.d.ts"
[creator]
type = "user"
id = 1
[inputs.assets]
path = "images/**/*.png"
output_path = "generated"
bleed = false
"#
        ),
    )
    .unwrap();
    sprite(
        directory.path(),
        "store/potion.png",
        8,
        10,
        [255, 0, 0, 255],
    );
    sprite(directory.path(), "other.png", 12, 14, [0, 255, 0, 255]);
    directory
}

fn sprite(root: &Path, key: &str, width: u32, height: u32, rgba: [u8; 4]) {
    RgbaImage::from_pixel(width, height, Rgba(rgba))
        .save(root.join("images").join(key))
        .unwrap();
}

fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_truffle"))
        .current_dir(root)
        .env("ASPHALT_TEST", "1")
        .args(["sync", "--api-key", "offline-test"])
        .args(arguments)
        .output()
        .unwrap()
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn catalog(root: &Path) -> String {
    fs::read_to_string(root.join("generated/assets.luau")).unwrap()
}

fn entry<'a>(catalog: &'a str, key: &str) -> &'a str {
    catalog
        .split(&format!("[\"{key}\"] = {{"))
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap()
}

fn files(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            (
                entry.path().strip_prefix(root).unwrap().to_path_buf(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn direct_subset_updates_selected_dimensions_and_keeps_unsynced_metadata_and_hashes() {
    let directory = fixture(false);
    let root = directory.path();
    success(run(root, &[]));
    let before = catalog(root);
    let old_lock = fs::read_to_string(root.join("truffle.lock.toml")).unwrap();
    sprite(root, "store/potion.png", 16, 18, [255, 0, 0, 26]);
    sprite(root, "other.png", 30, 32, [0, 255, 0, 255]);
    sprite(root, "store/grow-all.png", 10, 12, [0, 0, 255, 255]);
    success(run(root, &["--sync-only", "images/store/*.png"]));
    let after = catalog(root);
    assert!(entry(&after, "potion.png").contains("width = 16"));
    assert!(entry(&after, "grow-all.png").contains("height = 12"));
    assert_eq!(entry(&before, "other.png"), entry(&after, "other.png"));
    assert!(!after.contains("[\"*.png\"]"));
    let new_lock = fs::read_to_string(root.join("truffle.lock.toml")).unwrap();
    for table in old_lock
        .lines()
        .filter(|line| line.starts_with("[inputs.assets."))
    {
        assert!(new_lock.contains(table), "lost cached upload {table}");
    }
}

#[test]
fn exact_file_subset_can_create_the_first_catalog() {
    let directory = fixture(false);
    success(run(
        directory.path(),
        &["--sync-only", "images/store/potion.png"],
    ));
    let result = catalog(directory.path());
    assert_eq!(result.matches("[\"potion.png\"]").count(), 1);
    assert!(entry(&result, "potion.png").contains("width = 8"));
    assert!(!result.contains("other.png"));
}

#[test]
fn dry_run_preserves_all_project_files_with_and_without_existing_layout() {
    let directory = fixture(true);
    let root = directory.path();
    let before = files(root);
    success(run(
        root,
        &["--dry-run", "--sync-only", "images/store/*.png"],
    ));
    assert_eq!(files(root), before);
    assert!(!root.join(".truffle").exists());
    success(run(root, &[]));
    sprite(root, "store/potion.png", 26, 28, [255, 0, 0, 255]);
    let before = files(root);
    success(run(root, &["--dry-run"]));
    success(run(
        root,
        &[
            "--dry-run",
            "--skip-atlas",
            "--sync-only",
            "images/store/potion.png",
        ],
    ));
    assert_eq!(files(root), before);
}

#[test]
fn atlas_selection_reports_shared_layout_and_updates_all_changed_sizes() {
    let directory = fixture(true);
    let root = directory.path();
    success(run(root, &[]));
    sprite(root, "other.png", 20, 22, [0, 255, 0, 255]);
    sprite(root, "store/grow-all.png", 30, 32, [0, 0, 255, 26]);
    let result = success(run(root, &["--sync-only", "images/store/grow-all.png"]));
    assert!(String::from_utf8_lossy(&result.stdout).contains("complete layout"));
    let assets = catalog(root);
    assert!(entry(&assets, "grow-all.png").contains("width = 30"));
    assert!(entry(&assets, "other.png").contains("rectW = 20"));
    success(run(root, &[]));
    assert_eq!(catalog(root), assets);
}

#[test]
fn failed_publication_restores_previous_catalog_lock_and_absent_layout() {
    let directory = fixture(true);
    let root = directory.path();
    fs::write(root.join("generated/assets.luau"), "previous catalog").unwrap();
    fs::write(root.join("generated/assets.d.ts"), "").unwrap();
    fs::write(root.join("truffle.lock.toml"), "version = 2\n[inputs]\n").unwrap();
    let before = files(root);
    let result = run(
        root,
        &[
            "--assets-output",
            "generated/new",
            "--dts-output",
            "generated/new/declarations.d.ts",
        ],
    );
    assert!(String::from_utf8_lossy(&result.stderr)
        .contains("Failed to read generated/new/declarations.d.ts"));
    assert!(!result.status.success());
    for key in [
        "generated/assets.luau",
        "generated/assets.d.ts",
        "truffle.lock.toml",
    ] {
        assert_eq!(fs::read(root.join(key)).unwrap(), before[Path::new(key)]);
    }
    assert!(!root.join(".truffle/truffle-atlases.toml").exists());
    assert!(!root.join(".truffle/sync/atlases.luau").exists());
    assert!(!root.join("generated/new").exists());
}

#[test]
fn empty_selection_fails_without_modifying_project_files() {
    let directory = fixture(true);
    let before = files(directory.path());
    let result = run(
        directory.path(),
        &["--sync-only", "images/store/missing.png"],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("matched no PNG"));
    assert_eq!(files(directory.path()), before);
}

#[test]
fn repeated_sync_does_not_rewrite_unchanged_final_catalogs() {
    for atlas in [false, true] {
        let directory = fixture(atlas);
        let root = directory.path();
        success(run(root, &[]));
        let luau = root.join("generated/assets.luau");
        let dts = root.join("generated/assets.d.ts");
        let before = (
            fs::metadata(&luau).unwrap().modified().unwrap(),
            fs::metadata(&dts).unwrap().modified().unwrap(),
        );
        success(run(root, &[]));
        assert_eq!(
            before,
            (
                fs::metadata(&luau).unwrap().modified().unwrap(),
                fs::metadata(&dts).unwrap().modified().unwrap()
            )
        );
    }
}

#[test]
fn removed_excluded_images_do_not_reappear_from_previous_staging() {
    let directory = fixture(true);
    let root = directory.path();
    let args = ["--atlas-exclude", "other.png"];
    success(run(root, &args));
    assert!(catalog(root).contains("other.png"));
    fs::remove_file(root.join("images/other.png")).unwrap();
    success(run(root, &args));
    assert!(!catalog(root).contains("other.png"));
}
