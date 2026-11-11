use std::{fs, path::{Path, PathBuf}};
use chromora::core::{self, ColorPipeline, CoreError, ProjectHistory, WorkspaceDocument};
use serde_json::{Value, json};

fn fixture(root: &Path) -> PathBuf {
    let path = root.join("ICC 原片.png");
    fs::write(&path, include_bytes!("../fixtures/color06/legacy-srgb8.png")).unwrap();
    path
}

fn baseline() -> image::RgbImage {
    image::load_from_memory(include_bytes!("../fixtures/color06/legacy-srgb8-preview.png")).unwrap().to_rgb8()
}

fn save_value(doc: core::PhotoDocument, path: &Path) -> Value {
    core::save_project(&doc, path).unwrap();
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn old_value(doc: &core::PhotoDocument, current: &Value, version: u32) -> Value {
    if version == 1 {
        json!({"schema_version":1,"source_path":doc.source_path,
            "original_copy_path":doc.original_copy_path,"source_sha256":doc.source_sha256,
            "width":doc.width(),"height":doc.height(),"exposure_ev":0.0})
    } else {
        let mut value = current.clone();
        value["schema_version"] = json!(version);
        value["render_version"] = json!(if version == 2 { "chromora-0.3-srgb-v1" } else { "chromora-0.4-srgb-v1" });
        value["photos"][0].as_object_mut().unwrap().remove("color_pipeline");
        value
    }
}

#[test]
fn schemas_one_to_four_keep_frozen_05_pixels_and_require_another_save_path() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let legacy = core::open_image_with_pipeline(&source, ColorPipeline::LegacySrgb).unwrap();
    let current = save_value(legacy.clone(), &temp.path().join("template.chromora"));
    let expected = baseline();
    assert_eq!(core::render_preview(&legacy).unwrap(), expected);
    for version in 1..=4 {
        let original_path = temp.path().join(format!("schema{version}.chromora"));
        let value = old_value(&legacy, &current, version);
        let before = serde_json::to_vec_pretty(&value).unwrap();
        fs::write(&original_path, &before).unwrap();
        let mut loaded = core::load_workspace_with_history(&original_path).unwrap();
        assert!(loaded.requires_save_as);
        let doc = &mut loaded.workspace.photos[0];
        assert_eq!(doc.color_pipeline(), ColorPipeline::LegacySrgb);
        assert_eq!(core::render_preview(doc).unwrap(), expected);
        doc.evict_pixels();
        assert_eq!(core::render_preview(doc).unwrap(), expected);
        core::hydrate_document(doc).unwrap();
        for (sample, pixel) in doc.working_image().pixels.as_raw().iter().zip(expected.as_raw()) {
            assert_eq!(*sample, f32::from(*pixel) / 255.0);
        }
        assert!(matches!(core::save_workspace(&loaded.workspace, &original_path), Err(CoreError::InvalidProject(_))));
        assert_eq!(fs::read(&original_path).unwrap(), before);
        let new_path = temp.path().join(format!("schema{version}-0.7.chromora"));
        core::save_workspace(&loaded.workspace, &new_path).unwrap();
        let reopened = core::load_workspace_with_history(&new_path).unwrap();
        assert!(!reopened.requires_save_as);
        assert_eq!(reopened.workspace.photos[0].color_pipeline(), ColorPipeline::LegacySrgb);
        assert_eq!(core::render_preview(&reopened.workspace.photos[0]).unwrap(), expected);
    }
}

