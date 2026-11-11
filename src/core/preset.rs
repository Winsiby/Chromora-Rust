use std::{fs::{self, File, Metadata}, io::{Read, Write}, path::{Path, PathBuf}};

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{AdjustmentSettings, CoreError, CoreResult, PhotoDocument};

const MAX_PRESET_BYTES: u64 = 1024 * 1024;
const MAX_CATALOG_ENTRIES: usize = 4096;
const MAX_CATALOG_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetEntry {
    pub path: PathBuf,
    pub name: String,
    pub portrait: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetFile {
    #[serde(rename = "chromoraPreset")]
    version: u32,
    group: String,
    values: Map<String, Value>,
}

fn portrait_key(key: &str) -> bool {
    matches!(key, "blemishes" | "smoothing" | "skinTone" | "texture" | "skinRegion")
}

/// Loads a bounded, versioned preset into a copy of the current settings.
/// The source preset's portrait selection is validated but never transferred.
pub fn load_preset(current: &AdjustmentSettings, path: &Path, portrait: bool) -> CoreResult<AdjustmentSettings> {
    apply_preset(current, read_preset(path)?, portrait)
}

fn read_preset(path: &Path) -> CoreResult<PresetFile> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_PRESET_BYTES {
        return Err(CoreError::InvalidProject("个人预设超过1MiB限制".into()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PRESET_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PRESET_BYTES {
        return Err(CoreError::InvalidProject("个人预设超过1MiB限制".into()));
    }
    let preset: PresetFile = serde_json::from_slice(&bytes)
        .map_err(|error| CoreError::InvalidProject(format!("个人预设格式无效：{error}")))?;
    if preset.version != 1 {
        return Err(CoreError::InvalidProject(format!("不支持的个人预设版本：{}", preset.version)));
    }
    Ok(preset)
}

fn apply_preset(current: &AdjustmentSettings, preset: PresetFile, portrait: bool) -> CoreResult<AdjustmentSettings> {
    let group = if portrait { "portrait" } else { "landscape" };
    if preset.group != group {
        return Err(CoreError::InvalidProject(format!("预设分组不匹配：此入口需要{group}预设")));
    }
    if !preset.values.keys().any(|key| key != "skinRegion") {
        return Err(CoreError::InvalidProject("个人预设没有可应用的调节项".into()));
    }
    let mut merged = serde_json::to_value(current)?;
    let settings = merged.as_object_mut().expect("AdjustmentSettings serializes as an object");
    for (key, value) in preset.values {
        let known = key == "curves" || key == "skinRegion"
            || AdjustmentSettings::CONTROLS.iter().any(|(name, _, _)| *name == key);
        if !known || portrait_key(&key) != portrait {
            return Err(CoreError::InvalidProject(format!("此预设分组不支持字段：{key}")));
        }
        if key == "skinRegion" {
            // Older saved portrait presets included the old photo's region.
            if !value.is_null() {
                let fields = value.as_object().ok_or_else(|| CoreError::InvalidProject("预设皮肤区域格式无效".into()))?;
                if fields.len() != 4 || fields.keys().any(|key| !matches!(key.as_str(), "x" | "y" | "width" | "height")) {
                    return Err(CoreError::InvalidProject("预设皮肤区域字段无效".into()));
                }
                let mut check = AdjustmentSettings::default();
                check.skin_region = Some(serde_json::from_value(value)?);
                check.validate()?;
            }
        } else {
            settings.insert(key, value);
        }
    }
    let settings: AdjustmentSettings = serde_json::from_value(merged)?;
    super::image::validate_exposure(settings.exposure)?;
    settings.validate()?;
    Ok(settings)
}

fn is_link(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() { return true; }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return true; }
    }
    false
}

/// Lists validated v1 presets, ignoring unrelated or invalid JSON files.
/// A traversal limit returns an error rather than presenting an incomplete list.
pub fn discover_presets(directory: &Path) -> CoreResult<Vec<PresetEntry>> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(CoreError::InvalidProject("个人预设目录必须是非链接目录".into()));
    }
    let mut pending = vec![(directory.to_path_buf(), 0)];
    let mut inspected = 0;
    let mut presets = Vec::new();
    while let Some((directory, depth)) = pending.pop() {
        let mut paths = Vec::new();
        for entry in fs::read_dir(&directory)? {
            inspected += 1;
            if inspected > MAX_CATALOG_ENTRIES {
                return Err(CoreError::InvalidProject("个人预设目录超过4096个目录项，请整理后刷新".into()));
            }
            paths.push(entry?.path());
        }
        paths.sort();
        for path in paths {
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if is_link(&metadata) { continue; }
            if metadata.is_dir() {
                if depth >= MAX_CATALOG_DEPTH {
                    return Err(CoreError::InvalidProject("个人预设目录超过8层，请整理后刷新".into()));
                }
                pending.push((path, depth + 1));
            } else if metadata.is_file()
                && path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("json")) {
                let Ok(preset) = read_preset(&path) else { continue; };
                let portrait = match preset.group.as_str() {
                    "landscape" => false,
                    "portrait" => true,
                    _ => continue,
                };
                if apply_preset(&AdjustmentSettings::default(), preset, portrait).is_err() { continue; }
                let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                presets.push(PresetEntry { path, name, portrait });
            }
        }
    }
    presets.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(presets)
}

/// Copies color edits while preserving capture corrections, LUT choice/strength,
/// and all target-photo regions. The source LUT is deliberately not transferred.
pub fn copy_color_settings(source: &AdjustmentSettings, target: &AdjustmentSettings) -> CoreResult<AdjustmentSettings> {
    source.validate()?;
    target.validate()?;
    super::image::validate_exposure(source.exposure)?;
    super::image::validate_exposure(target.exposure)?;
    let mut copied = target.clone();
    for &(key, _, _) in AdjustmentSettings::CONTROLS {
        if !portrait_key(key) && !matches!(key, "exposure" | "temperature" | "tint" | "lutStrength") {
            copied.set_value(key, source.value(key).expect("known adjustment"))?;
        }
    }
    copied.curves = source.curves.clone();
    copied.validate()?;
    Ok(copied)
}

/// Saves a personal preset without replacing originals, assets, or existing files.
pub fn save_preset(
    documents: &[PhotoDocument],
    current: usize,
    portrait: bool,
    path: &Path,
) -> CoreResult<()> {
    let doc = documents.get(current)
        .ok_or_else(|| CoreError::InvalidProject("没有可保存的调整".into()))?;
    for document in documents {
        super::project::protect_original_destinations(document, path)?;
    }
    super::image::validate_exposure(doc.settings.exposure)?;
    doc.settings.validate()?;
    let mut values = serde_json::to_value(&doc.settings)?;
    if let Some(map) = values.as_object_mut() {
        map.retain(|key, _| {
            !matches!(key.as_str(), "geometry" | "masks") &&
            portrait_key(key) == portrait
        });
    }
    let data = serde_json::json!({
        "chromoraPreset": 1,
        "group": if portrait { "portrait" } else { "landscape" },
        "values": values,
    });
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, &data)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path).map_err(|error| CoreError::Io(error.error))?;
    Ok(())
}
