use std::{fs, io::Write, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
use super::{AdjustmentSettings, ColorPipeline, CoreError, CoreResult, PhotoDocument, RenderCancellation, open_image_with_pipeline, set_settings, set_lut};

const CURRENT_RENDER_VERSION: &str = "chromora-0.8-srgb-v4";
pub const MAX_WORKSPACE_PHOTOS: usize = 10_000;

#[derive(Clone, Debug)]
pub struct WorkspaceDocument {
    pub name: String,
    pub photos: Vec<PhotoDocument>,
    pub current_index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectEditState {
    pub settings: AdjustmentSettings,
    pub lut_path: Option<PathBuf>,
    pub lut_sha256: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectHistory {
    pub undo: Vec<ProjectEditState>,
    pub redo: Vec<ProjectEditState>,
}

#[derive(Clone, Debug)]
pub struct LoadedWorkspace {
    pub workspace: WorkspaceDocument,
    pub histories: Vec<ProjectHistory>,
    pub history_warnings: Vec<String>,
    pub requires_save_as: bool,
}

#[derive(Serialize, Deserialize)]
struct SavedPhoto {
    source_path: PathBuf,
    original_copy_path: Option<PathBuf>,
    source_sha256: String,
    width: u32,
    height: u32,
    #[serde(default)] settings: AdjustmentSettings,
    #[serde(default)] lut_path: Option<PathBuf>,
    #[serde(default)] lut_sha256: Option<String>,
    #[serde(default)] render_source_path: Option<PathBuf>,
    #[serde(default)] render_source_sha256: Option<String>,
    #[serde(default)] color_pipeline: Option<ColorPipeline>,
}

#[derive(Serialize, Deserialize)]
struct SavedWorkspace {
    schema_version: u32,
    render_version: String,
    name: String,
    current_index: usize,
    photos: Vec<SavedPhoto>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    history: serde_json::Value,
}

pub fn save_workspace(workspace: &WorkspaceDocument, path: &Path) -> CoreResult<()> {
    save_workspace_internal(workspace, None, path).map(|_| ())
}

pub fn save_workspace_with_history(workspace: &WorkspaceDocument, histories: &[ProjectHistory], path: &Path) -> CoreResult<Vec<String>> {
    save_workspace_internal(workspace, Some(histories), path)
}

fn save_workspace_internal(workspace: &WorkspaceDocument, histories: Option<&[ProjectHistory]>, path: &Path) -> CoreResult<Vec<String>> {
    validate_selection(workspace.photos.len(), workspace.current_index)?;
    if histories.is_some_and(|entries| entries.len() != workspace.photos.len()) {
        return Err(CoreError::InvalidProject("照片与撤销历史数量不一致".into()));
    }
    for doc in &workspace.photos { super::project::protect_original_destinations(doc, path)?; }
    if let Some(histories) = histories {
        for (doc, history) in workspace.photos.iter().zip(histories) {
            for state in history.undo.iter().chain(&history.redo) {
                if state.lut_path.is_some() {
                    let mut protected = doc.clone(); protected.lut_path = state.lut_path.clone();
                    super::project::protect_original_destinations(&protected, path)?;
                }
            }
        }
    }
    // Keep older files readable by their original application. Saving this
    // format at the same path would silently replace them with schema7.
    if path.is_file() && fs::metadata(path)?.len() <= 64 * 1024 * 1024 {
        if let Ok(old) = serde_json::from_slice::<serde_json::Value>(&fs::read(path)?) {
            if (old.get("state").is_some() && old.get("schema_version").is_none())
                || old.get("schema_version").and_then(|v| v.as_u64()).is_some_and(|v| (1..=6).contains(&v)) {
                return Err(CoreError::InvalidProject("旧版工程请另存为新的 .chromora 文件".into()));
            }
        }
    }
    let photos = workspace.photos.iter().map(|doc| {
        verified_path(&doc.source_path, doc.original_copy_path.as_deref(), &doc.source_sha256)?;
        let lut_sha256 = match (&doc.lut_path, &doc.lut) {
            (Some(path), Some(lut)) => { verified_path(path, None, &lut.sha256)?; Some(lut.sha256.clone()) },
            (None, None) => None,
            _ => return Err(CoreError::InvalidProject("风格引用与当前采样表不一致".into())),
        };
        let render_source_sha256 = match (&doc.render_source_path, &doc.render_source_sha256) {
            (Some(path), Some(hash)) => { verified_path(path, None, hash)?; Some(hash.clone()) },
            (None, None) => None,
            _ => return Err(CoreError::InvalidProject("显影缓存缺少完整校验信息".into())),
        };
        let mut settings = doc.settings.clone();
        settings.exposure = doc.exposure_ev;
        settings.validate()?;
        Ok(SavedPhoto {
            source_path: doc.source_path.clone(), original_copy_path: doc.original_copy_path.clone(),
            source_sha256: doc.source_sha256.clone(), width: doc.width(), height: doc.height(), settings,
            lut_path: doc.lut_path.clone(), lut_sha256,
            render_source_path: doc.render_source_path.clone(),
            render_source_sha256,
            color_pipeline: Some(doc.color_pipeline()),
        })
    }).collect::<CoreResult<Vec<_>>>()?;
    let mut warnings = Vec::new();
    let history = if let Some(histories) = histories {
        let mut saved = Vec::with_capacity(histories.len());
        for (index, history) in histories.iter().enumerate() {
            match validate_history(history) {
                Ok(()) => saved.push(history.clone()),
                Err(error) => {
                    saved.push(ProjectHistory::default());
                    warnings.push(format!("第{}张照片的撤销历史未保存：{error}；当前调整已保留", index + 1));
                }
            }
        }
        serde_json::to_value(saved)?
    } else { serde_json::Value::Null };
    let saved = SavedWorkspace { schema_version: super::SCHEMA_VERSION, render_version: CURRENT_RENDER_VERSION.into(),
        name: workspace.name.clone(), current_index: workspace.current_index, photos, history };
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, &saved)?;
    temp.flush()?;
    if temp.as_file().metadata()?.len() > 64 * 1024 * 1024 {
        return Err(CoreError::InvalidProject("工程及历史超过64MB，请减少照片或历史后另存".into()));
    }
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| CoreError::Io(e.error))?;
    Ok(warnings)
}

fn validate_history(history: &ProjectHistory) -> CoreResult<()> {
    if history.undo.len() + history.redo.len() > 100 {
        return Err(CoreError::InvalidProject("单张照片的撤销及重做历史总计不能超过100步".into()));
    }
    let mut verified = std::collections::HashSet::new();
    for state in history.undo.iter().chain(&history.redo) {
        super::image::validate_exposure(state.settings.exposure)?;
        state.settings.validate()?;
        match (&state.lut_path, &state.lut_sha256) {
            (Some(path), Some(hash)) => {
                if verified.insert((path, hash)) { verified_path(path, None, hash)?; }
            }
            (None, None) => {},
            _ => return Err(CoreError::InvalidProject("历史风格缺少路径或校验信息".into())),
        }
    }
    Ok(())
}

fn validate_selection(count: usize, current: usize) -> CoreResult<()> {
    if count == 0 || count > MAX_WORKSPACE_PHOTOS || current >= count {
        return Err(CoreError::InvalidProject("照片列表或当前照片索引无效".into()));
    }
    Ok(())
}

fn verified_path(source: &Path, copy: Option<&Path>, hash: &str) -> CoreResult<PathBuf> {
    for p in copy.into_iter().chain(std::iter::once(source)) {
        if p.is_file() && super::image::sha256_file(p).is_ok_and(|actual| actual == hash) { return Ok(p.to_path_buf()); }
    }
    Err(CoreError::MissingOrChangedSource(source.to_path_buf()))
}

fn restore(saved: SavedPhoto) -> CoreResult<PhotoDocument> {
    let original = verified_path(&saved.source_path, saved.original_copy_path.as_deref(), &saved.source_sha256)?;
    let decode_path = match (&saved.render_source_path, &saved.render_source_sha256) {
        (Some(path), Some(hash)) => verified_path(path, None, hash)?,
        (None, None) => original,
        _ => return Err(CoreError::InvalidProject("显影缓存缺少完整校验信息".into())),
    };
    let mut doc = open_image_with_pipeline(&decode_path, saved.color_pipeline.unwrap_or(ColorPipeline::LegacySrgb))?;
    if doc.width() != saved.width || doc.height() != saved.height {
        return Err(CoreError::MissingOrChangedSource(decode_path));
    }
    doc.source_path = saved.source_path;
    doc.original_copy_path = saved.original_copy_path;
    doc.source_sha256 = saved.source_sha256;
    doc.render_source_path = saved.render_source_path;
    doc.render_source_sha256 = saved.render_source_sha256;
    set_settings(&mut doc, saved.settings)?;
    if let Some(path) = saved.lut_path {
        let hash = saved.lut_sha256.ok_or_else(|| CoreError::InvalidProject("风格缺少校验信息".into()))?;
        verified_path(&path, None, &hash)?;
        set_lut(&mut doc, Some(&path))?;
    }
    Ok(doc)
}

pub fn load_workspace(path: &Path) -> CoreResult<WorkspaceDocument> {
    Ok(load_workspace_with_history(path)?.workspace)
}

pub fn load_workspace_with_history(path: &Path) -> CoreResult<LoadedWorkspace> {
    load_workspace_with_progress(path, &RenderCancellation::new(), |_, _, _| {})
}

pub fn load_workspace_with_progress(path: &Path, cancel: &RenderCancellation,
    mut progress: impl FnMut(usize, usize, &Path)) -> CoreResult<LoadedWorkspace> {
    cancel.check()?;
    if fs::metadata(path)?.len() > 64 * 1024 * 1024 {
        return Err(CoreError::InvalidProject("工程文件超过 64MB".into()));
    }
    let mut value: serde_json::Value = serde_json::from_reader(fs::File::open(path)?)?;
    let version = value.get("schema_version").and_then(|v| v.as_u64());
    let history = value.as_object_mut().and_then(|v| v.remove("history")).unwrap_or(serde_json::Value::Null);
    cancel.check()?;
    let workspace = decode_workspace(value, path, cancel, &mut progress)?;
    let mut histories = vec![ProjectHistory::default(); workspace.photos.len()];
    let mut history_warnings = Vec::new();
    if matches!(version, Some(4) | Some(5) | Some(6) | Some(7)) && !history.is_null() {
        if let Some(entries) = history.as_array().filter(|v| v.len() == histories.len()) {
            for (index, entry) in entries.iter().enumerate() {
                cancel.check()?;
                match serde_json::from_value::<ProjectHistory>(entry.clone()).map_err(CoreError::from)
                    .and_then(|entry| { validate_history(&entry)?; Ok(entry) }) {
                    Ok(history) => histories[index] = history,
                    Err(error) => history_warnings.push(format!("第{}张照片的撤销历史已忽略：{error}；当前调整已保留", index + 1)),
                }
            }
        } else {
            history_warnings.push("撤销历史结构或照片数量无效，已保留全部当前调整并忽略历史".into());
        }
    }
    cancel.check()?;
    Ok(LoadedWorkspace { workspace, histories, history_warnings, requires_save_as: version != Some(7) })
}

fn decode_workspace(mut value: serde_json::Value, path: &Path, cancel: &RenderCancellation,
    progress: &mut impl FnMut(usize, usize, &Path)) -> CoreResult<WorkspaceDocument> {
    if value.get("state").is_some() && value.get("schema_version").is_none() { return load_legacy(value, path, cancel, progress); }
    let version = value.get("schema_version").and_then(|v| v.as_u64()).and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| CoreError::InvalidProject("缺少有效 schema_version".into()))?;
    if version == 1 {
        let old: super::document::ProjectFile = serde_json::from_value(value)?;
        cancel.check()?;
        progress(0, 1, &old.source_path);
        super::image::validate_exposure(old.exposure_ev)?;
        let mut settings = AdjustmentSettings::default(); settings.exposure = old.exposure_ev;
        let doc = restore(SavedPhoto { source_path: old.source_path, original_copy_path: old.original_copy_path,
            source_sha256: old.source_sha256, width: old.width, height: old.height, settings,
            lut_path: None, lut_sha256: None, render_source_path: None, render_source_sha256: None, color_pipeline: None })?;
        cancel.check()?;
        progress(1, 1, &doc.source_path);
        cancel.check()?;
        return Ok(WorkspaceDocument { name: path.file_stem().unwrap_or_default().to_string_lossy().into(), photos: vec![doc], current_index: 0 });
    }
    if !(2..=7).contains(&version) { return Err(CoreError::UnsupportedProjectVersion(version)); }
    if version < 5 {
        // Older schemas never carried this meaning. Ignore an extra field,
        // rather than letting it upgrade an old image's rendering semantics.
        if let Some(photos) = value.get_mut("photos").and_then(|v| v.as_array_mut()) {
            for photo in photos { if let Some(photo) = photo.as_object_mut() { photo.remove("color_pipeline"); } }
        }
    }
    let saved: SavedWorkspace = serde_json::from_value(value)?;
    let expected_renderer = match version {
        2 => "chromora-0.3-srgb-v1", 3 | 4 => "chromora-0.4-srgb-v1",
        5 => "chromora-0.6-srgb-v2", 6 => "chromora-0.7-srgb-v3", _ => CURRENT_RENDER_VERSION,
    };
    if saved.render_version != expected_renderer { return Err(CoreError::InvalidProject("不支持的渲染版本".into())); }
    if version >= 5 && saved.photos.iter().any(|p| p.color_pipeline.is_none()) {
        return Err(CoreError::InvalidProject("新版工程缺少照片的色彩处理版本".into()));
    }
    if version == 5 && saved.photos.iter().any(|p| matches!(p.color_pipeline, Some(ColorPipeline::AccurateSrgb | ColorPipeline::FloatSrgb))) {
        return Err(CoreError::InvalidProject("schema5 工程不能包含 0.7 色彩处理版本".into()));
    }
    if version == 6 && saved.photos.iter().any(|p| p.color_pipeline == Some(ColorPipeline::FloatSrgb)) {
        return Err(CoreError::InvalidProject("schema6 工程不能包含 0.8 浮点滤镜处理版本".into()));
    }
    if version == 2 && saved.photos.iter().any(|p| p.settings.geometry != Default::default() || !p.settings.masks.is_empty()) {
        return Err(CoreError::InvalidProject("旧版本工程不能包含新构图或蒙版参数".into()));
    }
    validate_selection(saved.photos.len(), saved.current_index)?;
    let total = saved.photos.len();
    let mut photos = Vec::with_capacity(saved.photos.len());
    for (index, photo) in saved.photos.into_iter().enumerate() {
        cancel.check()?;
        progress(index, total, &photo.source_path);
        cancel.check()?;
        let mut doc = restore(photo)?;
        cancel.check()?;
        super::prepare_thumbnail(&mut doc)?;
        cancel.check()?;
        if index != saved.current_index { doc.evict_pixels(); }
        progress(index + 1, total, &doc.source_path);
        cancel.check()?;
        photos.push(doc);
    }
    Ok(WorkspaceDocument { name: saved.name, photos, current_index: saved.current_index })
}

