use std::{fs, io::{BufWriter, Write}, path::{Path, PathBuf}, process::{Command, Stdio}};

use image::{ImageEncoder, Rgb32FImage, RgbImage};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use moxcms::ColorProfile;
use tempfile::NamedTempFile;

use super::{CoreError, CoreResult, PhotoDocument, RenderCancellation, render_rgb_f32_cancellable};
use super::colorspace::{self, ExportColorSpace};

pub const DEFAULT_JPEG_QUALITY: u8 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind { Jpeg, Png, Tiff, Psd }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportOptions {
    pub jpeg_quality: u8,
    /// Final output long edge. Smaller images retain their original size.
    pub max_long_edge: Option<u32>,
    /// Wide-gamut delivery space. The working buffer stays sRGB.
    pub color_space: ExportColorSpace,
}
impl Default for ExportOptions {
    fn default() -> Self { Self { jpeg_quality: DEFAULT_JPEG_QUALITY, max_long_edge: None, color_space: ExportColorSpace::Srgb } }
}
impl ExportOptions {
    pub fn validate(&self, kind: ExportKind) -> CoreResult<()> {
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err(CoreError::InvalidJpeg("质量必须在 1..=100 之间".into()));
        }
        if self.max_long_edge.is_some_and(|edge| !(1..=30_000).contains(&edge)) {
            return Err(CoreError::InvalidExport("导出长边必须在 1..=30000 像素之间".into()));
        }
        if kind == ExportKind::Psd && self.max_long_edge.is_some() {
            return Err(CoreError::InvalidExport("双图层 PSD 保持原尺寸，请选择原尺寸或改用 JPG、PNG、TIFF".into()));
        }
        if kind == ExportKind::Psd && self.color_space != ExportColorSpace::Srgb {
            return Err(CoreError::InvalidExport("双图层 PSD 仅支持 sRGB 输出，请改用 JPG、PNG 或 TIFF".into()));
        }
        Ok(())
    }
}

fn check_destination(doc: &PhotoDocument, path: &Path) -> CoreResult<()> {
    super::project::protect_original_destinations(doc, path)?;
    if path.exists() {
        return Err(CoreError::Io(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "导出目标已存在，不会覆盖")));
    }
    Ok(())
}

/// A batch output must also protect assets referenced by the other photographs.
pub fn protect_export_destinations(documents: &[PhotoDocument], destination: &Path,
    cancel: &RenderCancellation) -> CoreResult<()> {
    cancel.check()?;
    for doc in documents {
        cancel.check()?;
        super::project::protect_original_destinations(doc, destination)?;
    }
    cancel.check()
}

/// All edits and geometry are rendered at full resolution before optional downscaling.
/// Cancellation is cooperative; a third-party resize or encoder call must return first.
pub fn export_with_options(doc: &PhotoDocument, path: &Path, kind: ExportKind,
    options: &ExportOptions, cancel: &RenderCancellation) -> CoreResult<()> {
    check_destination(doc, path)?;
    options.validate(kind)?;
    cancel.check()?;
    if kind == ExportKind::Psd { return export_psd_cancellable(doc, path, cancel); }
    let mut rendered = render_rgb_f32_cancellable(doc, None, cancel)?;
    if let Some(edge) = options.max_long_edge.filter(|edge| *edge < rendered.width().max(rendered.height())) {
        let scale = f64::from(edge) / f64::from(rendered.width().max(rendered.height()));
        let width = (f64::from(rendered.width()) * scale).round_ties_even().max(1.0) as u32;
        let height = (f64::from(rendered.height()) * scale).round_ties_even().max(1.0) as u32;
        rendered = image::imageops::resize(&rendered, width, height, image::imageops::FilterType::Lanczos3);
        cancel.check()?;
    }
    // Wide-gamut delivery: the edit pipeline emits sRGB, so conversion happens
    // once here and never touches adjustment or mask math.
    let rendered = if options.color_space == ExportColorSpace::Srgb {
        rendered
    } else {
        let mut converted = rendered;
        for (index, pixel) in converted.pixels_mut().enumerate() {
            if index % 16_384 == 0 { cancel.check()?; }
            pixel.0 = colorspace::from_srgb(options.color_space, pixel.0);
        }
        converted
    };
    match kind {
        ExportKind::Jpeg => {
            let mut samples = Vec::with_capacity(rendered.as_raw().len());
            for chunk in rendered.as_raw().chunks(65536) {
                cancel.check()?;
                samples.extend(chunk.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8));
            }
            let rgb = RgbImage::from_raw(rendered.width(), rendered.height(), samples).expect("RGB dimensions match rendered samples");
            encode_jpeg_exclusive(&rgb, path, options.jpeg_quality, options.color_space, cancel)
        },
        ExportKind::Png => encode_png_exclusive(&rendered, path, options.color_space, cancel),
        ExportKind::Tiff => encode_tiff_exclusive(&rendered, path, options.color_space, cancel),
        ExportKind::Psd => unreachable!("PSD handled before rendering"),
    }
}

