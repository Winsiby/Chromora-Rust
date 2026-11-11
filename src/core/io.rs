use std::path::Path;

use image::{ColorType, DynamicImage, GenericImageView, ImageDecoder, ImageFormat, ImageReader, Rgb32FImage};
use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};

use super::{CaptureInfo, ColorConversionState, ColorPipeline, CoreError, CoreResult};
use super::metadata::parse_capture_info;

pub(crate) struct DecodedImage {
    pub pixels: Rgb32FImage,
    pub capture_info: CaptureInfo,
}

#[cfg(test)]
pub(crate) fn decode_source(path: &Path) -> CoreResult<DecodedImage> {
    decode_source_with_pipeline(path, ColorPipeline::FloatSrgb)
}

struct IccFailure { stage: &'static str, message: String }

impl IccFailure {
    fn at(stage: &'static str, error: impl std::fmt::Display) -> Self {
        Self { stage, message: error.to_string() }
    }
    fn report(self, path: &Path, pipeline: ColorPipeline, state: &mut ColorConversionState) -> CoreResult<()> {
        if pipeline != ColorPipeline::LegacySrgb {
            return Err(CoreError::InvalidIcc { path: path.to_path_buf(), stage: self.stage, message: self.message });
        }
        *state = ColorConversionState::LegacyIccFallback { reason: format!("{}失败：{}", self.stage, self.message) };
        Ok(())
    }
}

fn convert16(profile: &ColorProfile, source: &[u16]) -> Result<Vec<u16>, IccFailure> {
    let transform = profile.create_transform_16bit(Layout::Rgb, &ColorProfile::new_srgb(), Layout::Rgb, TransformOptions::default())
        .map_err(|error| IccFailure::at("创建 RGB 到 sRGB 转换", error))?;
    let mut converted = vec![0; source.len()];
    transform.transform(source, &mut converted).map_err(|error| IccFailure::at("执行 RGB 到 sRGB 转换", error))?;
    Ok(converted)
}

pub(crate) fn decode_source_with_pipeline(path: &Path, pipeline: ColorPipeline) -> CoreResult<DecodedImage> {
    if super::raw::is_raw_path(path) {
        return super::raw::decode_raw(path);
    }
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let format = match reader.format() {
        Some(format @ (ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Tiff)) => format,
        _ => return Err(CoreError::UnsupportedFormat(path.to_path_buf())),
    };
    let mut decoder = reader.into_decoder()?;
    let source_dimensions = decoder.dimensions();
    let color_type = decoder.color_type();
    if !matches!(color_type, ColorType::L8 | ColorType::La8 | ColorType::Rgb8 | ColorType::Rgba8
        | ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16) {
        return Err(CoreError::UnsupportedBitDepth(color_type));
    }
    let orientation = decoder.orientation()?;
    let profile_read = if format == ImageFormat::Tiff {
        // image 0.25.8 asks for Unknown(34675), but tiff 0.10.3 stores
        // that directory entry under its known IccProfile variant.
        let file = std::io::BufReader::new(std::fs::File::open(path)?);
        tiff::decoder::Decoder::new(file)?
            .find_tag(tiff::tags::Tag::IccProfile)
            .and_then(|value| value.map(|value| value.into_u8_vec()).transpose())
            .map_err(|error| IccFailure::at("读取 ICC", error))
    } else {
        decoder.icc_profile().map_err(|error| IccFailure::at("读取 ICC", error))
    };
    let mut color_conversion = ColorConversionState::AssumedSrgb;
    let profile_bytes = match profile_read {
        Ok(bytes) => bytes,
        Err(failure) => { failure.report(path, pipeline, &mut color_conversion)?; None },
    };
    let profile = match profile_bytes.as_deref().map(ColorProfile::new_from_slice).transpose() {
        Ok(profile) => profile,
        Err(error) => { IccFailure::at("解析 ICC", error).report(path, pipeline, &mut color_conversion)?; None },
    };
    if pipeline != ColorPipeline::LegacySrgb {
        if let Some(profile) = &profile {
            if profile.color_space != DataColorSpace::Rgb {
                return Err(CoreError::InvalidIcc { path: path.to_path_buf(), stage: "ICC 色彩空间支持检查",
                    message: format!("此输入路径仅支持 RGB ICC，实际为 {:?}", profile.color_space) });
            }
        }
    }
    let mut decoded = if format == ImageFormat::Jpeg && matches!(pipeline, ColorPipeline::AccurateSrgb | ColorPipeline::FloatSrgb) {
        drop(decoder);
        let rgb = super::jpeg::decode(path)?;
        if rgb.dimensions() != source_dimensions {
            return Err(CoreError::InvalidJpeg("JPEG 解码器与元数据尺寸不一致".into()));
        }
        DynamicImage::ImageRgb8(rgb)
    } else { DynamicImage::from_decoder(decoder)? };
    decoded.apply_orientation(orientation);
    let dimensions = decoded.dimensions();
    let pixels = if matches!(color_type, ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16) {
        let rgba = decoded.to_rgba16();
        if rgba.pixels().any(|p| p[3] != u16::MAX) {
            return Err(CoreError::UnsupportedTransparency(path.to_path_buf()));
        }
        let mut rgb: Vec<u16> = rgba.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
        // Preserve the source's 16-bit precision through ICC conversion.
        if let Some(profile) = &profile {
            match convert16(profile, &rgb) {
                Ok(converted) => { rgb = converted; color_conversion = ColorConversionState::ConvertedToSrgb; },
                Err(failure) => failure.report(path, pipeline, &mut color_conversion)?,
            }
        }
        Rgb32FImage::from_raw(dimensions.0, dimensions.1, rgb.into_iter().map(|v| f32::from(v) / 65535.0).collect()).unwrap()
    } else {
        let rgba = decoded.to_rgba8();
        if format != ImageFormat::Png && rgba.pixels().any(|p| p[3] != u8::MAX) {
            return Err(CoreError::UnsupportedTransparency(path.to_path_buf()));
        }
        if pipeline != ColorPipeline::LegacySrgb {
            // Replication preserves each original RGB8 code exactly while avoiding
            // another RGB8 quantization during the color transform.
            let mut rgb: Vec<u16> = rgba.pixels().flat_map(|p| [u16::from(p[0]) * 257, u16::from(p[1]) * 257, u16::from(p[2]) * 257]).collect();
            if let Some(profile) = &profile {
                match convert16(profile, &rgb) {
                    Ok(converted) => { rgb = converted; color_conversion = ColorConversionState::ConvertedToSrgb; },
                    Err(failure) => failure.report(path, pipeline, &mut color_conversion)?,
                }
            }
            if format == ImageFormat::Png {
                for (pixel, rgba_pixel) in rgb.chunks_exact_mut(3).zip(rgba.pixels()) {
                    let alpha = u32::from(rgba_pixel[3]);
                    for sample in pixel {
                        *sample = ((u32::from(*sample) * alpha + 65535 * (255 - alpha) + 127) / 255) as u16;
                    }
                }
            }
            Rgb32FImage::from_raw(dimensions.0, dimensions.1, rgb.into_iter().map(|v| f32::from(v) / 65535.0).collect()).unwrap()
        } else {
            let mut rgb: Vec<u8> = rgba.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
            if let Some(profile) = &profile {
                let conversion = profile.create_transform_8bit(Layout::Rgb, &ColorProfile::new_srgb(), Layout::Rgb, TransformOptions::default())
                    .map_err(|error| IccFailure::at("创建 RGB 到 sRGB 转换", error))
                    .and_then(|transform| {
                        let mut converted = vec![0; rgb.len()];
                        transform.transform(&rgb, &mut converted).map_err(|error| IccFailure::at("执行 RGB 到 sRGB 转换", error))?;
                        Ok(converted)
                    });
                match conversion {
                    Ok(converted) => { rgb = converted; color_conversion = ColorConversionState::ConvertedToSrgb; },
                    Err(failure) => failure.report(path, pipeline, &mut color_conversion)?,
                }
            }
            if format == ImageFormat::Png {
                // Compatibility: preserve the original encoded-sRGB white composite
                // and its RGB8 rounding for every old project.
                for (pixel, rgba_pixel) in rgb.chunks_exact_mut(3).zip(rgba.pixels()) {
                    let alpha = u32::from(rgba_pixel[3]);
                    for sample in pixel {
                        *sample = ((u32::from(*sample) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
                    }
                }
            }
            Rgb32FImage::from_raw(dimensions.0, dimensions.1, rgb.into_iter().map(|v| f32::from(v) / 255.0).collect()).unwrap()
        }
    };
    let mut capture_info = parse_capture_info(path, format, source_dimensions, dimensions, color_type, profile_bytes.as_deref());
    capture_info.color_conversion = color_conversion;
    Ok(DecodedImage { pixels, capture_info })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;
    use tiff::encoder::{TiffEncoder, colortype};

    #[test]
    fn tiff16_samples_survive_without_rgb8_quantization() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("深色阶.tif");
        let values = [1000u16, 1001, 1002, 30000, 30001, 30002];
        TiffEncoder::new(File::create(&path).unwrap()).unwrap()
            .write_image::<colortype::RGB16>(2, 1, &values).unwrap();
        let decoded = decode_source(&path).unwrap();
        for (actual, expected) in decoded.pixels.as_raw().iter().zip(values) {
            assert!((actual * 65535.0 - f32::from(expected)).abs() < 0.01);
        }
        assert_eq!(decoded.capture_info.bit_depth_per_sample, Some(16));
    }

    #[test]
    fn transparent_tiff_is_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("transparent.tif");
        TiffEncoder::new(File::create(&path).unwrap()).unwrap()
            .write_image::<colortype::RGBA16>(1, 1, &[100, 200, 300, 32000]).unwrap();
        assert!(matches!(decode_source(&path), Err(CoreError::UnsupportedTransparency(_))));
    }

    #[test]
    fn tiff16_icc_conversion_keeps_more_than_rgb8_precision() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("linear-rgb16.tif");
        let mut linear = ColorProfile::new_srgb();
        linear.cicp = None;
        linear.red_trc = Some(moxcms::ToneReprCurve::Lut(vec![256]));
        linear.green_trc = linear.red_trc.clone();
        linear.blue_trc = linear.red_trc.clone();
        let profile = linear.encode().unwrap();
        let mut encoder = TiffEncoder::new(File::create(&path).unwrap()).unwrap();
        let mut image = encoder.new_image::<colortype::RGB16>(1, 1).unwrap();
        image.encoder().write_tag(tiff::tags::Tag::IccProfile, profile.as_slice()).unwrap();
        image.write_data(&[1000u16, 1001, 1002]).unwrap();
        let decoded = decode_source(&path).unwrap();
        assert!(decoded.capture_info.icc_profile.embedded);
        assert_eq!(decoded.capture_info.icc_profile.bytes, Some(profile.len()));
        for (sample, source) in decoded.pixels.as_raw().iter().zip([1000u16, 1001, 1002]) {
            let linear = f32::from(source) / 65535.0;
            let expected = 1.055 * linear.powf(1.0 / 2.4) - 0.055;
            assert!((sample - expected).abs() < 4.0 / 65535.0, "ICC output {sample}, expected {expected}");
        }
        assert!(decoded.pixels.as_raw().windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn rgb8_linear_icc_uses_deep_srgb_values_without_a_second_rgb8_quantization() {
        use image::ImageEncoder;
        let dir = tempdir().unwrap();
        let path = dir.path().join("linear-rgb8.png");
        let mut profile = ColorProfile::new_srgb();
        profile.cicp = None;
        profile.red_trc = Some(moxcms::ToneReprCurve::Lut(vec![256]));
        profile.green_trc = profile.red_trc.clone();
        profile.blue_trc = profile.red_trc.clone();
        let mut encoder = image::codecs::png::PngEncoder::new(File::create(&path).unwrap());
        encoder.set_icc_profile(profile.encode().unwrap()).unwrap();
        encoder.write_image(&[1, 1, 1, 30, 30, 30, 127, 127, 127], 3, 1, image::ExtendedColorType::Rgb8).unwrap();
        let decoded = decode_source(&path).unwrap();
        assert_eq!(decoded.capture_info.color_conversion, ColorConversionState::ConvertedToSrgb);
        for (actual, code) in decoded.pixels.as_raw().iter().zip([1u8, 1, 1, 30, 30, 30, 127, 127, 127]) {
            let linear = f64::from(code) / 255.0;
            let expected = if linear <= 0.0031308 { 12.92 * linear } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
            assert!((f64::from(*actual) - expected).abs() < 4.0 / 65535.0, "code={code}, actual={actual}, expected={expected}");
        }
        assert!(decoded.pixels.as_raw().iter().any(|v| (v * 255.0 - (v * 255.0).round()).abs() > 0.01));
    }

    #[test]
    fn malformed_icc_is_rejected_for_new_imports_and_explicitly_marked_in_legacy() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad-profile.tif");
        let mut encoder = TiffEncoder::new(File::create(&path).unwrap()).unwrap();
        let mut image = encoder.new_image::<colortype::RGB16>(1, 1).unwrap();
        image.encoder().write_tag(tiff::tags::Tag::IccProfile, b"not an ICC profile".as_slice()).unwrap();
        image.write_data(&[1001, 2002, 3003]).unwrap();
        let before = std::fs::read(&path).unwrap();
        let originals = dir.path().join("originals");
        assert!(matches!(super::super::import_image(&path, &originals), Err(CoreError::InvalidIcc { .. })));
        assert!(!originals.exists());
        let legacy = decode_source_with_pipeline(&path, ColorPipeline::LegacySrgb).unwrap();
        assert!(matches!(legacy.capture_info.color_conversion, ColorConversionState::LegacyIccFallback { .. }));
        assert_eq!(legacy.pixels.get_pixel(0, 0).0, [1001.0 / 65535.0, 2002.0 / 65535.0, 3003.0 / 65535.0]);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
