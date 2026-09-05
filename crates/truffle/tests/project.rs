use image::{Rgba, RgbaImage};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::{tempdir, TempDir};

fn fixture() -> TempDir {
    let directory = tempdir().unwrap();
    let source = RgbaImage::from_fn(2, 1, |x, _| {
        if x == 0 {
            Rgba([200, 100, 50, 255])
        } else {
            Rgba([30, 60, 90, 128])
        }
    });
    source.save(directory.path().join("paint.png")).unwrap();
    fs::write(
        directory.path().join("map.json"),
        r#"{
        "version":1,"source_size":[2,1],"output_size":[3,2],
        "rows":[{"at":[1,1],"pixels":[[1,0,0],[0,0,0]]}]
    }"#,
    )
    .unwrap();
    directory
}

fn run(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_truffle"))
        .current_dir(directory)
        .args(["image", "project", "paint.png", "--map", "map.json"])
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

#[test]
fn two_inputs_reproduce_the_png_in_an_unrelated_directory_without_rewriting() {
    let directory = fixture();
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    success(run(directory.path(), &[]));
    let path = directory.path().join("paint-projected.png");
    let image = image::open(&path).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (3, 2));
    assert_eq!(image.get_pixel(1, 1).0, [30, 60, 90, 128]);
    assert_eq!(image.get_pixel(2, 1).0, [200, 100, 50, 255]);
    assert_eq!(image.get_pixel(0, 0).0, [0; 4]);
    let before = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let repeated = success(run(directory.path(), &[]));
    assert!(String::from_utf8_lossy(&repeated.stdout).contains("Unchanged"));
    assert_eq!(before, fs::read(&path).unwrap());
    assert_eq!(modified, fs::metadata(&path).unwrap().modified().unwrap());
    success(run(directory.path(), &["--output", "fresh/nested.png"]));
    assert_eq!(
        before,
        fs::read(directory.path().join("fresh/nested.png")).unwrap()
    );
}

#[test]
fn dry_run_validates_without_creating_directories_and_force_replaces_changed_outputs() {
    let directory = fixture();
    let args = ["--output", "nested/result.png"];
    success(run(directory.path(), &[args[0], args[1], "--dry-run"]));
    assert!(!directory.path().join("nested").exists());
    success(run(directory.path(), &args));
    let path = directory.path().join("nested/result.png");
    let before = fs::read(&path).unwrap();
    RgbaImage::from_pixel(2, 1, Rgba([255; 4]))
        .save(directory.path().join("paint.png"))
        .unwrap();
    assert!(!run(directory.path(), &args).status.success());
    assert_eq!(before, fs::read(&path).unwrap());
    success(run(
        directory.path(),
        &[args[0], args[1], "--force", "--dry-run"],
    ));
    assert_eq!(before, fs::read(&path).unwrap());
    success(run(directory.path(), &[args[0], args[1], "--force"]));
    assert_ne!(before, fs::read(&path).unwrap());
}

#[test]
fn invalid_maps_fail_even_in_dry_run_and_preserve_existing_output() {
    let directory = fixture();
    fs::write(directory.path().join("result.png"), b"existing output").unwrap();
    fs::write(
        directory.path().join("map.json"),
        r#"{
        "version":1,"source_size":[2,1],"output_size":[3,2],
        "rows":[{"at":[0,0],"pixels":[[2,0,0]]}]
    }"#,
    )
    .unwrap();
    for extra in [&["--force"][..], &["--force", "--dry-run"][..]] {
        let mut args = vec!["--output", "result.png"];
        args.extend_from_slice(extra);
        let output = run(directory.path(), &args);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("outside source_size"));
        assert_eq!(
            fs::read(directory.path().join("result.png")).unwrap(),
            b"existing output"
        );
    }
}

#[test]
fn force_cannot_overwrite_either_input_or_a_hard_link_to_it() {
    let directory = fixture();
    let source = fs::read(directory.path().join("paint.png")).unwrap();
    let map = fs::read(directory.path().join("map.json")).unwrap();
    fs::hard_link(
        directory.path().join("map.json"),
        directory.path().join("map-link.png"),
    )
    .unwrap();
    fs::hard_link(
        directory.path().join("paint.png"),
        directory.path().join("paint-link.png"),
    )
    .unwrap();
    for name in ["paint.png", "map-link.png", "paint-link.png"] {
        let output = run(directory.path(), &["--output", name, "--force"]);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Cannot overwrite projection input")
        );
    }
    assert_eq!(
        source,
        fs::read(directory.path().join("paint.png")).unwrap()
    );
    assert_eq!(map, fs::read(directory.path().join("map.json")).unwrap());
}

#[cfg(unix)]
#[test]
fn force_cannot_overwrite_a_symlink_to_an_input() {
    let directory = fixture();
    std::os::unix::fs::symlink("paint.png", directory.path().join("alias.png")).unwrap();
    let output = run(directory.path(), &["--output", "alias.png", "--force"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Cannot overwrite projection input"));
}