pub fn export_jpeg(doc: &PhotoDocument, path: &Path, quality: u8) -> CoreResult<()> {
    export_with_options(doc, path, ExportKind::Jpeg, &ExportOptions { jpeg_quality: quality, ..Default::default() }, &RenderCancellation::new())
}

/// The ICC profile matching an output space; the working buffer does not change.
fn icc_profile(space: ExportColorSpace) -> CoreResult<Vec<u8>> {
    let profile = match space {
        ExportColorSpace::Srgb => ColorProfile::new_srgb(),
        ExportColorSpace::DisplayP3 => ColorProfile::new_display_p3(),
        ExportColorSpace::AdobeRgb => ColorProfile::new_adobe_rgb(),
        ExportColorSpace::ProPhotoRgb => ColorProfile::new_pro_photo_rgb(),
    };
    Ok(profile.encode()?)
}

fn encode_jpeg_exclusive(image: &RgbImage, path: &Path, quality: u8, space: ExportColorSpace, cancel: &RenderCancellation) -> CoreResult<()> {
    cancel.check()?;
    let width = u16::try_from(image.width()).map_err(|_| CoreError::JpegDimensionsTooLarge)?;
    let height = u16::try_from(image.height()).map_err(|_| CoreError::JpegDimensionsTooLarge)?;
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, quality);
    encoder.set_sampling_factor(SamplingFactor::F_1_1); // 4:4:4, matching legacy export
    encoder.add_icc_profile(&icc_profile(space)?)?;
    cancel.check()?;
    encoder.encode(image.as_raw(), width, height, ColorType::Rgb)?;

    persist_exclusive(path, cancel, |file| { file.write_all(&bytes)?; Ok(()) })
}

fn persist_exclusive(path: &Path, cancel: &RenderCancellation, write: impl FnOnce(&mut fs::File) -> CoreResult<()>) -> CoreResult<()> {
    cancel.check()?;
    if path.exists() {
        return Err(CoreError::Io(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "导出目标已存在，不会覆盖")));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    write(temp.as_file_mut())?;
    temp.as_file().sync_all()?;
    cancel.check()?;
    temp.persist_noclobber(path)
        .map_err(|e| CoreError::Io(e.error))?;
    Ok(())
}

/// Full-resolution, lossless RGB16 PNG with the selected ICC profile,
/// quantized after all edits.
pub fn export_png(doc: &PhotoDocument, path: &Path) -> CoreResult<()> {
    export_with_options(doc, path, ExportKind::Png, &ExportOptions::default(), &RenderCancellation::new())
}

fn encode_png_exclusive(rendered: &Rgb32FImage, path: &Path, space: ExportColorSpace, cancel: &RenderCancellation) -> CoreResult<()> {
    // ImageEncoder expects native-endian RGB16 bytes and handles PNG's big-endian storage.
    let mut samples = Vec::with_capacity(rendered.as_raw().len() * 2);
    for chunk in rendered.as_raw().chunks(65536) {
        cancel.check()?;
        samples.extend(chunk.iter().flat_map(|v| ((v.clamp(0.0, 1.0) * 65535.0).round_ties_even() as u16).to_ne_bytes()));
    }
    let profile = icc_profile(space)?;
    persist_exclusive(path, cancel, |file| {
        let mut writer = BufWriter::new(file);
        let mut encoder = image::codecs::png::PngEncoder::new_with_quality(
            &mut writer,
            image::codecs::png::CompressionType::Default,
            image::codecs::png::FilterType::Adaptive,
        );
        encoder.set_icc_profile(profile).map_err(image::ImageError::Unsupported)?;
        encoder.write_image(&samples, rendered.width(), rendered.height(), image::ExtendedColorType::Rgb16)?;
        writer.flush()?;
        Ok(())
    })
}

