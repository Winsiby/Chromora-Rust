use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// When this variable names a path whose data directories can be created and written,
/// that path is the data root. An empty value is treated as unset. On Windows, an unset
/// or unusable value keeps the historical default: `D:\修图软件`, then `%LOCALAPPDATA%\Chromora`.
pub const DATA_ROOT_ENV: &str = "CHROMORA_DATA_ROOT";

fn writable_root(root: &Path) -> bool {
    StoragePaths::from_root(root).ensure_data_dirs().is_ok()
}

// Used by the Windows default and by unit tests. Non-Windows release builds keep the
// historical `$HOME/Chromora` path without calling this probe.
#[cfg_attr(not(windows), allow(dead_code))]
fn select_data_root(preferred: &Path, fallback: &Path, mut usable: impl FnMut(&Path) -> bool) -> PathBuf {
    if usable(preferred) { preferred.to_path_buf() } else { fallback.to_path_buf() }
}

fn data_root_override_from_env(value: Option<&OsStr>) -> Option<PathBuf> {
    let value = value?;
    if value.is_empty() { None } else { Some(PathBuf::from(value)) }
}

/// Use `configured` when it is usable. Otherwise keep the historical preferred-then-fallback order.
/// The fallback itself is not probed: an unusable fallback is still returned, matching the
/// previous Windows selection.
#[cfg_attr(not(windows), allow(dead_code))]
fn select_data_root_with_override(
    configured: Option<&Path>,
    preferred: &Path,
    fallback: &Path,
    mut usable: impl FnMut(&Path) -> bool,
) -> PathBuf {
    if let Some(path) = configured {
        if usable(path) {
            return path.to_path_buf();
        }
    }
    select_data_root(preferred, fallback, usable)
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
        let configured = data_root_override_from_env(std::env::var_os(DATA_ROOT_ENV).as_deref());
        #[cfg(windows)]
        let root = {
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
                .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local")))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            select_data_root_with_override(
                configured.as_deref(),
                Path::new(r"D:\修图软件"),
                &local.join("Chromora"),
                writable_root,
            )
        };
        #[cfg(not(windows))]
        let root = {
            let fallback = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Chromora");
            match configured.as_deref() {
                Some(path) if writable_root(path) => path.to_path_buf(),
                _ => fallback,
            }
        };
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
    use std::ffi::OsStr;

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
    fn env_name_is_chromora_data_root_and_empty_means_unset() {
        assert_eq!(DATA_ROOT_ENV, "CHROMORA_DATA_ROOT");
        assert!(data_root_override_from_env(None).is_none());
        assert!(data_root_override_from_env(Some(OsStr::new(""))).is_none());
        assert_eq!(
            data_root_override_from_env(Some(OsStr::new(r"E:\Chromora"))).unwrap(),
            PathBuf::from(r"E:\Chromora")
        );
    }

    #[test]
    fn writable_override_is_used_before_the_windows_default_and_does_not_probe_d() {
        let configured = Path::new(r"E:\ChromoraData");
        let preferred = Path::new(r"D:\修图软件");
        let fallback = Path::new(r"C:\Users\never\AppData\Local\Chromora");
        let mut seen = Vec::new();
        let selected = select_data_root_with_override(Some(configured), preferred, fallback, |path| {
            seen.push(path.to_path_buf());
            path == configured
        });
        assert_eq!(selected, configured);
        assert_eq!(seen, vec![configured.to_path_buf()]);
    }

    #[test]
    fn unset_override_keeps_preferred_then_localappdata_order() {
        let preferred = Path::new(r"D:\修图软件");
        let fallback = Path::new(r"C:\Users\never\AppData\Local\Chromora");
        assert_eq!(select_data_root_with_override(None, preferred, fallback, |_| true), preferred);
        assert_eq!(select_data_root_with_override(None, preferred, fallback, |_| false), fallback);
    }

    #[test]
    fn unusable_override_falls_through_to_preferred_without_probing_past_it() {
        let configured = Path::new(r"E:\blocked");
        let preferred = Path::new(r"D:\修图软件");
        let fallback = Path::new(r"C:\Users\never\AppData\Local\Chromora");
        let mut seen = Vec::new();
        let selected = select_data_root_with_override(Some(configured), preferred, fallback, |path| {
            seen.push(path.to_path_buf());
            false
        });
        assert_eq!(selected, fallback);
        assert_eq!(seen, vec![configured.to_path_buf(), preferred.to_path_buf()]);
    }

    #[test]
    fn writable_env_directory_is_created_and_the_other_candidates_are_left_alone() {
        let temp = tempfile::tempdir().unwrap();
        let configured = temp.path().join("from-env");
        let preferred = temp.path().join("preferred-should-stay-absent");
        let fallback = temp.path().join("fallback-should-stay-absent");
        let selected = select_data_root_with_override(Some(&configured), &preferred, &fallback, writable_root);
        assert_eq!(selected, configured);
        assert!(configured.join("数据").join("原片").is_dir());
        assert!(configured.join("导出").is_dir());
        assert!(!preferred.exists());
        assert!(!fallback.exists());
    }

    #[test]
    fn unwritable_env_file_falls_back_to_preferred_and_keeps_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let configured = temp.path().join("not-a-directory");
        std::fs::write(&configured, b"keep").unwrap();
        let preferred = temp.path().join("preferred");
        let fallback = temp.path().join("fallback");
        let selected = select_data_root_with_override(Some(&configured), &preferred, &fallback, writable_root);
        assert_eq!(selected, preferred);
        assert!(preferred.join("数据").join("工程").is_dir());
        assert_eq!(std::fs::read(&configured).unwrap(), b"keep");
        assert!(!fallback.exists());
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
