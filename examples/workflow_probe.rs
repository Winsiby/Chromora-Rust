//! Local workflow/export probe; requires a new output directory.
use std::{fs,path::PathBuf};
use chromora::core::{self,GeometrySettings,NormalizedRect,LocalMask,LocalAdjustments,MaskShape,WorkspaceDocument};
fn main()->Result<(),Box<dyn std::error::Error>> {
    let mut args=std::env::args_os().skip(1);
    let output=PathBuf::from(args.next().ok_or("usage: workflow_probe NEW_DIRECTORY SOURCE")?);
    let source=PathBuf::from(args.next().ok_or("source image is required")?);
    if output.exists(){return Err("output directory exists".into());}fs::create_dir_all(&output)?;
    let mut doc=core::open_image(&source)?;let mut settings=doc.settings.clone();
    settings.geometry=GeometrySettings{quarter_turns:1,straighten_degrees:5.,crop:Some(NormalizedRect{x:0.125,y:0.125,width:0.75,height:0.75})};
    settings.masks=vec![
        LocalMask{id:"sky".into(),name:"线性压暗".into(),enabled:true,invert:false,shape:MaskShape::Linear{start:[0.,0.1],end:[0.,0.7]},adjustments:LocalAdjustments{exposure:-0.4,..Default::default()}},
        LocalMask{id:"subject".into(),name:"径向补光".into(),enabled:true,invert:false,shape:MaskShape::Radial{center:[0.5,0.5],radius:[0.3,0.3],feather:0.5},adjustments:LocalAdjustments{exposure:0.6,shadows:15.,..Default::default()}},
    ];
    core::set_settings(&mut doc,settings)?;core::prepare_preview(&mut doc)?;
    core::render_original(&doc,None)?.save(output.join("expected-original.png"))?;
    core::render_preview(&doc)?.save(output.join("expected-edited.png"))?;
    core::render_preview_at(&doc,Some(1440))?.save(output.join("fast-preview.png"))?;
    core::export_tiff(&doc,&output.join("构图蒙版.tif"))?;core::export_psd(&doc,&output.join("构图蒙版.psd"))?;core::export_jpeg(&doc,&output.join("构图蒙版.jpg"),96)?;
    let workspace=WorkspaceDocument{name:"构图蒙版验证".into(),photos:vec![doc.clone()],current_index:0};
    core::save_workspace(&workspace,&output.join("构图蒙版.chromora"))?;
    let reopened=core::load_project(&output.join("构图蒙版.chromora"))?;
    assert_eq!(core::render_preview(&doc)?,core::render_preview(&reopened)?);
    let report=serde_json::json!({"source":[doc.width(),doc.height()],"output":core::output_dimensions(doc.width(),doc.height(),&doc.settings.geometry),"settings":doc.settings,"project_roundtrip":true});
    fs::write(output.join("probe.json"),serde_json::to_vec_pretty(&report)?)?;println!("{report}");Ok(())
}