/// Full-resolution RGB16 TIFF with the selected ICC profile. The working
/// float buffer is quantized only here, after the complete edit pipeline.
pub fn export_tiff(doc: &PhotoDocument, path: &Path) -> CoreResult<()> {
    export_with_options(doc, path, ExportKind::Tiff, &ExportOptions::default(), &RenderCancellation::new())
}

fn encode_tiff_exclusive(rendered: &Rgb32FImage, path: &Path, space: ExportColorSpace, cancel: &RenderCancellation) -> CoreResult<()> {
    let mut samples = Vec::with_capacity(rendered.as_raw().len());
    for chunk in rendered.as_raw().chunks(65536) {
        cancel.check()?;
        samples.extend(chunk.iter().map(|v| (v.clamp(0.0, 1.0) * 65535.0).round_ties_even() as u16));
    }
    let profile = icc_profile(space)?;
    persist_exclusive(path, cancel, |file| {
        let mut encoder = tiff::encoder::TiffEncoder::new(file)?
            .with_compression(tiff::encoder::Compression::Deflate(tiff::encoder::DeflateLevel::Balanced));
        let mut image = encoder.new_image::<tiff::encoder::colortype::RGB16>(rendered.width(), rendered.height())?;
        image.encoder().write_tag(tiff::tags::Tag::IccProfile, profile.as_slice())?;
        image.write_data(&samples)?;
        Ok(())
    })
}

fn write_u16(out: &mut impl Write, value: u16) -> std::io::Result<()> { out.write_all(&value.to_be_bytes()) }
fn write_u32(out: &mut impl Write, value: u32) -> std::io::Result<()> { out.write_all(&value.to_be_bytes()) }

fn write_plane(out: &mut impl Write, image: &RgbImage, channel: usize, cancel: &RenderCancellation) -> CoreResult<()> {
    let mut plane = [0u8; 16 * 1024];
    for chunk in image.as_raw().chunks(plane.len() * 3) {
        cancel.check()?;
        for (index, pixel) in chunk.chunks_exact(3).enumerate() { plane[index] = pixel[channel]; }
        out.write_all(&plane[..chunk.len() / 3])?;
    }
    Ok(())
}

fn layer_record(out: &mut impl Write, width: u32, height: u32, plane_bytes: u32, name: &[u8]) -> std::io::Result<()> {
    write_u32(out, 0)?; write_u32(out, 0)?; write_u32(out, height)?; write_u32(out, width)?;
    write_u16(out, 3)?;
    for channel in 0..3 { write_u16(out, channel)?; write_u32(out, plane_bytes + 2)?; }
    out.write_all(b"8BIMnorm")?;
    out.write_all(&[255, 0, 0, 0])?; // opaque, unclipped, visible, reserved
    let padded_name = (name.len() + 1).div_ceil(4) * 4;
    write_u32(out, 8 + padded_name as u32)?;
    write_u32(out, 0)?; // no layer mask
    write_u32(out, 0)?; // default blending ranges
    out.write_all(&[name.len() as u8])?;
    out.write_all(name)?;
    out.write_all(&[0; 3][..padded_name - name.len() - 1])?;
    Ok(())
}

/// Strict PSD v1 subset: RGB8, two opaque normal pixel layers (Original,
/// Edited), merged Edited pixels and sRGB ICC. Parameters stay in .chromora.
/// Layout follows Adobe's PSD specification; no adjustment/text/AI layers.
fn validate_psd_size(width: u32, height: u32, icc_bytes: usize) -> CoreResult<()> {
    if width == 0 || height == 0 || width > 30_000 || height > 30_000 {
        return Err(CoreError::InvalidExport("PSD 单边尺寸必须为 1..=30000 像素。".into()));
    }
    let size = 214u64 + (icc_bytes + icc_bytes % 2) as u64 + 9 * u64::from(width) * u64::from(height);
    if size > 2_147_483_647 {
        return Err(CoreError::InvalidExport("双图层 PSD 将超过 2GB，请改用 TIFF 或降低图像尺寸。".into()));
    }
    Ok(())
}