#[test]
fn mixed_schema_seven_keeps_all_four_pipelines_through_history_reopen_and_eviction() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let legacy = core::open_image_with_pipeline(&source, ColorPipeline::LegacySrgb).unwrap();
    let hq = core::open_image_with_pipeline(&source, ColorPipeline::HqSrgb).unwrap();
    let accurate = core::open_image_with_pipeline(&source, ColorPipeline::AccurateSrgb).unwrap();
    let float = core::open_image(&source).unwrap();
    assert_eq!(float.color_pipeline(), ColorPipeline::FloatSrgb);
    assert_ne!(legacy.working_image().pixels.as_raw(), hq.working_image().pixels.as_raw());
    let expected = [&legacy, &hq, &accurate, &float].map(|doc| core::render_rgb_f32(doc, None).unwrap());
    let path = temp.path().join("混合管线.chromora");
    let workspace = WorkspaceDocument { name:"mixed".into(),photos:vec![legacy,hq,accurate,float],current_index:0 };
    let history = ProjectHistory { undo:vec![core::ProjectEditState { settings:Default::default(),lut_path:None,lut_sha256:None }],redo:vec![] };
    core::save_workspace_with_history(&workspace, &[history.clone(),history.clone(),history.clone(),history], &path).unwrap();
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 7);
    assert_eq!(value["render_version"], "chromora-0.8-srgb-v4");
    assert_eq!(value["photos"][0]["color_pipeline"], "legacy_srgb");
    assert_eq!(value["photos"][1]["color_pipeline"], "hq_srgb");
    assert_eq!(value["photos"][2]["color_pipeline"], "accurate_srgb");
    assert_eq!(value["photos"][3]["color_pipeline"], "float_srgb");
    let mut loaded = core::load_workspace_with_history(&path).unwrap();
    assert!(!loaded.requires_save_as && loaded.history_warnings.is_empty());
    assert_eq!(loaded.histories[0].undo.len(), 1);
    assert_eq!(loaded.histories[1].undo.len(), 1);
    assert_eq!(loaded.histories[2].undo.len(), 1);
    assert_eq!(loaded.histories[3].undo.len(), 1);
    assert_eq!(loaded.workspace.photos[1].working_image().pixels.width(), 0);
    for (index, pipeline) in [ColorPipeline::LegacySrgb, ColorPipeline::HqSrgb, ColorPipeline::AccurateSrgb, ColorPipeline::FloatSrgb].into_iter().enumerate() {
        let doc = &mut loaded.workspace.photos[index];
        assert_eq!(doc.color_pipeline(), pipeline);
        assert_eq!(core::render_rgb_f32(doc, None).unwrap(), expected[index]);
        doc.evict_pixels();
        assert_eq!(core::render_rgb_f32(doc, None).unwrap(), expected[index]);
        core::hydrate_document(doc).unwrap();
        assert_eq!(core::render_rgb_f32(doc, None).unwrap(), expected[index]);
    }
    core::save_workspace_with_history(&loaded.workspace, &loaded.histories, &path).unwrap();
}

#[test]
fn schema_five_preserves_06_pixels_and_history_but_cannot_claim_the_07_pipeline() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let pipelines = [ColorPipeline::LegacySrgb, ColorPipeline::HqSrgb];
    let photos: Vec<_> = pipelines.into_iter().map(|p| core::open_image_with_pipeline(&source, p).unwrap()).collect();
    let expected: Vec<_> = photos.iter().map(|doc| core::render_rgb_f32(doc, None).unwrap()).collect();
    let workspace = WorkspaceDocument { name:"schema5".into(), photos, current_index:0 };
    let history = ProjectHistory { undo:vec![core::ProjectEditState { settings:Default::default(), lut_path:None, lut_sha256:None }], redo:vec![] };
    let template = temp.path().join("template.chromora");
    core::save_workspace_with_history(&workspace, &[history.clone(), history], &template).unwrap();
    let mut value: Value = serde_json::from_slice(&fs::read(&template).unwrap()).unwrap();
    value["schema_version"] = json!(5);
    value["render_version"] = json!("chromora-0.6-srgb-v2");
    let path = temp.path().join("original-0.6.chromora");
    let before = serde_json::to_vec_pretty(&value).unwrap();
    fs::write(&path, &before).unwrap();
    let mut loaded = core::load_workspace_with_history(&path).unwrap();
    assert!(loaded.requires_save_as && loaded.history_warnings.is_empty());
    for (index, pipeline) in pipelines.into_iter().enumerate() {
        let doc = &mut loaded.workspace.photos[index];
        assert_eq!(doc.color_pipeline(), pipeline);
        assert_eq!(loaded.histories[index].undo.len(), 1);
        assert_eq!(core::render_rgb_f32(doc, None).unwrap(), expected[index]);
        doc.evict_pixels();
        core::hydrate_document(doc).unwrap();
        assert_eq!(core::render_rgb_f32(doc, None).unwrap(), expected[index]);
    }
    assert!(matches!(core::save_workspace_with_history(&loaded.workspace, &loaded.histories, &path), Err(CoreError::InvalidProject(_))));
    assert_eq!(fs::read(&path).unwrap(), before);
    let upgraded = temp.path().join("saved-0.7.chromora");
    core::save_workspace_with_history(&loaded.workspace, &loaded.histories, &upgraded).unwrap();
    let reopened = core::load_workspace_with_history(&upgraded).unwrap();
    assert!(!reopened.requires_save_as);
    for (index, pipeline) in pipelines.into_iter().enumerate() {
        assert_eq!(reopened.workspace.photos[index].color_pipeline(), pipeline);
        assert_eq!(core::render_rgb_f32(&reopened.workspace.photos[index], None).unwrap(), expected[index]);
        assert_eq!(reopened.histories[index].undo.len(), 1);
    }
    let bad_path = temp.path().join("tampered-schema5.chromora");
    for pipeline in [Value::Null, json!("accurate_srgb"), json!("float_srgb"), json!("not_a_pipeline")] {
        let mut bad = value.clone(); bad["photos"][0]["color_pipeline"] = pipeline;
        fs::write(&bad_path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(core::load_workspace(&bad_path).is_err());
    }
    value["photos"][0].as_object_mut().unwrap().remove("color_pipeline");
    fs::write(&bad_path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(core::load_workspace(&bad_path), Err(CoreError::InvalidProject(_))));
}

