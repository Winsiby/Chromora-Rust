use std::{fs::File, io::BufReader, path::Path};

use exif::{Field, In, Reader, Tag, Value};
use image::{ColorType, ImageFormat};
use moxcms::{ColorProfile, ProfileText};

#[derive(Clone, Debug, PartialEq)]
pub struct PhotoInfo {
    /// Raw EXIF DateTimeOriginal text; timezone is only present if recorded separately by the source.
    pub captured_at_exif: Option<String>,
    pub captured_at_offset_exif: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens_model: Option<String>,
    pub focal_length_mm: Option<f64>,
    pub equivalent_focal_length_35mm: Option<f64>,
    pub aperture_f_number: Option<f64>,
    pub exposure_time_seconds: Option<f64>,
    pub exposure_compensation_ev: Option<f64>,
    pub iso: Option<u32>,
    pub white_balance: Option<WhiteBalance>,
    /// No standard cross-vendor Picture Style/Look tag is used; vendor handlers are future work.
    pub vendor_style_name: Option<String>,
    pub file_format: CaptureFileFormat,
    pub file_size_bytes: Option<u64>,
    pub source_dimensions: (u32, u32),
    pub oriented_dimensions: (u32, u32),
    /// EXIF orientation value as recorded in the file; None if absent/invalid.
    pub orientation: Option<u8>,
    pub bit_depth_per_sample: Option<u8>,
    pub source_has_alpha: bool,
    pub icc_profile: IccProfileSummary,
    pub color_conversion: ColorConversionState,
}