pub fn export_psd(doc: &PhotoDocument, path: &Path) -> CoreResult<()> {
    export_with_options(doc, path, ExportKind::Psd, &ExportOptions::default(), &RenderCancellation::new())
}

fn export_psd_cancellable(doc: &PhotoDocument, path: &Path, cancel: &RenderCancellation) -> CoreResult<()> {
    let profile = ColorProfile::new_srgb().encode()?;
    let (out_width, out_height) = super::output_dimensions(doc.width(), doc.height(), &doc.settings.geometry);
    validate_psd_size(out_width, out_height, profile.len())?;
    let mut hydrated;
    let doc = if doc.working_image().pixels.width() == 0 {
        hydrated = doc.clone();
        super::hydrate_document(&mut hydrated)?;
        &hydrated
    } else { doc };
    cancel.check()?;
    let edited = super::render_preview_at_cancellable(doc, None, cancel)?;
    let original = super::render_original_cancellable(doc, None, cancel)?;
    let (width, height) = edited.dimensions();
    if width == 0 || height == 0 || width > 30_000 || height > 30_000 {
        return Err(CoreError::InvalidExport("PSD 单边尺寸必须为 1..=30000 像素。".into()));
    }
    let plane_bytes = width.checked_mul(height).ok_or_else(|| CoreError::InvalidExport("PSD 尺寸溢出。".into()))?;
    let layer_info_length = plane_bytes.checked_mul(6).and_then(|v| v.checked_add(154))
        .ok_or_else(|| CoreError::InvalidExport("PSD 图层数据超过 4GB。".into()))?;
    let layer_mask_length = layer_info_length.checked_add(8)
        .ok_or_else(|| CoreError::InvalidExport("PSD 图层数据超过 4GB。".into()))?;
    persist_exclusive(path, cancel, |file| {
        let mut out = BufWriter::new(file);
        out.write_all(b"8BPS")?; write_u16(&mut out, 1)?; out.write_all(&[0; 6])?;
        write_u16(&mut out, 3)?; write_u32(&mut out, height)?; write_u32(&mut out, width)?;
        write_u16(&mut out, 8)?; write_u16(&mut out, 3)?;
        write_u32(&mut out, 0)?; // RGB has no color-mode data
        let padded_icc = profile.len() + profile.len() % 2;
        write_u32(&mut out, (12 + padded_icc) as u32)?;
        out.write_all(b"8BIM")?; write_u16(&mut out, 1039)?; out.write_all(&[0, 0])?;
        write_u32(&mut out, profile.len() as u32)?; out.write_all(&profile)?;
        if profile.len() % 2 != 0 { out.write_all(&[0])?; }
        write_u32(&mut out, layer_mask_length)?;
        write_u32(&mut out, layer_info_length)?;
        write_u16(&mut out, 2)?;
        // PSD records enumerate layers from the bottom to the top.
        layer_record(&mut out, width, height, plane_bytes, b"Original")?;
        layer_record(&mut out, width, height, plane_bytes, b"Edited")?;
        for image in [&original, &edited] {
            for channel in 0..3 {
                write_u16(&mut out, 0)?; // uncompressed channel
                write_plane(&mut out, image, channel, cancel)?;
            }
        }
        write_u32(&mut out, 0)?; // global layer mask
        write_u16(&mut out, 0)?; // uncompressed merged RGB image
        for channel in 0..3 { write_plane(&mut out, &edited, channel, cancel)?; }
        out.flush()?;
        Ok(())
    })
}

pub fn patchy_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PHOTO_WORKBENCH_PATCHY") { candidates.push(PathBuf::from(path)); }
    if let Some(directory) = std::env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf)) {
        candidates.push(directory.join("Tools/Patchy/patchy.exe"));
    }
    candidates.push(crate::paths::StoragePaths::default_for_platform().root.join("Tools/Patchy/patchy.exe"));
    candidates.into_iter().find(|path| path.is_file())
}