#[test]
fn schema_six_keeps_frozen_07_clarity_for_all_old_pipelines_and_requires_save_as() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("均匀16位灰.png");
    fs::write(&source, include_bytes!("../fixtures/color08/uniform-rgb16.png")).unwrap();
    let pipelines = [ColorPipeline::LegacySrgb, ColorPipeline::HqSrgb, ColorPipeline::AccurateSrgb];
    let photos: Vec<_> = pipelines.into_iter().map(|pipeline| {
        let mut doc = core::open_image_with_pipeline(&source, pipeline).unwrap();
        let mut settings = doc.settings.clone(); settings.clarity = 100.;
        core::set_settings(&mut doc, settings).unwrap();
        doc
    }).collect();
    let expected: Vec<_> = photos.iter().map(|doc| core::render_rgb_f32(doc, None).unwrap()).collect();
    // Frozen 0.7 Debug-core uniform-field result, recorded before the 0.8 change.
    for image in &expected {
        assert!(image.as_raw().iter().all(|v| v.to_bits() == 0.014895857_f32.to_bits()));
    }
    let workspace = WorkspaceDocument { name:"schema6".into(), photos, current_index:0 };
    let history = ProjectHistory { undo:vec![core::ProjectEditState { settings:Default::default(), lut_path:None, lut_sha256:None }], redo:vec![] };
    let template = temp.path().join("template.chromora");
    core::save_workspace_with_history(&workspace, &[history.clone(),history.clone(),history], &template).unwrap();
    let mut value: Value = serde_json::from_slice(&fs::read(&template).unwrap()).unwrap();
    value["schema_version"] = json!(6);
    value["render_version"] = json!("chromora-0.7-srgb-v3");
    let path = temp.path().join("original-0.7.chromora");
    let before = serde_json::to_vec_pretty(&value).unwrap(); fs::write(&path, &before).unwrap();
    let mut loaded = core::load_workspace_with_history(&path).unwrap();
    assert!(loaded.requires_save_as && loaded.history_warnings.is_empty());
    for (index, pipeline) in pipelines.into_iter().enumerate() {
        assert_eq!(loaded.histories[index].undo.len(), 1);
        let doc = &mut loaded.workspace.photos[index];
        assert_eq!(doc.color_pipeline(), pipeline);
        assert_eq!(core::render_rgb_f32(doc, None).unwrap(), expected[index]);
        doc.evict_pixels();
        assert_eq!(core::render_rgb_f32(doc, None).unwrap(), expected[index]);
        core::hydrate_document(doc).unwrap();
        assert_eq!(core::render_rgb_f32(doc, None).unwrap(), expected[index]);
    }
    assert!(matches!(core::save_workspace_with_history(&loaded.workspace, &loaded.histories, &path), Err(CoreError::InvalidProject(_))));
    assert_eq!(fs::read(&path).unwrap(), before);
    let saved = temp.path().join("saved-0.8.chromora");
    core::save_workspace_with_history(&loaded.workspace, &loaded.histories, &saved).unwrap();
    let reopened = core::load_workspace_with_history(&saved).unwrap();
    assert!(!reopened.requires_save_as);
    for index in 0..pipelines.len() {
        assert_eq!(reopened.workspace.photos[index].color_pipeline(), pipelines[index]);
        assert_eq!(core::render_rgb_f32(&reopened.workspace.photos[index], None).unwrap(), expected[index]);
        assert_eq!(reopened.histories[index].undo.len(), 1);
    }
    let bad_path = temp.path().join("tampered-schema6.chromora");
    for pipeline in [Value::Null, json!("float_srgb"), json!("not_a_pipeline")] {
        let mut bad = value.clone(); bad["photos"][0]["color_pipeline"] = pipeline;
        fs::write(&bad_path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(core::load_workspace(&bad_path).is_err());
    }
    value["photos"][0].as_object_mut().unwrap().remove("color_pipeline");
    fs::write(&bad_path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(core::load_workspace(&bad_path), Err(CoreError::InvalidProject(_))));
}

