use std::path::{Path, PathBuf};

fn writable_root(root: &Path) -> bool {
    StoragePaths::from_root(root).ensure_data_dirs().is_ok()
}

fn select_data_root(preferred: &Path, fallback: &Path, mut usable: impl FnMut(&Path) -> bool) -> PathBuf {
    if usable(preferred) { preferred.to_path_buf() } else { fallback.to_path_buf() }
}

#[derive(Clone, Debug)]
pub struct StoragePaths {
    pub root: PathBuf,
    pub projects: PathBuf,
    pub originals: PathBuf,
    pub exports: PathBuf,
    pub cache: PathBuf,
}

impl StoragePaths {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            projects: root.join("数据").join("工程"),
            originals: root.join("数据").join("原片"),
            exports: root.join("导出"),
            cache: root.join("缓存"),
            root,
        }
    }

    pub fn default_for_platform() -> Self {
        #[cfg(windows)]
        let root = {
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
                .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local")))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            select_data_root(Path::new(r"D:\修图软件"), &local.join("Chromora"), writable_root)
        };
        #[cfg(not(windows))]
        let root = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Chromora");
        Self::from_root(root)
    }

    pub fn ensure_data_dirs(&self) -> std::io::Result<()> {
        for path in [&self.projects, &self.originals, &self.exports, &self.cache] {
            std::fs::create_dir_all(path)?;
        }
        let data = self.root.join("数据");
        // Existing directories can still be read-only under the current user's ACL.
        for directory in [&self.root, &data, &self.projects, &self.originals, &self.exports, &self.cache] {
            drop(tempfile::Builder::new().prefix(".chromora-write-check-").tempfile_in(directory)?);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn available_preferred_root_preserves_existing_data_location() {
        let temp = tempfile::tempdir().unwrap();
        let preferred = temp.path().join("preferred");
        let fallback = temp.path().join("local-app-data").join("Chromora");
        assert_eq!(select_data_root(&preferred, &fallback, writable_root), preferred);
        assert!(preferred.is_dir());
        assert!(!fallback.exists());
        assert!(!std::fs::read_dir(preferred).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with(".chromora-write-check-")));
    }
    #[test]
    fn unusable_preferred_root_falls_back_without_mutating_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        let preferred = temp.path().join("blocked");
        std::fs::write(&preferred, b"preserve").unwrap();
        let fallback = temp.path().join("local-app-data").join("Chromora");
        let selected = select_data_root(&preferred, &fallback, writable_root);
        assert_eq!(selected, fallback);
        StoragePaths::from_root(selected).ensure_data_dirs().unwrap();
        assert!(fallback.join("数据").join("原片").is_dir());
        assert_eq!(std::fs::read(preferred).unwrap(), b"preserve");
    }
    #[test]
    fn root_selection_does_not_depend_on_process_environment() {
        let preferred = Path::new("preferred"); let fallback = Path::new("fallback");
        assert_eq!(select_data_root(preferred, fallback, |_| false), fallback);
        assert_eq!(select_data_root(preferred, fallback, |_| true), preferred);
    }
    #[test]
    fn blocked_data_child_uses_fallback_and_preserves_the_blocking_file() {
        let temp = tempfile::tempdir().unwrap();
        let preferred = temp.path().join("preferred"); std::fs::create_dir(&preferred).unwrap();
        let blocked = preferred.join("数据"); std::fs::write(&blocked, b"existing user file").unwrap();
        let fallback = temp.path().join("local-app-data").join("Chromora");
        let selected = select_data_root(&preferred, &fallback, writable_root);
        assert_eq!(selected, fallback);
        StoragePaths::from_root(selected).ensure_data_dirs().unwrap();
        assert_eq!(std::fs::read(blocked).unwrap(), b"existing user file");
        assert!(fallback.join("数据").join("原片").is_dir());
    }
    #[test]
    fn unusable_fallback_reports_error_without_selecting_a_third_directory() {
        let temp = tempfile::tempdir().unwrap();
        let preferred = temp.path().join("blocked-preferred"); let fallback = temp.path().join("blocked-fallback");
        std::fs::write(&preferred, b"preferred").unwrap(); std::fs::write(&fallback, b"fallback").unwrap();
        let selected = select_data_root(&preferred, &fallback, writable_root);
        assert_eq!(selected, fallback);
        assert!(StoragePaths::from_root(selected).ensure_data_dirs().is_err());
        assert_eq!(std::fs::read(preferred).unwrap(), b"preferred");
        assert_eq!(std::fs::read(fallback).unwrap(), b"fallback");
    }
}