pub fn open_in_patchy(doc: &PhotoDocument, tiff_path: &Path) -> CoreResult<()> {
    super::project::protect_original_destinations(doc, tiff_path)?;
    let patchy = patchy_path().ok_or_else(|| CoreError::ExternalTool("未找到 Patchy，请恢复 Tools\\Patchy 或设置 PHOTO_WORKBENCH_PATCHY。".into()))?;
    export_tiff(doc, tiff_path)?;
    let mut command = Command::new(&patchy);
    command.arg(tiff_path.canonicalize()?).current_dir(patchy.parent().unwrap_or_else(|| Path::new(".")))
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // no console helper window
    }
    command.spawn().map_err(|error| CoreError::ExternalTool(format!("无法启动 Patchy：{error}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::import_image;
    use image::{ImageDecoder, ImageReader, RgbImage};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn cancellation_after_encoding_removes_temporary_file_before_publication() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("cancelled.png");
        let cancel = RenderCancellation::new();
        let result = persist_exclusive(&path, &cancel, |file| {
            file.write_all(b"encoder output")?;
            cancel.cancel();
            Ok(())
        });
        assert!(matches!(result, Err(CoreError::Cancelled)));
        assert!(!path.exists());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[test]
    fn psd_size_limit_is_checked_before_allocating_image_buffers() {
        assert!(validate_psd_size(6000, 4000, 612).is_ok());
        assert!(validate_psd_size(30000, 30000, 612).is_err());
        assert!(validate_psd_size(30001, 1, 612).is_err());
    }

    #[test]
    fn jpeg_is_decodable_and_existing_destination_is_not_overwritten() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("out.jpg");
        let source_path = temp.path().join("source.png");
        RgbImage::from_fn(16, 12, |x, y| image::Rgb([x as u8, y as u8, 90]))
            .save(&source_path)
            .unwrap();
        let doc = import_image(&source_path, &temp.path().join("originals")).unwrap();
        export_jpeg(&doc, &path, DEFAULT_JPEG_QUALITY).unwrap();
        let decoded = image::open(&path).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (16, 12));
        let mut decoder = ImageReader::open(&path)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        assert!(decoder.icc_profile().unwrap().is_some());
        let before = fs::read(&path).unwrap();
        assert!(export_jpeg(&doc, &path, DEFAULT_JPEG_QUALITY).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn tiff_export_is_rgb16_with_profile_and_retains_deep_samples() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.tif");
        let samples = [1000u16, 1001, 1002, 30000, 30001, 30002];
        tiff::encoder::TiffEncoder::new(fs::File::create(&source).unwrap()).unwrap()
            .write_image::<tiff::encoder::colortype::RGB16>(2, 1, &samples).unwrap();
        let doc = import_image(&source, &temp.path().join("originals")).unwrap();
        let output = temp.path().join("output.tif");
        export_tiff(&doc, &output).unwrap();
        let mut reader = tiff::decoder::Decoder::new(std::io::BufReader::new(fs::File::open(&output).unwrap())).unwrap();
        assert_eq!(reader.colortype().unwrap(), tiff::ColorType::RGB(16));
        assert!(!reader.get_tag_u8_vec(tiff::tags::Tag::IccProfile).unwrap().is_empty());
        match reader.read_image().unwrap() {
            tiff::decoder::DecodingResult::U16(actual) => assert_eq!(actual, samples),
            _ => panic!("expected RGB16"),
        }
        let before = fs::read(&output).unwrap();
        assert!(export_tiff(&doc, &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), before);
    }

    #[test]
    fn psd_contains_two_named_layers_and_an_embedded_profile() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.png");
        RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30])).save(&source).unwrap();
        let doc = import_image(&source, &temp.path().join("originals")).unwrap();
        let output = temp.path().join("output.psd");
        export_psd(&doc, &output).unwrap();
        let bytes = fs::read(&output).unwrap();
        assert_eq!(&bytes[..4], b"8BPS");
        assert!(bytes.windows(8).any(|v| v == b"Original"));
        assert!(bytes.windows(6).any(|v| v == b"Edited"));
        assert!(bytes.windows(4).any(|v| v == b"acsp"));
        assert!(export_psd(&doc, &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), bytes);
    }
}