#[test]
fn current_schema_requires_explicit_known_pipeline_and_matching_renderer() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let value = save_value(core::open_image(&source).unwrap(), &temp.path().join("good.chromora"));
    let path = temp.path().join("bad.chromora");
    for pipeline in [Value::Null, json!("not_a_pipeline")] {
        let mut bad = value.clone(); bad["photos"][0]["color_pipeline"] = pipeline;
        fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(core::load_project(&path).is_err());
    }
    let mut missing = value.clone(); missing["photos"][0].as_object_mut().unwrap().remove("color_pipeline");
    fs::write(&path, serde_json::to_vec(&missing).unwrap()).unwrap();
    assert!(matches!(core::load_project(&path), Err(CoreError::InvalidProject(_))));
    let mut wrong_renderer = value.clone(); wrong_renderer["render_version"] = json!("chromora-0.4-srgb-v1");
    fs::write(&path, serde_json::to_vec(&wrong_renderer).unwrap()).unwrap();
    assert!(matches!(core::load_project(&path), Err(CoreError::InvalidProject(_))));
    let mut unknown_schema = value; unknown_schema["schema_version"] = json!(8);
    fs::write(&path, serde_json::to_vec(&unknown_schema).unwrap()).unwrap();
    assert!(matches!(core::load_project(&path), Err(CoreError::UnsupportedProjectVersion(8))));
}

#[test]
fn legacy_schema_cannot_silently_opt_into_a_new_pipeline_field() {
    let temp = tempfile::tempdir().unwrap(); let source = fixture(temp.path());
    let legacy = core::open_image_with_pipeline(&source, ColorPipeline::LegacySrgb).unwrap();
    let value = save_value(legacy.clone(), &temp.path().join("current.chromora"));
    let mut old = old_value(&legacy, &value, 4);
    old["photos"][0]["color_pipeline"] = json!("hq_srgb");
    let path = temp.path().join("old.chromora");
    fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let loaded = core::load_project(&path).unwrap();
    assert_eq!(loaded.color_pipeline(), ColorPipeline::LegacySrgb);
    assert_eq!(core::render_preview(&loaded).unwrap(), baseline());
}

#[test]
fn legacy_bad_icc_remains_openable_after_new_format_save_and_eviction() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("旧损坏ICC.jpg");
    fs::write(&source, include_bytes!("../fixtures/invalid_icc_reference.jpg")).unwrap();
    assert!(matches!(core::open_image(&source), Err(CoreError::InvalidIcc { .. })));
    let legacy = core::open_image_with_pipeline(&source, ColorPipeline::LegacySrgb).unwrap();
    let expected = core::render_rgb_f32(&legacy, None).unwrap();
    let current = save_value(legacy.clone(), &temp.path().join("current.chromora"));
    let path = temp.path().join("old.chromora");
    fs::write(&path, serde_json::to_vec(&old_value(&legacy, &current, 4)).unwrap()).unwrap();
    let loaded = core::load_workspace(&path).unwrap();
    let saved = temp.path().join("old-0.7.chromora");
    core::save_workspace(&loaded, &saved).unwrap();
    let mut reopened = core::load_project(&saved).unwrap();
    assert_eq!(reopened.color_pipeline(), ColorPipeline::LegacySrgb);
    assert_eq!(core::render_rgb_f32(&reopened, None).unwrap(), expected);
    reopened.evict_pixels();
    assert_eq!(core::render_rgb_f32(&reopened, None).unwrap(), expected);
}
