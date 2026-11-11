use std::{fs, path::Path};
use chromora::core::{self, AdjustmentSettings, CoreError, ProjectEditState, ProjectHistory, WorkspaceDocument};
use image::{Rgb, RgbImage};

fn fixture(path: &Path) -> core::PhotoDocument {
    RgbImage::from_pixel(4, 3, Rgb([40, 80, 120])).save(path).unwrap();
    let mut doc = core::open_image(path).unwrap(); core::set_exposure(&mut doc, 0.5).unwrap(); doc
}
fn state(ev: f32) -> ProjectEditState {
    ProjectEditState { settings: AdjustmentSettings { exposure: ev, ..Default::default() }, lut_path: None, lut_sha256: None }
}
fn cube(path: &Path) -> ProjectEditState {
    fs::write(path, "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
    let lut = core::load_cube(path).unwrap();
    ProjectEditState { lut_path: Some(path.to_path_buf()), lut_sha256: Some(lut.sha256), ..state(0.0) }
}
#[test]
fn history_round_trip_keeps_current_pixels_and_both_stacks_without_pixel_snapshots() {
    let temp = tempfile::tempdir().unwrap(); let doc = fixture(&temp.path().join("原片.png"));
    let expected = core::render_preview(&doc).unwrap();
    let workspace = WorkspaceDocument { name: "历史".into(), photos: vec![doc], current_index: 0 };
    let history = ProjectHistory { undo: vec![state(0.0), state(0.25)], redo: vec![state(0.75)] };
    let path = temp.path().join("history.chromora");
    assert!(core::save_workspace_with_history(&workspace, &[history], &path).unwrap().is_empty());
    let loaded = core::load_workspace_with_history(&path).unwrap();
    assert!(loaded.history_warnings.is_empty());
    assert_eq!(loaded.histories[0].undo.len(), 2); assert_eq!(loaded.histories[0].redo[0].settings.exposure, 0.75);
    assert_eq!(core::render_preview(&loaded.workspace.photos[0]).unwrap(), expected);
    let json: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(json["schema_version"], 7);
    let keys = json["history"][0]["undo"][0].as_object().unwrap();
    assert_eq!(keys.len(), 3); assert!(keys.contains_key("settings") && keys.contains_key("lut_path") && keys.contains_key("lut_sha256"));
}

#[test]
fn malformed_history_never_discards_valid_current_adjustments() {
    let temp = tempfile::tempdir().unwrap(); let doc = fixture(&temp.path().join("original.png"));
    let expected = core::render_preview(&doc).unwrap(); let path = temp.path().join("history.chromora");
    core::save_workspace_with_history(&WorkspaceDocument { name: "history".into(), photos: vec![doc], current_index: 0 }, &[ProjectHistory::default()], &path).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for bad in [serde_json::json!("not an array"), serde_json::json!([{"undo":[{"settings":"invalid"}],"redo":[]}])] {
        let mut json = original.clone(); json["history"] = bad;
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        let loaded = core::load_workspace_with_history(&path).unwrap();
        assert!(!loaded.history_warnings.is_empty()); assert!(loaded.histories[0].undo.is_empty());
        assert_eq!(loaded.workspace.photos[0].exposure_ev, 0.5);
        assert_eq!(core::render_preview(&loaded.workspace.photos[0]).unwrap(), expected);
    }
}

#[test]
fn changed_historical_lut_clears_only_history_on_load_and_save() {
    let temp = tempfile::tempdir().unwrap(); let doc = fixture(&temp.path().join("original.png"));
    let workspace = WorkspaceDocument { name: "history".into(), photos: vec![doc], current_index: 0 };
    let look = temp.path().join("old.cube");
    let history = ProjectHistory { undo: vec![cube(&look)], redo: vec![] };
    let path = temp.path().join("history.chromora");
    core::save_workspace_with_history(&workspace, &[history.clone()], &path).unwrap();
    fs::write(&look, b"different file").unwrap();
    let loaded = core::load_workspace_with_history(&path).unwrap();
    assert_eq!(loaded.workspace.photos[0].exposure_ev, 0.5); assert!(!loaded.history_warnings.is_empty());
    assert!(loaded.histories[0].undo.is_empty());
    assert!(!core::save_workspace_with_history(&workspace, &[history], &path).unwrap().is_empty());
    assert_eq!(core::load_project(&path).unwrap().exposure_ev, 0.5);
}

#[test]
fn project_cannot_overwrite_a_lut_only_referenced_by_old_history_or_its_alias() {
    let temp = tempfile::tempdir().unwrap(); let doc = fixture(&temp.path().join("original.png"));
    let workspace = WorkspaceDocument { name: "history".into(), photos: vec![doc], current_index: 0 };
    let look = temp.path().join("old.cube");
    let history = ProjectHistory { undo: vec![], redo: vec![cube(&look)] }; let bytes = fs::read(&look).unwrap();
    assert!(matches!(core::save_workspace_with_history(&workspace, &[history.clone()], &look), Err(CoreError::ProtectedOriginalDestination(_))));
    let alias = temp.path().join("alias.chromora"); fs::hard_link(&look, &alias).unwrap();
    assert!(matches!(core::save_workspace_with_history(&workspace, &[history.clone()], &alias), Err(CoreError::ProtectedOriginalDestination(_))));
    #[cfg(windows)] {
        let case_alias = look.to_string_lossy().to_uppercase();
        assert!(matches!(core::save_workspace_with_history(&workspace, &[history], Path::new(&case_alias)), Err(CoreError::ProtectedOriginalDestination(_))));
    }
    assert_eq!(fs::read(&look).unwrap(), bytes);
}

#[test]
fn old_schema_projects_start_with_empty_history() {
    let temp = tempfile::tempdir().unwrap(); let doc = fixture(&temp.path().join("original.png"));
    let path = temp.path().join("old.chromora"); core::save_project(&doc, &path).unwrap();
    let loaded = core::load_workspace_with_history(&path).unwrap();
    assert!(loaded.histories[0].undo.is_empty() && loaded.history_warnings.is_empty());
    assert_eq!(loaded.workspace.photos[0].exposure_ev, 0.5);
}
