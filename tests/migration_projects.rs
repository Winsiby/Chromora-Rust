use std::{fs, path::Path};
use chromora::core::{self, CoreError, WorkspaceDocument};
use image::{Rgb, RgbImage};
use sha2::{Digest, Sha256};

fn source(root: &Path, name: &str, value: u8) -> core::PhotoDocument {
    let path = root.join(name);
    RgbImage::from_pixel(8, 6, Rgb([value, 90, 140])).save(&path).unwrap();
    core::import_image(&path, &root.join("originals")).unwrap()
}

#[test]
fn workspace_preserves_per_photo_edits_and_lut_and_evicts_inactive_pixels() {
    let temp = tempfile::tempdir().unwrap();
    let mut first = source(temp.path(), "人像.png", 50);
    let second = source(temp.path(), "风景.png", 120);
    let mut settings = first.settings.clone();
    settings.set_value("temperature", 18.0).unwrap();
    settings.set_value("saturation", -12.0).unwrap();
    settings.curves.insert("rgb".into(), vec![[0.0,0.0],[0.5,0.6],[1.0,1.0]]);
    core::set_settings(&mut first, settings).unwrap();
    let lut = temp.path().join("identity.cube");
    fs::write(&lut, "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
    core::set_lut(&mut first, Some(&lut)).unwrap();
    let expected = core::render_preview(&first).unwrap();
    let workspace = WorkspaceDocument { name: "中文工程".into(), photos: vec![first,second], current_index: 0 };
    let target = temp.path().join("工作台.chromora");
    core::save_workspace(&workspace, &target).unwrap();
    let mut reopened = core::load_workspace(&target).unwrap();
    assert_eq!(reopened.name, "中文工程");
    assert_eq!(reopened.photos.len(), 2);
    assert_eq!(core::render_preview(&reopened.photos[0]).unwrap(), expected);
    assert_eq!(reopened.photos[1].working_image().pixels.width(), 0);
    core::hydrate_document(&mut reopened.photos[1]).unwrap();
    assert_eq!(reopened.photos[1].width(), 8);
    // The LUT reference is content-bound; replacing its bytes is an error on reopen.
    let project_before = fs::read(&target).unwrap();
    fs::write(lut, "changed").unwrap();
    assert!(matches!(core::save_workspace(&workspace, &target), Err(CoreError::MissingOrChangedSource(_))));
    assert_eq!(fs::read(&target).unwrap(), project_before);
    assert!(matches!(core::load_workspace(&target), Err(CoreError::MissingOrChangedSource(_))));
}

#[test]
fn workspace_target_protects_every_photo_including_inactive_ones() {
    let temp = tempfile::tempdir().unwrap();
    let a = source(temp.path(), "a.png", 10);
    let b = source(temp.path(), "b.png", 20);
    let destination = b.original_copy_path.as_ref().unwrap().clone();
    let before = fs::read(&destination).unwrap();
    let workspace = WorkspaceDocument { name: "test".into(), photos:vec![a,b], current_index: 0 };
    assert!(matches!(core::save_workspace(&workspace, &destination), Err(CoreError::ProtectedOriginalDestination(_))));
    assert_eq!(fs::read(destination).unwrap(), before);
}

#[test]
fn rust_schema_one_remains_readable_and_renders_the_same_exposure() {
    let temp = tempfile::tempdir().unwrap();
    let mut doc = source(temp.path(), "old.png", 30);
    core::set_exposure(&mut doc, 0.5).unwrap();
    let target = temp.path().join("v1.chromora");
    let value = serde_json::json!({"schema_version":1,"source_path":doc.source_path,
        "original_copy_path":doc.original_copy_path,"source_sha256":doc.source_sha256,
        "width":8,"height":6,"exposure_ev":0.5});
    fs::write(&target, serde_json::to_vec(&value).unwrap()).unwrap();
    let old = core::load_project(&target).unwrap();
    assert_eq!(core::render_preview(&old).unwrap(), core::render_preview(&doc).unwrap());
}

#[test]
fn python_project_import_is_explicit_and_never_overwrites_legacy_file() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("数据");
    fs::create_dir_all(data.join("工程")).unwrap();
    fs::create_dir_all(data.join("原片")).unwrap();
    let fixture = temp.path().join("photo.png");
    RgbImage::from_pixel(3,2,Rgb([90,100,110])).save(&fixture).unwrap();
    let bytes = fs::read(fixture).unwrap();
    let id = format!("{:x}.png", Sha256::digest(&bytes));
    fs::write(data.join("原片").join(&id), bytes).unwrap();
    let value = serde_json::json!({"name":"旧工程","state":{"photos":[{"id":id,"raw":false,
        "settings":{"exposure":0.25,"contrast":12,"redHue":-8}}],"currentId":id,"luts":[],"favorites":[]}});
    let legacy = data.join("工程").join("legacy.json");
    let before = serde_json::to_vec(&value).unwrap();
    fs::write(&legacy,&before).unwrap();
    let workspace = core::load_workspace(&legacy).unwrap();
    assert_eq!(workspace.photos[0].exposure_ev,0.25);
    assert_eq!(workspace.photos[0].settings.value("redHue"),Some(-8.0));
    assert!(core::save_workspace(&workspace,&legacy).is_err());
    assert_eq!(fs::read(&legacy).unwrap(), before);
    let target = data.join("工程").join("migrated.chromora");
    core::save_workspace(&workspace,&target).unwrap();
    assert_eq!(core::render_preview(&workspace.photos[0]).unwrap(), core::render_preview(&core::load_project(&target).unwrap()).unwrap());
}