/// Backwards-compatible alias; PhotoInfo is the single canonical snapshot type.
pub type CaptureInfo = PhotoInfo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteBalanceMode {
    Auto,
    Manual,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WhiteBalance {
    pub mode: Option<WhiteBalanceMode>,
    pub temperature_kelvin: Option<u32>,
    pub tint: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureFileFormat {
    Jpeg,
    Png,
    Tiff,
    Raw,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IccProfileSummary {
    pub embedded: bool,
    pub bytes: Option<usize>,
    /// ICC description when the parser can decode it as a plain string.
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorConversionState {
    AssumedSrgb,
    ConvertedToSrgb,
    RawDevelopedSrgb,
    /// Older projects retain their original pixel interpretation, with an explicit warning.
    LegacyIccFallback { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingColorSpace {
    SRgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingSampleFormat {
    U8,
    F32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkingImageMetadata {
    pub color_space: WorkingColorSpace,
    pub sample_format: WorkingSampleFormat,
    /// Output pixels are opaque; transparent raster sources are composited onto white.
    pub has_alpha: bool,
    pub source_format: CaptureFileFormat,
    pub source_dimensions: (u32, u32),
    pub oriented_dimensions: (u32, u32),
    pub exif_orientation: Option<u8>,
    pub source_bit_depth_per_sample: Option<u8>,
    pub source_icc_profile: IccProfileSummary,
}

impl WorkingImageMetadata {
    pub(crate) fn for_capture(info: &PhotoInfo) -> Self {
        Self {
            color_space: WorkingColorSpace::SRgb,
            sample_format: WorkingSampleFormat::F32,
            has_alpha: false,
            source_format: info.file_format,
            source_dimensions: info.source_dimensions,
            oriented_dimensions: info.oriented_dimensions,
            exif_orientation: info.orientation,
            source_bit_depth_per_sample: info.bit_depth_per_sample,
            source_icc_profile: info.icc_profile.clone(),
        }
    }
}

fn field(exif: &exif::Exif, tag: Tag) -> Option<&Field> {
    exif.get_field(tag, In::PRIMARY)
        .or_else(|| exif.fields().find(|field| field.tag == tag))
}

fn text(exif: &exif::Exif, tag: Tag) -> Option<String> {
    let field = field(exif, tag)?;
    match &field.value {
        Value::Ascii(items) => items
            .first()
            .and_then(|item| std::str::from_utf8(item).ok())
            .map(|value| value.trim_end_matches('\0').trim().to_owned())
            .filter(|s| !s.is_empty()),
        _ => None,
    }
}

fn numeric(exif: &exif::Exif, tag: Tag) -> Option<f64> {
    let field = field(exif, tag)?;
    match &field.value {
        Value::Rational(values) => values.first().map(|v| v.to_f64()),
        Value::SRational(values) => values.first().map(|v| v.to_f64()),
        Value::Short(values) => values.first().map(|v| f64::from(*v)),
        Value::Long(values) => values.first().map(|v| *v as f64),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn uint(exif: &exif::Exif, tag: Tag) -> Option<u32> {
    let field = field(exif, tag)?;
    field.value.get_uint(0)
}

fn icc_summary(profile_bytes: Option<&[u8]>) -> IccProfileSummary {
    let Some(bytes) = profile_bytes else {
        return IccProfileSummary {
            embedded: false,
            bytes: None,
            description: None,
        };
    };
    let description = ColorProfile::new_from_slice(bytes)
        .ok()
        .and_then(|profile| match profile.description {
            Some(ProfileText::PlainString(name)) if !name.trim().is_empty() => Some(name),
            _ => None,
        });
    IccProfileSummary {
        embedded: true,
        bytes: Some(bytes.len()),
        description,
    }
}

pub(crate) fn parse_capture_info(
    path: &Path,
    format: ImageFormat,
    source_dimensions: (u32, u32),
    oriented_dimensions: (u32, u32),
    source_color_type: ColorType,
    icc_profile_bytes: Option<&[u8]>,
) -> CaptureInfo {
    // Metadata is advisory: malformed or absent EXIF never prevents valid pixels opening.
    let exif = File::open(path).ok().and_then(|file| {
        Reader::new()
            .read_from_container(&mut BufReader::new(file))
            .ok()
    });
    let get_text = |tag| exif.as_ref().and_then(|x| text(x, tag));
    let get_number = |tag| exif.as_ref().and_then(|x| numeric(x, tag));
    let get_uint = |tag| exif.as_ref().and_then(|x| uint(x, tag));
    let orientation = get_uint(Tag::Orientation)
        .and_then(|v| u8::try_from(v).ok())
        .filter(|v| (1..=8).contains(v));
    let white_balance_mode = get_uint(Tag::WhiteBalance).and_then(|v| match v {
        0 => Some(WhiteBalanceMode::Auto),
        1 => Some(WhiteBalanceMode::Manual),
        _ => None,
    });
    let white_balance = if white_balance_mode.is_some() {
        Some(WhiteBalance {
            mode: white_balance_mode,
            temperature_kelvin: None,
            tint: None,
        })
    } else {
        None
    };
    let bit_depth_per_sample = match source_color_type {
        ColorType::L8 | ColorType::La8 | ColorType::Rgb8 | ColorType::Rgba8 => Some(8),
        ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16 => Some(16),
        ColorType::Rgb32F | ColorType::Rgba32F => Some(32),
        _ => None,
    };
    let file_format = match format {
        ImageFormat::Jpeg => CaptureFileFormat::Jpeg,
        ImageFormat::Png => CaptureFileFormat::Png,
        ImageFormat::Tiff => CaptureFileFormat::Tiff,
        _ => unreachable!("decoder format was checked before capture metadata parsing"),
    };
    let file_size_bytes = path.metadata().ok().map(|m| m.len());

    PhotoInfo {
        captured_at_exif: get_text(Tag::DateTimeOriginal),
        captured_at_offset_exif: get_text(Tag::OffsetTimeOriginal),
        make: get_text(Tag::Make),
        model: get_text(Tag::Model),
        lens_model: get_text(Tag::LensModel),
        focal_length_mm: get_number(Tag::FocalLength),
        equivalent_focal_length_35mm: get_number(Tag::FocalLengthIn35mmFilm),
        aperture_f_number: get_number(Tag::FNumber),
        exposure_time_seconds: get_number(Tag::ExposureTime),
        exposure_compensation_ev: get_number(Tag::ExposureBiasValue),
        iso: get_uint(Tag::PhotographicSensitivity).or_else(|| get_uint(Tag::ISOSpeed)),
        white_balance,
        vendor_style_name: None,
        file_format,
        file_size_bytes,
        source_dimensions,
        oriented_dimensions,
        orientation,
        bit_depth_per_sample,
        source_has_alpha: source_color_type.has_alpha(),
        icc_profile: icc_summary(icc_profile_bytes),
        color_conversion: ColorConversionState::AssumedSrgb,
    }
}
