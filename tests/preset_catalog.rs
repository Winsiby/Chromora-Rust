use std::{fs, path::Path};

use chromora::core::{self, AdjustmentSettings, GeometrySettings, LocalMask, SkinRegion};
use serde_json::{Value, json};

fn write(root: &Path, name: &str, value: Value) {
    fs::write(root.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn catalog_lists_nested_valid_presets_and_ignores_metadata_invalid_values_and_oversized_files() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("富士理光");
    fs::create_dir(&folder).unwrap();
    write(temp.path(), "02_人像.json", json!({"chromoraPreset":1,"group":"portrait","values":{"smoothing":25}}));
    write(&folder, "01_ASTIA.JSON", json!({"chromoraPreset":1,"group":"landscape","values":{"contrast":-8,"curves":{"rgb":[[0,0],[0.5,0.55],[1,1]]}}}));
    write(temp.path(), "manifest.json", json!({"name":"风格包","files":["01_ASTIA.JSON"]}));
    write(temp.path(), "metadata.json", json!({"chromoraPreset":1,"group":"landscape","values":{"contrast":4},"source":"notes"}));
    write(temp.path(), "bad-version.json", json!({"chromoraPreset":2,"group":"landscape","values":{"contrast":5}}));
    write(temp.path(), "bad-group.json", json!({"chromoraPreset":1,"group":"unknown","values":{"contrast":5}}));
    write(temp.path(), "bad-value.json", json!({"chromoraPreset":1,"group":"landscape","values":{"contrast":101}}));
    write(temp.path(), "bad-fields.json", json!({"chromoraPreset":1,"group":"landscape","values":{"masks":[]}}));
    fs::write(temp.path().join("broken.json"), b"{invalid").unwrap();
    let mut oversized = serde_json::to_vec(&json!({"chromoraPreset":1,"group":"landscape","values":{"contrast":5}})).unwrap();
    oversized.resize(1024 * 1024 + 1, b' ');
    fs::write(temp.path().join("too-big.json"), oversized).unwrap();
    let before = fs::read(folder.join("01_ASTIA.JSON")).unwrap();

    let entries = core::discover_presets(temp.path()).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.windows(2).all(|pair| pair[0].path < pair[1].path));
    assert!(entries.iter().any(|entry| entry.name == "01_ASTIA" && !entry.portrait));
    assert!(entries.iter().any(|entry| entry.name == "02_人像" && entry.portrait));
    for entry in &entries {
        core::load_preset(&AdjustmentSettings::default(), &entry.path, entry.portrait).unwrap();
    }
    assert_eq!(fs::read(folder.join("01_ASTIA.JSON")).unwrap(), before);
}

#[test]
fn catalog_missing_directory_is_empty_and_depth_limit_is_explicit() {
    let temp = tempfile::tempdir().unwrap();
    assert!(core::discover_presets(&temp.path().join("not-created")).unwrap().is_empty());
    let mut path = temp.path().to_path_buf();
    for _ in 0..9 {
        path = path.join("nested");
        fs::create_dir(&path).unwrap();
    }
    assert!(core::discover_presets(temp.path()).unwrap_err().to_string().contains("8层"));
}

#[test]
fn catalog_checks_a_fixed_number_of_entries_instead_of_silently_truncating() {
    let temp = tempfile::tempdir().unwrap();
    for index in 0..4097 {
        fs::write(temp.path().join(format!("note-{index}.txt")), b"").unwrap();
    }
    assert!(core::discover_presets(temp.path()).unwrap_err().to_string().contains("4096"));
}

#[cfg(unix)]
#[test]
fn catalog_does_not_follow_a_directory_link_cycle_or_preset_file_links() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real.json");
    write(temp.path(), "real.json", json!({"chromoraPreset":1,"group":"landscape","values":{"contrast":5}}));
    symlink(temp.path(), temp.path().join("cycle")).unwrap();
    symlink(&real, temp.path().join("alias.json")).unwrap();
    let entries = core::discover_presets(temp.path()).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, real);
}

#[test]
fn copying_color_keeps_target_exposure_wb_lut_and_photo_specific_regions() {
    let source = AdjustmentSettings {
        exposure: 1.0, temperature: 32., tint: -18., lut_strength: 12.,
        contrast: -8., highlights: -20., shadows: 13., clarity: -7., saturation: -4.,
        sharpening: 14., denoise: 8., post_exposure: 0.5, post_temperature: 10.,
        orange_hue: 4., midtone_hue: 42., midtone_sat: 9.,
        smoothing: 70., skin_tone: 20., texture: 30., blemishes: 40.,
        skin_region: Some(SkinRegion { x: 0.1, y: 0.1, width: 0.2, height: 0.3 }),
        geometry: GeometrySettings { quarter_turns: 1, ..Default::default() },
        masks: vec![LocalMask { id: "source-mask".into(), ..Default::default() }],
        ..Default::default()
    };
    let mut source = source;
    source.curves.insert("blue".into(), vec![[0., 0.], [0.5, 0.6], [1., 1.]]);
    let target = AdjustmentSettings {
        exposure: -0.5, temperature: -8., tint: 5., lut_strength: 85.,
        smoothing: 15., skin_tone: 8., texture: 12., blemishes: 6.,
        skin_region: Some(SkinRegion { x: 0.3, y: 0.2, width: 0.5, height: 0.4 }),
        geometry: GeometrySettings { quarter_turns: 3, ..Default::default() },
        masks: vec![LocalMask { id: "target-mask".into(), ..Default::default() }],
        ..Default::default()
    };
    let result = core::copy_color_settings(&source, &target).unwrap();
    assert_eq!((result.exposure, result.temperature, result.tint, result.lut_strength), (-0.5, -8., 5., 85.));
    assert_eq!((result.smoothing, result.skin_tone, result.texture, result.blemishes), (15., 8., 12., 6.));
    assert_eq!(result.skin_region, target.skin_region);
    assert_eq!(result.geometry, target.geometry);
    assert_eq!(result.masks, target.masks);
    assert_eq!(result.curves, source.curves);
    assert_eq!((result.contrast, result.highlights, result.shadows, result.clarity), (-8., -20., 13., -7.));
    assert_eq!((result.sharpening, result.denoise, result.post_exposure, result.orange_hue, result.midtone_hue), (14., 8., 0.5, 4., 42.));
    assert_eq!(core::copy_color_settings(&source, &result).unwrap(), result);
}

#[test]
fn copying_rejects_invalid_source_or_target_without_mutating_either() {
    let current = AdjustmentSettings::default();
    let invalid = AdjustmentSettings { contrast: f32::NAN, ..Default::default() };
    assert!(core::copy_color_settings(&invalid, &current).is_err());
    assert!(core::copy_color_settings(&current, &invalid).is_err());
    assert_eq!(current, AdjustmentSettings::default());
    let invalid_exposure = AdjustmentSettings { exposure: 0.03, ..Default::default() };
    assert!(core::copy_color_settings(&invalid_exposure, &current).is_err());
    assert!(core::copy_color_settings(&current, &invalid_exposure).is_err());
}
