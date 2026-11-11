//! Acceptance-only executable; the delivered application is not changed.
use std::{fs, path::{Path, PathBuf}, time::Instant};
use chromora::core::{self, GeometrySettings, LocalAdjustments, LocalMask, MaskShape, NormalizedRect};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn digest(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::Read;
    let mut file=fs::File::open(path)?;let mut hash=Sha256::new();let mut buffer=[0u8;65536];
    loop {let n=file.read(&mut buffer)?;if n==0{break;}hash.update(&buffer[..n]);}
    Ok(format!("{:x}",hash.finalize()))
}

fn exercise(source:&Path,output:&Path,report:&mut Value)->Result<(),Box<dyn std::error::Error>> {
    report["stage"]=json!("decode");let timer=Instant::now();
    let mut doc=core::open_image(source)?;
    report["decode_ms"]=json!(timer.elapsed().as_millis());
    report["source_sha256"]=json!(doc.source_sha256);
    report["source_dimensions"]=json!([doc.width(),doc.height()]);
    report["metadata"]=json!({"make":doc.photo_info().make,"model":doc.photo_info().model,
        "file_format":format!("{:?}",doc.photo_info().file_format),
        "sensor_bit_depth":doc.photo_info().bit_depth_per_sample,
        "working_format":format!("{:?}",doc.working_image().metadata.sample_format)});
    if doc.width()==0||doc.height()==0{return Err("empty decoded image".into());}
    let mut sums=[0f64;3];let mut minima=[1f32;3];let mut maxima=[0f32;3];
    for pixel in doc.working_image().pixels.pixels(){for channel in 0..3 {
        let v=pixel[channel];if !v.is_finite()||!(0.0..=1.0).contains(&v){return Err("invalid developed float sample".into());}
        sums[channel]+=f64::from(v);minima[channel]=minima[channel].min(v);maxima[channel]=maxima[channel].max(v);
    }}
    let count=f64::from(doc.width())*f64::from(doc.height());
    report["developed_statistics"]=json!({"minimum":minima,"maximum":maxima,"mean":sums.map(|v|v/count)});
    report["stage"]=json!("preview_and_adjustments");
    core::render_preview_at(&doc,Some(1440))?.save(output.join("neutral-preview.png"))?;
    let mut settings=doc.settings.clone();settings.exposure=0.35;
    settings.geometry=GeometrySettings{quarter_turns:1,straighten_degrees:3.0,
        crop:Some(NormalizedRect{x:0.1,y:0.1,width:0.8,height:0.8})};
    settings.masks=vec![LocalMask{id:"acceptance-radial".into(),name:"验收局部补光".into(),enabled:true,invert:false,
        shape:MaskShape::Radial{center:[0.5,0.5],radius:[0.35,0.35],feather:0.5},
        adjustments:LocalAdjustments{exposure:0.25,shadows:10.0,..Default::default()}}];
    core::set_settings(&mut doc,settings)?;
    let uncached=core::render_preview_at(&doc,Some(1440))?;core::prepare_preview(&mut doc)?;
    let cached=core::render_preview_at(&doc,Some(1440))?;
    if cached!=uncached{return Err("cached and uncached preview mismatch".into());}
    cached.save(output.join("edited-preview.png"))?;
    report["cached_preview_exact"]=json!(true);report["settings"]=json!(doc.settings);
    report["output_dimensions"]=json!(core::output_dimensions(doc.width(),doc.height(),&doc.settings.geometry));
    report["stage"]=json!("project_roundtrip");core::save_project(&doc,&output.join("sample.chromora"))?;
    let reopened=core::load_project(&output.join("sample.chromora"))?;
    if core::render_preview_at(&reopened,Some(1440))?!=cached{return Err("project preview mismatch".into());}
    report["project_roundtrip_exact"]=json!(true);drop(reopened);
    report["stage"]=json!("export");core::export_jpeg(&doc,&output.join("edited.jpg"),96)?;
    core::export_tiff(&doc,&output.join("edited-rgb16.tif"))?;
    if digest(source)?!=doc.source_sha256{return Err("source changed during acceptance".into());}
    report["source_unchanged"]=json!(true);report["stage"]=json!("complete");Ok(())
}

fn main()->Result<(),Box<dyn std::error::Error>> {
    let mut args=std::env::args_os().skip(1);
    let source=PathBuf::from(args.next().ok_or("usage: raw_acceptance_probe RAW NEW_OUTPUT_DIRECTORY")?);
    let output=PathBuf::from(args.next().ok_or("output directory required")?);
    if output.exists(){return Err("output directory must not exist".into());}fs::create_dir_all(&output)?;
    let timer=Instant::now();let mut report=json!({"passed":false,"source":source,"libraw_path":core::libraw_path(),
        "scope":"same-source core pipeline; not a GUI or independent color accuracy acceptance","stage":"start"});
    if let Err(error)=exercise(&source,&output,&mut report){report["error"]=json!(error.to_string());}
    else {report["passed"]=json!(true);}
    report["elapsed_ms"]=json!(timer.elapsed().as_millis());
    fs::write(output.join("probe.json"),serde_json::to_vec_pretty(&report)?)?;println!("{report}");
    if report["passed"]!=true{std::process::exit(1);}Ok(())
}
