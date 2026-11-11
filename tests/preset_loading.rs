use std::{fs, path::{Path,PathBuf}};
use chromora::core::{self,AdjustmentSettings,CoreError,GeometrySettings,LocalMask,SkinRegion};
use image::{Rgb,RgbImage};
use serde_json::{json,Value};

fn write_preset(root: &Path, value: Value) -> PathBuf {
    let path = root.join("个人预设.json");
    fs::write(&path,serde_json::to_vec(&value).unwrap()).unwrap();
    path
}
fn preset(values: Value, portrait: bool) -> Value {
    json!({"chromoraPreset":1,"group":if portrait {"portrait"} else {"landscape"},"values":values})
}

#[test]
fn saved_color_and_legacy_portrait_presets_load_without_transferring_photo_specific_data() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.png");
    RgbImage::from_pixel(4,4,Rgb([50,80,110])).save(&source).unwrap();
    let mut doc = core::open_image(&source).unwrap();
    let mut settings = doc.settings.clone();
    settings.exposure = 0.5;
    settings.temperature = 22.;
    settings.curves.insert("rgb".into(),vec![[0.,0.],[0.5,0.6],[1.,1.]]);
    settings.smoothing = 35.;
    settings.skin_tone = 18.;
    settings.skin_region = Some(SkinRegion{x:0.1,y:0.1,width:0.4,height:0.6});
    settings.geometry.quarter_turns = 1;
    settings.masks = vec![LocalMask::default()];
    core::set_settings(&mut doc,settings.clone()).unwrap();
    let mut current = AdjustmentSettings::default();
    current.smoothing = 9.;
    current.temperature = -15.;
    current.skin_region = Some(SkinRegion{x:0.3,y:0.2,width:0.5,height:0.5});
    current.geometry = GeometrySettings{quarter_turns:3,..Default::default()};
    current.masks = vec![LocalMask{id:"another".into(),..Default::default()}];
    for portrait in [false,true] {
        let path = temp.path().join(format!("saved-{portrait}.json"));
        core::save_preset(&[doc.clone()],0,portrait,&path).unwrap();
        let loaded = core::load_preset(&current,&path,portrait).unwrap();
        assert_eq!(loaded.geometry,current.geometry);
        assert_eq!(loaded.masks,current.masks);
        assert_eq!(loaded.skin_region,current.skin_region);
        if portrait {
            assert_eq!(loaded.smoothing,35.);
            assert_eq!(loaded.skin_tone,18.);
            assert_eq!(loaded.temperature,current.temperature);
            assert_eq!(loaded.curves,current.curves);
        } else {
            assert_eq!(loaded.exposure,0.5);
            assert_eq!(loaded.temperature,22.);
            assert_eq!(loaded.curves,settings.curves);
            assert_eq!(loaded.smoothing,current.smoothing);
        }
    }
}

#[test]
fn malformed_versions_groups_fields_and_values_are_rejected_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let current = AdjustmentSettings{temperature:23.,..Default::default()};
    let cases = [
        (json!({}),false),
        (json!({"chromoraPreset":2,"group":"landscape","values":{"temperature":1}}),false),
        (json!({"chromoraPreset":"1","group":"landscape","values":{"temperature":1}}),false),
        (json!({"chromoraPreset":1,"group":"portrait","values":{"smoothing":20}}),false),
        (json!({"chromoraPreset":1,"group":"landscape","values":{"temperature":1},"extra":0}),false),
        (preset(json!({}),false),false),
        (preset(json!([]),false),false),
        (preset(json!(null),false),false),
        (preset(json!({"unknown":5}),false),false),
        (preset(json!({"smoothing":5}),false),false),
        (preset(json!({"temperature":5}),true),true),
        (preset(json!({"geometry":{}}),false),false),
        (preset(json!({"masks":[]}),false),false),
        (preset(json!({"skinRegion":null}),false),false),
        (preset(json!({"skinRegion":null}),true),true),
        (preset(json!({"temperature":"warm"}),false),false),
        (preset(json!({"temperature":null}),false),false),
        (preset(json!({"temperature":true}),false),false),
        (preset(json!({"temperature":101}),false),false),
        (preset(json!({"exposure":0.03}),false),false),
        (preset(json!({"curves":{"other":[[0,0],[1,1]]}}),false),false),
        (preset(json!({"curves":{"rgb":[[0,0],[0,1]]}}),false),false),
        (preset(json!({"smoothing":20,"skinRegion":{}}),true),true),
        (preset(json!({"smoothing":20,"skinRegion":{"x":-1,"y":0,"width":0.4,"height":0.4}}),true),true),
        (preset(json!({"smoothing":20,"skinRegion":{"x":0,"y":0,"width":0.4,"height":0.4,"extra":0}}),true),true),
    ];
    for (value,portrait) in cases {
        let path = write_preset(temp.path(),value.clone());
        let bytes = fs::read(&path).unwrap();
        let before = current.clone();
        assert!(core::load_preset(&current,&path,portrait).is_err(),"accepted {value}");
        assert_eq!(current,before);
        assert_eq!(fs::read(path).unwrap(),bytes);
    }
}

#[test]
fn preset_size_limit_is_inclusive_and_nonfinite_json_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let current = AdjustmentSettings::default();
    let path = temp.path().join("bounded.json");
    let mut bytes = serde_json::to_vec(&preset(json!({"temperature":12}),false)).unwrap();
    bytes.resize(1024*1024,b' ');
    fs::write(&path,&bytes).unwrap();
    assert_eq!(core::load_preset(&current,&path,false).unwrap().temperature,12.);
    bytes.push(b' ');
    fs::write(&path,&bytes).unwrap();
    assert!(matches!(core::load_preset(&current,&path,false),Err(CoreError::InvalidProject(message)) if message.contains("1MiB")));
    for text in ["", "{\"chromoraPreset\":1,\"group\":\"landscape\",\"values\":{\"exposure\":NaN}}", "{\"chromoraPreset\":1,\"group\":\"landscape\",\"values\":{\"exposure\":1e400}}"] {
        fs::write(&path,text).unwrap();
        assert!(core::load_preset(&current,&path,false).is_err());
    }
}

#[test]
fn applying_identical_values_returns_identical_settings() {
    let temp = tempfile::tempdir().unwrap();
    let current = AdjustmentSettings{temperature:12.,exposure:0.5,..Default::default()};
    let path = write_preset(temp.path(),preset(json!({"temperature":12,"exposure":0.5}),false));
    assert_eq!(core::load_preset(&current,&path,false).unwrap(),current);
}
