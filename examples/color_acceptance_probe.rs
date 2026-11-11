//! Acceptance-only adapter around the unchanged production core. No reference
//! color conversion is implemented here; independent QA reads the raw samples.
use std::{fs, io::{BufWriter, Write}, path::{Path, PathBuf}};
use chromora::core::{open_image_with_pipeline, ColorPipeline, CoreError, render_preview, render_rgb_f32, set_exposure,
    export_png, export_tiff, export_jpeg, DEFAULT_JPEG_QUALITY};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case { id: String, input_path: PathBuf, #[serde(default)] exposure_ev: f32, #[serde(default="default_pipeline")] pipeline:String }
fn default_pipeline()->String {"float".into()}
#[derive(Deserialize)]
struct Manifest { cases: Vec<Case> }

fn write_f32(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    let mut output = BufWriter::new(fs::File::create_new(path)?);
    for sample in samples { output.write_all(&sample.to_le_bytes())?; }
    output.flush()
}

fn process(case: &Case, output: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let pipeline=match case.pipeline.as_str(){"float"=>ColorPipeline::FloatSrgb,"accurate"=>ColorPipeline::AccurateSrgb,"hq"=>ColorPipeline::HqSrgb,"legacy"=>ColorPipeline::LegacySrgb,_=>return Err("unknown QA pipeline".into())};
    let mut document = open_image_with_pipeline(&case.input_path,pipeline)?;
    write_f32(&output.join("source-f32.bin"), document.working_image().pixels.as_raw())?;
    set_exposure(&mut document, case.exposure_ev)?;
    let rendered = render_rgb_f32(&document, None)?;
    write_f32(&output.join("render-f32.bin"), rendered.as_raw())?;
    render_preview(&document)?.save(output.join("preview.png"))?;
    export_png(&document, &output.join("export.png"))?;
    export_tiff(&document, &output.join("export.tif"))?;
    export_jpeg(&document, &output.join("export.jpg"), DEFAULT_JPEG_QUALITY)?;
    Ok(serde_json::json!({"id":case.id,"input_path":case.input_path,
        "width":rendered.width(),"height":rendered.height(),
        "source_sha256":document.source_sha256,"exposure_ev":case.exposure_ev,
        "pipeline":case.pipeline,"icc_conversion":format!("{:?}",document.photo_info().color_conversion),
        "source_metadata":{"bit_depth_per_sample":document.photo_info().bit_depth_per_sample,
            "icc_profile":{"embedded":document.photo_info().icc_profile.embedded,
                "description":document.photo_info().icc_profile.description,
                "bytes":document.photo_info().icc_profile.bytes}},
        "working_metadata":{"color_space":format!("{:?}",document.working_image().metadata.color_space),
            "sample_format":format!("{:?}",document.working_image().metadata.sample_format)},
        "passed_execution":true}))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let manifest = PathBuf::from(arguments.next().ok_or("usage: color_acceptance_probe CASES_JSON NEW_OUTPUT_ROOT")?);
    let output = PathBuf::from(arguments.next().ok_or("missing output root")?);
    let cases: Manifest = serde_json::from_slice(&fs::read(manifest)?)?;
    if output.exists() { return Err("destination exists; preserve the prior run".into()); }
    fs::create_dir_all(&output)?;
    let mut failures = 0;
    for case in cases.cases {
        if case.id.is_empty() || !case.id.chars().all(|v| v.is_ascii_alphanumeric() || v == '-' || v == '_') {
            return Err("case id must be a simple ASCII file name".into());
        }
        let folder = output.join(&case.id);
        fs::create_dir(&folder)?;
        let report = match process(&case, &folder) {
            Ok(report) => report,
            Err(error) => { failures += 1; let kind=if matches!(error.downcast_ref::<CoreError>(),Some(CoreError::InvalidIcc{..})){"color_profile"}else{"execution"};
                serde_json::json!({"id":case.id,"input_path":case.input_path,"pipeline":case.pipeline,"error_kind":kind,"passed_execution":false,"error":error.to_string()}) }
        };
        fs::write(folder.join("probe.json"), serde_json::to_vec_pretty(&report)?)?;
        println!("{}", report);
    }
    if failures != 0 { return Err(format!("{failures} cases returned an execution error").into()); }
    Ok(())
}