fn safe_id(id: &str, has_extension: bool) -> bool {
    let hash = id.split('.').next().unwrap_or_default();
    hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()) &&
        if has_extension { id.len() <= 70 && id[64..].starts_with('.') && id[65..].bytes().all(|c| c.is_ascii_alphanumeric()) } else { id.len() == 64 }
}

fn load_legacy(value: serde_json::Value, project_path: &Path, cancel: &RenderCancellation,
    progress: &mut impl FnMut(usize, usize, &Path)) -> CoreResult<WorkspaceDocument> {
    let state = &value["state"];
    let entries = state["photos"].as_array().ok_or_else(|| CoreError::InvalidProject("旧版照片列表无效".into()))?;
    let current_id = state["currentId"].as_str();
    let current_index = entries.iter().position(|p| p["id"].as_str() == current_id).unwrap_or(0);
    validate_selection(entries.len(), current_index)?;
    let data = project_path.parent().and_then(Path::parent).ok_or_else(|| CoreError::InvalidProject("旧工程应位于 数据/工程 目录下".into()))?;
    let mut photos = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        cancel.check()?;
        let id = entry["id"].as_str().filter(|id| safe_id(id, true)).ok_or_else(|| CoreError::InvalidProject("旧版照片标识无效".into()))?;
        let source = data.join("原片").join(id);
        progress(index, entries.len(), &source);
        cancel.check()?;
        let expected_hash = &id[..64];
        verified_path(&source, None, expected_hash)?;
        let cache = data.join("RAW显影").join(format!("{id}.tif"));
        let use_cache = entry["raw"].as_bool() == Some(true) && cache.is_file();
        let mut doc = open_image_with_pipeline(if use_cache { &cache } else { &source }, ColorPipeline::LegacySrgb)?;
        doc.source_path = source.clone(); doc.original_copy_path = Some(source);
        doc.source_sha256 = expected_hash.to_string();
        doc.render_source_path = if use_cache { Some(cache) } else { None };
        doc.render_source_sha256 = doc.render_source_path.as_deref().map(super::image::sha256_file).transpose()?;
        let settings: AdjustmentSettings = serde_json::from_value(entry.get("settings").cloned().unwrap_or_else(|| serde_json::json!({})))?;
        set_settings(&mut doc, settings)?;
        if let Some(id) = entry["lutId"].as_str() {
            if !safe_id(id, false) { return Err(CoreError::InvalidProject("旧版风格标识无效".into())); }
            let path = data.join("风格").join(format!("{id}.cube"));
            verified_path(&path, None, id)?;
            set_lut(&mut doc, Some(&path))?;
        }
        super::prepare_thumbnail(&mut doc)?;
        cancel.check()?;
        if index != current_index { doc.evict_pixels(); }
        progress(index + 1, entries.len(), &doc.source_path);
        cancel.check()?;
        photos.push(doc);
    }
    Ok(WorkspaceDocument { name: value["name"].as_str().unwrap_or("旧版工程").into(), photos, current_index })
}
