use std::{fs, path::Path};
use chromora::core::{self, CoreError, PhotoDocument};
use image::{Rgb, RgbImage};

fn document(root: &Path, name: &str) -> PhotoDocument {
    let path = root.join(name);
    RgbImage::from_pixel(3,2,Rgb([40,80,120])).save(&path).unwrap();
    core::import_image(&path,&root.join("originals")).unwrap()
}

#[test]
fn preset_targets_protect_all_originals_copies_caches_and_luts() {
    let temp = tempfile::tempdir().unwrap();
    let mut first = document(temp.path(),"first.png");
    let second = document(temp.path(),"inactive.png");
    let cache = temp.path().join("render-cache.png");
    fs::copy(&first.source_path,&cache).unwrap();
    first.render_source_path = Some(cache.clone());
    let lut = temp.path().join("identity.cube");
    fs::write(&lut,"LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
    core::set_lut(&mut first,Some(&lut)).unwrap();
    let targets = vec![first.source_path.clone(),first.original_copy_path.clone().unwrap(),
        cache,lut,second.source_path.clone(),second.original_copy_path.clone().unwrap()];
    let documents = vec![first,second];
    for target in targets {
        let before = fs::read(&target).unwrap();
        assert!(matches!(core::save_preset(&documents,0,false,&target),Err(CoreError::ProtectedOriginalDestination(_))));
        assert_eq!(fs::read(&target).unwrap(),before);
    }
}

#[test]
fn named_presets_save_complete_groups_and_never_replace_existing_files() {
    let temp = tempfile::tempdir().unwrap();
    let mut doc = document(temp.path(),"photo.png");
    doc.settings.set_value("temperature",22.0).unwrap();
    doc.settings.set_value("smoothing",35.0).unwrap();
    let documents = vec![doc];
    for (portrait,name) in [(false,"调色预设.json"),(true,"人像预设.json")] {
        let target = temp.path().join(name);
        core::save_preset(&documents,0,portrait,&target).unwrap();
        let before = fs::read(&target).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(value["chromoraPreset"],1);
        if portrait {
            assert_eq!(value["values"]["smoothing"],35.0);
            assert!(value["values"].get("temperature").is_none());
        } else {
            assert_eq!(value["values"]["temperature"],22.0);
            assert!(value["values"].get("smoothing").is_none());
        }
        assert!(matches!(core::save_preset(&documents,0,portrait,&target),Err(CoreError::Io(_))));
        assert_eq!(fs::read(&target).unwrap(),before);
    }
}
