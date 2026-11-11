use std::{fs, path::Path};
use chromora::core::{self, GeometrySettings, NormalizedRect, LocalMask, LocalAdjustments, MaskShape, WorkspaceDocument};
use image::{GenericImageView, ImageReader, Rgb, RgbImage};

fn source(path: &Path) -> core::PhotoDocument {
    RgbImage::from_fn(16, 12, |x,y| Rgb([(40+x*4) as u8,(50+y*4) as u8,90])).save(path).unwrap();
    core::open_image(path).unwrap()
}
fn mask() -> LocalMask {
    LocalMask { id:"landscape-sky".into(),name:"天空".into(),enabled:true,invert:false,
        shape:MaskShape::Linear { start:[0.,0.2],end:[0.,0.8] },
        adjustments:LocalAdjustments { exposure:0.5,..Default::default() } }
}

#[test]
fn cropped_masked_document_reopens_and_exports_matching_dimensions() {
    let tmp=tempfile::tempdir().unwrap();
    let mut doc=source(&tmp.path().join("风景.png"));
    let mut settings=doc.settings.clone();
    settings.geometry=GeometrySettings { crop:Some(NormalizedRect{x:0.25,y:0.25,width:0.5,height:0.5}),quarter_turns:1,straighten_degrees:0. };
    settings.masks=vec![mask()];
    core::set_settings(&mut doc,settings).unwrap();
    let expected=core::render_preview(&doc).unwrap();
    assert_eq!(expected.dimensions(),(6,8));
    // The source dimensions remain intact for coordinates and non-destructive editing.
    assert_eq!((doc.width(),doc.height()),(16,12));
    let project=tmp.path().join("构图蒙版.chromora");
    core::save_workspace(&WorkspaceDocument{name:"构图蒙版".into(),photos:vec![doc.clone()],current_index:0},&project).unwrap();
    let reopened=core::load_project(&project).unwrap();
    assert_eq!(reopened.settings,doc.settings);
    assert_eq!(core::render_preview(&reopened).unwrap(),expected);
    for extension in ["jpg","tif","psd"] {
        let target=tmp.path().join(format!("cropped.{extension}"));
        match extension { "jpg"=>core::export_jpeg(&reopened,&target,96).unwrap(),"tif"=>core::export_tiff(&reopened,&target).unwrap(),_=>core::export_psd(&reopened,&target).unwrap() }
        if extension!="psd" {assert_eq!(ImageReader::open(target).unwrap().decode().unwrap().dimensions(),(6,8));}
        else {let bytes=fs::read(target).unwrap();assert_eq!(u32::from_be_bytes(bytes[14..18].try_into().unwrap()),8);assert_eq!(u32::from_be_bytes(bytes[18..22].try_into().unwrap()),6);}
    }
    assert_eq!(core::render_original(&reopened,None).unwrap().dimensions(),(6,8));
}

#[test]
fn schema_two_projects_get_neutral_new_tools_without_changed_pixels() {
    let tmp=tempfile::tempdir().unwrap();let doc=source(&tmp.path().join("old.png"));
    let path=tmp.path().join("v2.chromora");core::save_project(&doc,&path).unwrap();
    let mut value:serde_json::Value=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["schema_version"]=serde_json::json!(2);value["render_version"]=serde_json::json!("chromora-0.3-srgb-v1");
    let settings=value["photos"][0]["settings"].as_object_mut().unwrap();settings.remove("geometry");settings.remove("masks");
    fs::write(&path,serde_json::to_vec(&value).unwrap()).unwrap();let reopened=core::load_project(&path).unwrap();
    assert_eq!(reopened.settings.geometry,GeometrySettings::default());assert!(reopened.settings.masks.is_empty());
    assert_eq!(core::render_preview(&doc).unwrap(),core::render_preview(&reopened).unwrap());
}

#[test]
fn masks_disable_and_invert_have_visible_and_repeatable_results() {
    let tmp=tempfile::tempdir().unwrap();let mut doc=source(&tmp.path().join("mask.png"));let base=core::render_preview(&doc).unwrap();
    let mut settings=doc.settings.clone();let mut local=mask();local.shape=MaskShape::Radial{center:[0.5,0.5],radius:[0.3,0.3],feather:0.5};
    settings.masks.push(local);core::set_settings(&mut doc,settings.clone()).unwrap();let radial=core::render_preview(&doc).unwrap();
    assert_eq!(radial.get_pixel(0,0),base.get_pixel(0,0));assert!(radial.get_pixel(8,6)[0]>base.get_pixel(8,6)[0]);
    settings.masks[0].enabled=false;core::set_settings(&mut doc,settings.clone()).unwrap();assert_eq!(core::render_preview(&doc).unwrap(),base);
    settings.masks[0].enabled=true;settings.masks[0].invert=true;core::set_settings(&mut doc,settings).unwrap();let inverse=core::render_preview(&doc).unwrap();
    assert_eq!(inverse.get_pixel(8,6),base.get_pixel(8,6));assert!(inverse.get_pixel(0,0)[0]>base.get_pixel(0,0)[0]);
}

#[test]
fn color_presets_do_not_capture_crop_or_photo_specific_masks() {
    let tmp=tempfile::tempdir().unwrap();let mut doc=source(&tmp.path().join("preset.png"));
    doc.settings.geometry.quarter_turns=1;doc.settings.masks.push(mask());let path=tmp.path().join("color.json");
    core::save_preset(&[doc],0,false,&path).unwrap();let data:serde_json::Value=serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(data["values"].get("geometry").is_none());assert!(data["values"].get("masks").is_none());
}

#[test]
#[ignore="24MP preview cache measurement, run separately"]
fn measure_cached_preview_without_a_timing_threshold() {
    use std::time::Instant;
    let tmp=tempfile::tempdir().unwrap();let path=tmp.path().join("24mp.jpg");
    RgbImage::from_fn(6000,4000,|x,y|Rgb([(x%256) as u8,(y%256) as u8,((x+y)%256) as u8])).save(&path).unwrap();
    let mut cached=core::open_image(&path).unwrap();core::set_exposure(&mut cached,0.5).unwrap();let uncached=cached.clone();
    let start=Instant::now();core::prepare_preview(&mut cached).unwrap();let prepare_ms=start.elapsed().as_millis();
    let mut cold=vec![];let mut warm=vec![];
    for _ in 0..3 {let start=Instant::now();let a=core::render_preview_at(&uncached,Some(1440)).unwrap();cold.push(start.elapsed().as_millis());
        let start=Instant::now();let b=core::render_preview_at(&cached,Some(1440)).unwrap();warm.push(start.elapsed().as_millis());assert_eq!(a,b);}
    println!("CHROMORA_PREVIEW_CACHE {}",serde_json::json!({"source":[6000,4000],"edge":1440,"prepare_ms":prepare_ms,"uncached_ms":cold,"cached_ms":warm,"scope":"warm input, core render only, no GUI or disk decode timing"}));
}
