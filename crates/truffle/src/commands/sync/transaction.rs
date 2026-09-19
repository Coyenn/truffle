use anyhow::{Context, Result};
use std::{fs, path::PathBuf};

struct Snapshot {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

pub(super) struct SyncTransaction {
    snapshots: Vec<Snapshot>,
}

impl SyncTransaction {
    pub(super) fn begin(paths: &[PathBuf]) -> Result<Self> {
        let mut snapshots = Vec::new();
        for path in paths {
            if snapshots
                .iter()
                .any(|snapshot: &Snapshot| snapshot.path == *path)
            {
                continue;
            }
            let contents = match fs::read(path) {
                Ok(contents) => Some(contents),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("Failed to snapshot {}", path.display()));
                }
            };
            snapshots.push(Snapshot {
                path: path.clone(),
                contents,
            });
        }
        Ok(Self { snapshots })
    }

    pub(super) fn commit(self) {}

    pub(super) fn rollback(self) -> Result<()> {
        let mut failures = Vec::new();
        for snapshot in self.snapshots {
            let result = match snapshot.contents {
                Some(contents) => {
                    let parent = snapshot
                        .path
                        .parent()
                        .filter(|path| !path.as_os_str().is_empty());
                    parent
                        .map_or(Ok(()), fs::create_dir_all)
                        .and_then(|()| fs::write(&snapshot.path, contents))
                }
                None => match fs::remove_file(&snapshot.path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    result => result,
                },
            };
            if let Err(error) = result {
                failures.push(format!("{}: {error}", snapshot.path.display()));
            }
        }
        anyhow::ensure!(
            failures.is_empty(),
            "Failed to restore sync state: {}",
            failures.join("; ")
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_sync_restores_catalog_layout_and_lockfile_together() {
        let directory = tempfile::tempdir().unwrap();
        let paths: Vec<_> = [
            "assets.luau",
            "assets.d.ts",
            "truffle-atlases.toml",
            "truffle.lock.toml",
        ]
        .iter()
        .map(|name| directory.path().join(name))
        .collect();
        for (index, path) in paths.iter().enumerate() {
            fs::write(path, format!("original-{index}")).unwrap();
        }
        let transaction = SyncTransaction::begin(&paths).unwrap();
        for path in &paths {
            fs::write(path, "partially-published").unwrap();
        }
        transaction.rollback().unwrap();
        for (index, path) in paths.iter().enumerate() {
            assert_eq!(
                fs::read_to_string(path).unwrap(),
                format!("original-{index}")
            );
        }
    }

    #[test]
    fn rollback_preserves_empty_files_and_removes_new_files() {
        let directory = tempfile::tempdir().unwrap();
        let empty = directory.path().join("empty.luau");
        let missing = directory.path().join("new.d.ts");
        fs::write(&empty, []).unwrap();
        let transaction = SyncTransaction::begin(&[empty.clone(), missing.clone()]).unwrap();
        fs::write(&empty, "new contents").unwrap();
        fs::write(&missing, "new contents").unwrap();
        transaction.rollback().unwrap();
        assert_eq!(fs::read(empty).unwrap(), Vec::<u8>::new());
        assert!(!missing.exists());
    }

    #[test]
    fn committed_sync_keeps_new_contents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("assets.luau");
        let transaction = SyncTransaction::begin(std::slice::from_ref(&path)).unwrap();
        fs::write(&path, "published").unwrap();
        transaction.commit();
        assert_eq!(fs::read_to_string(path).unwrap(), "published");
    }

    #[test]
    fn rollback_restores_backend_catalog_after_staging_directory_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let staging = directory.path().join("subset");
        fs::create_dir(&staging).unwrap();
        let path = staging.join("assets.luau");
        fs::write(&path, "original").unwrap();
        let transaction = SyncTransaction::begin(std::slice::from_ref(&path)).unwrap();
        fs::remove_dir_all(&staging).unwrap();
        transaction.rollback().unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
    }

    #[test]
    fn rollback_restores_other_files_when_one_restore_fails() {
        let directory = tempfile::tempdir().unwrap();
        let blocked = directory.path().join("blocked");
        let catalog = directory.path().join("assets.luau");
        fs::write(&catalog, "original").unwrap();
        let transaction = SyncTransaction::begin(&[blocked.clone(), catalog.clone()]).unwrap();
        fs::create_dir(&blocked).unwrap();
        fs::write(&catalog, "partial").unwrap();
        assert!(transaction.rollback().is_err());
        assert_eq!(fs::read_to_string(catalog).unwrap(), "original");
    }
}
