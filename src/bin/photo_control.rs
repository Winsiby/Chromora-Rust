//! Local JSON command adapter around the same core used by the desktop editor.
//! This does not listen on the network and does not control an already-running GUI.
//! Usage: photo_control REQUEST.json
use std::{collections::HashSet, ffi::OsStr, fs::{self, File, OpenOptions}, io::{BufWriter, Read, Write}, path::{Path, PathBuf}};

use chromora::core::{self, AdjustmentSettings, ExportKind, ExportOptions, PhotoInfo, RenderCancellation, WorkspaceDocument};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    source: PathBuf,
    output_dir: PathBuf,
    variants: Vec<Variant>,
    #[serde(default = "default_preview_edge")]
    preview_long_edge: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    name: String,
    preset: Option<PathBuf>,
    #[serde(default)]
    adjustments: Map<String, Value>,
    #[serde(default)]
    full_export: bool,
}

fn default_preview_edge() -> u32 { 1600 }

fn safe_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return false;
    }
    let upper = name.to_ascii_uppercase();
    !matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(upper.len() == 4 && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
}

// Core settings accept legacy fields leniently; this new request boundary must
// reject silently ignored fields, including fields inside masks and geometry.
fn reject_ignored_fields(input: &Value, normalized: &Value, at: &str) -> Result<()> {
    match (input, normalized) {
        (Value::Object(fields), Value::Object(known)) => {
            for (key, value) in fields {
                let child = known.get(key).ok_or_else(|| format!("unknown adjustment field: {at}.{key}"))?;
                reject_ignored_fields(value, child, &format!("{at}.{key}"))?;
            }
        }
        (Value::Array(values), Value::Array(known)) => {
            for (index, (value, child)) in values.iter().zip(known).enumerate() {
                reject_ignored_fields(value, child, &format!("{at}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn apply_adjustments(base: &AdjustmentSettings, changes: &Map<String, Value>) -> Result<AdjustmentSettings> {
    let mut merged = serde_json::to_value(base)?;
    for (key, value) in changes {
        if !AdjustmentSettings::CONTROLS.iter().any(|(name, _, _)| *name == key.as_str())
            && !matches!(key.as_str(), "curves" | "geometry" | "masks" | "skinRegion") {
            return Err(format!("unknown adjustment: {key}").into());
        }
        merged.as_object_mut().expect("serialized settings object").insert(key.clone(), value.clone());
    }
    let settings: AdjustmentSettings = serde_json::from_value(merged)?;
    reject_ignored_fields(&Value::Object(changes.clone()), &serde_json::to_value(&settings)?, "adjustments")?;
    settings.validate()?;
    Ok(settings)
}

fn validate_request(request: &Request) -> Result<()> {
    if !(1..=16).contains(&request.variants.len()) { return Err("variants must contain 1..=16 entries".into()); }
    if !(1..=3000).contains(&request.preview_long_edge) { return Err("preview_long_edge must be in 1..=3000".into()); }
    let mut names = HashSet::new();
    for variant in &request.variants {
        if !safe_name(&variant.name) { return Err(format!("invalid variant name: {}", variant.name).into()); }
        if !names.insert(variant.name.to_ascii_lowercase()) { return Err("variant names must be unique, ignoring case".into()); }
        apply_adjustments(&AdjustmentSettings::default(), &variant.adjustments)?;
    }
    Ok(())
}

fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { base.join(path) }
}

fn write_json_new(path: &Path, value: &Value) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn digest(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 { break; }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn photo_info(info: &PhotoInfo) -> Value {
    json!({
        "captured_at_exif": info.captured_at_exif,
        "captured_at_offset_exif": info.captured_at_offset_exif,
        "make": info.make, "model": info.model, "lens_model": info.lens_model,
        "focal_length_mm": info.focal_length_mm,
        "equivalent_focal_length_35mm": info.equivalent_focal_length_35mm,
        "aperture_f_number": info.aperture_f_number, "exposure_time_seconds": info.exposure_time_seconds,
        "exposure_compensation_ev": info.exposure_compensation_ev, "iso": info.iso,
        "white_balance": info.white_balance.as_ref().map(|wb| json!({
            "mode": wb.mode.map(|mode| format!("{mode:?}")),
            "temperature_kelvin": wb.temperature_kelvin, "tint": wb.tint
        })),
        "vendor_style_name": info.vendor_style_name, "file_format": format!("{:?}", info.file_format),
        "file_size_bytes": info.file_size_bytes, "source_dimensions": info.source_dimensions,
        "oriented_dimensions": info.oriented_dimensions, "orientation": info.orientation,
        "bit_depth_per_sample": info.bit_depth_per_sample, "source_has_alpha": info.source_has_alpha,
        "icc_profile": {"embedded": info.icc_profile.embedded, "bytes": info.icc_profile.bytes,
            "description": info.icc_profile.description},
        "color_conversion": format!("{:?}", info.color_conversion)
    })
}

fn run(request_path: &Path) -> Result<Value> {
    let request_path = request_path.canonicalize()?;
    let input = File::open(&request_path)?;
    let mut bytes = Vec::new();
    input.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 { return Err("request exceeds 1 MiB".into()); }
    let request: Request = serde_json::from_slice(&bytes)?;
    validate_request(&request)?;
    let base = request_path.parent().ok_or("request has no parent directory")?;
    let source = resolve(base, &request.source).canonicalize()?;
    let requested_output = resolve(base, &request.output_dir);
    if requested_output.exists() { return Err("output_dir already exists; previous results are preserved".into()); }
    let parent = requested_output.parent().ok_or("output_dir has no parent")?.canonicalize()?;
    let output = parent.join(requested_output.file_name().ok_or("output_dir must name a new directory")?);

    // The single source decode and its full-precision pixels are shared by all variants.
    let mut original = core::open_image(&source)?;
    core::prepare_preview(&mut original)?;
    let mut prepared = Vec::with_capacity(request.variants.len());
    for variant in &request.variants {
        let preset_path = variant.preset.as_ref().map(|path| resolve(base, path).canonicalize()).transpose()?;
        let settings = if let Some(path) = &preset_path { core::load_preset(&original.settings, path, false)? }
            else { original.settings.clone() };
        let settings = apply_adjustments(&settings, &variant.adjustments)?;
        let mut document = original.clone();
        core::set_settings(&mut document, settings)?;
        prepared.push((document, preset_path));
    }
    // Atomic directory creation prevents another run's existing output from being reused.
    fs::create_dir(&output)?;
    let mut variants = Vec::with_capacity(request.variants.len());
    for (variant, (document, preset_path)) in request.variants.iter().zip(prepared) {
        let directory = output.join(&variant.name);
        fs::create_dir(&directory)?;
        let preview_path = directory.join("preview.png");
        let preview = core::render_preview_at(&document, Some(request.preview_long_edge))?;
        let preview_size = [preview.width(), preview.height()];
        let diagnostics = core::PreviewDiagnostics::from_rgb8(&preview);
        let mut writer = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(&preview_path)?);
        preview.write_to(&mut writer, image::ImageFormat::Png)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(preview);
        let project_path = directory.join("edit.chromora");
        core::save_workspace(&WorkspaceDocument { name: variant.name.clone(), photos: vec![document.clone()], current_index: 0 }, &project_path)?;
        let exports = if variant.full_export {
            let png = directory.join("full.png");
            let jpeg = directory.join("full.jpg");
            let options = ExportOptions { jpeg_quality: 96, max_long_edge: None };
            core::export_with_options(&document, &png, ExportKind::Png, &options, &RenderCancellation::new())?;
            core::export_with_options(&document, &jpeg, ExportKind::Jpeg, &options, &RenderCancellation::new())?;
            json!({"png_16bit": png, "jpeg_quality_96": jpeg})
        } else { Value::Null };
        variants.push(json!({
            "name": variant.name, "preset": preset_path, "settings": document.settings,
            "preview_path": preview_path, "preview_dimensions": preview_size,
            "project_path": project_path, "full_exports": exports,
            "output_dimensions": core::output_dimensions(document.width(), document.height(), &document.settings.geometry),
            "preview_diagnostics": {"shadow_percent": diagnostics.shadow_percent(), "highlight_percent": diagnostics.highlight_percent()}
        }));
    }
    let source_after = digest(&source)?;
    if source_after != original.source_sha256 { return Err("source changed during processing; do not accept these outputs".into()); }
    let response_path = output.join("response.json");
    let response = json!({
        "success": true, "interface_version": 1,
        "scope": "Chromora core JSON CLI; does not operate the current desktop GUI",
        "request_path": request_path, "request_sha256": format!("{:x}", Sha256::digest(&bytes)),
        "source_path": source, "source_sha256": original.source_sha256, "source_unchanged": true,
        "source_dimensions": [original.width(), original.height()], "photo_info": photo_info(original.photo_info()),
        "color_pipeline": original.color_pipeline(), "preview_long_edge": request.preview_long_edge,
        "output_dir": output, "response_path": response_path, "variants": variants
    });
    write_json_new(&response_path, &response)?;
    Ok(response)
}

fn is_help_arg(arg: &OsStr) -> bool {
    arg == OsStr::new("--help") || arg == OsStr::new("-h") || arg == OsStr::new("/?")
}

fn help_text() -> String {
    format!(
        "\
Chromora photo_control {version}

用 JSON 请求调用本机修图引擎，生成预览、工程和可选成片。
不监听网络端口，也不操作已经打开的桌面窗口。

用法:
  photo_control REQUEST.json
  photo_control --help

REQUEST.json 最大 1 MiB。其中的相对路径以请求文件所在目录为基准。
output_dir 必须是尚不存在的目录。成功时把 JSON 写到标准输出，并写入该目录下的 response.json。

请求字段见仓库中的 docs/photo-control.md。
",
        version = env!("CARGO_PKG_VERSION"),
    )
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [arg] if is_help_arg(arg) => print!("{}", help_text()),
        [path] => match run(Path::new(path)) {
            Ok(response) => println!("{response}"),
            Err(error) => {
                println!("{}", json!({"success": false, "error": error.to_string()}));
                std::process::exit(1);
            }
        },
        _ => {
            println!("{}", json!({"success": false, "error": "usage: photo_control REQUEST.json"}));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_top_level_and_nested_adjustments() {
        for changes in [json!({"expsoure": 1}), json!({"geometry": {"typo": 1}}),
            json!({"skinRegion": {"x":0.1,"y":0.1,"width":0.5,"height":0.5,"typo":1}})] {
            assert!(apply_adjustments(&AdjustmentSettings::default(), changes.as_object().unwrap()).is_err());
        }
        assert!(serde_json::from_value::<Request>(json!({"source":"a.png","output_dir":"new","variants":[],"typo":1})).is_err());
    }

    #[test]
    fn rejects_unsafe_or_colliding_windows_names() {
        for name in ["", "../escape", "a/b", "C:photo", "NUL", "com1", "lpt9", ".hidden", "人像"] { assert!(!safe_name(name), "{name}"); }
        assert!(safe_name("fuji-01_natural"));
        let request: Request = serde_json::from_value(json!({"source":"a.png","output_dir":"new","variants":[{"name":"Look"},{"name":"look"}]})).unwrap();
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn help_describes_the_local_json_command() {
        let help = help_text();
        assert!(help.contains(env!("CARGO_PKG_VERSION")));
        assert!(help.contains("photo_control REQUEST.json"));
        assert!(help.contains("--help"));
        assert!(help.contains("不监听网络"));
        assert!(is_help_arg(std::ffi::OsStr::new("--help")));
        assert!(is_help_arg(std::ffi::OsStr::new("-h")));
        assert!(is_help_arg(std::ffi::OsStr::new("/?")));
        assert!(!is_help_arg(std::ffi::OsStr::new("request.json")));
    }

    #[test]
    fn parses_defaults_and_enforces_control_curve_and_preview_bounds() {
        let mut request: Request = serde_json::from_value(json!({"source":"a.png","output_dir":"new","variants":[{"name":"natural","adjustments":{"exposure":0.5,"contrast":100,"curves":{"rgb":[[0,0],[1,1]]}}}]})).unwrap();
        validate_request(&request).unwrap();
        assert_eq!(request.preview_long_edge, 1600);
        assert!(!request.variants[0].full_export);
        let settings = apply_adjustments(&AdjustmentSettings::default(), &request.variants[0].adjustments).unwrap();
        assert_eq!(settings.exposure, 0.5);
        assert_eq!(settings.contrast, 100.);
        for changes in [json!({"contrast":100.1}), json!({"exposure":3.1}), json!({"curves":{"rgb":[[0,0],[0,1]]}})] {
            assert!(apply_adjustments(&AdjustmentSettings::default(), changes.as_object().unwrap()).is_err());
        }
        request.preview_long_edge = 3001;
        assert!(validate_request(&request).is_err());
    }
}
