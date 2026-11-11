//! Explicit local interchange probe; no outputs are written unless a new
//! destination directory is supplied. Python is used only for independent QA.
use std::{fs, path::PathBuf};
use chromora::core::{export_jpeg, export_psd, export_tiff, open_image, render_preview, set_exposure, DEFAULT_JPEG_QUALITY};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let output = PathBuf::from(arguments.next().ok_or("usage: io_probe NEW_OUTPUT_DIR [RAW_FILE]")?);
    if output.exists() { return Err("probe destination already exists; choose a new directory".into()); }
    fs::create_dir_all(&output)?;
    let source = output.join("source-rgb16.tif");
    let samples: Vec<u16> = (0..11 * 7 * 3).map(|v| ((v * 251 + 1000) % 65536) as u16).collect();
    tiff::encoder::TiffEncoder::new(fs::File::create(&source)?)?
        .write_image::<tiff::encoder::colortype::RGB16>(11, 7, &samples)?;
    let mut doc = open_image(&source)?;
    export_tiff(&doc, &output.join("neutral-rgb16.tif"))?;
    doc.original_rgb.save(output.join("expected-original.png"))?;
    set_exposure(&mut doc, 0.95)?;
    render_preview(&doc)?.save(output.join("expected-edited.png"))?;
    export_jpeg(&doc, &output.join("edited.jpg"), DEFAULT_JPEG_QUALITY)?;
    export_tiff(&doc, &output.join("edited-rgb16.tif"))?;
    export_psd(&doc, &output.join("edited-two-layers.psd"))?;
    // The same export must work after a filmstrip item has released its cache.
    doc.evict_pixels();
    export_psd(&doc, &output.join("edited-evicted.psd"))?;
    drop(doc);
    let mut report = serde_json::json!({"synthetic_dimensions":[11,7],"exposure_ev":0.95,"raw":null});
    if let Some(raw_path) = arguments.next() {
        let raw_path = PathBuf::from(raw_path);
        let raw = open_image(&raw_path)?;
        export_tiff(&raw, &output.join("raw-developed-rgb16.tif"))?;
        report["raw"] = serde_json::json!({
            "path": raw_path,
            "dimensions":[raw.width(),raw.height()],
            "make":raw.photo_info().make,
            "model":raw.photo_info().model,
            "sensor_bit_depth_known":raw.photo_info().bit_depth_per_sample,
            "decoded_rgb_bit_depth":16,
            "working_format":format!("{:?}",raw.working_image().metadata.sample_format),
        });
    }
    fs::write(output.join("probe.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{}", report);
    Ok(())
}
