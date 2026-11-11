use std::{path::PathBuf, sync::Arc};

use image::{Rgb32FImage, RgbImage};
use serde::{Deserialize, Serialize};

use super::metadata::{PhotoInfo, WorkingImageMetadata};
use super::{lut::Lut3d, settings::AdjustmentSettings};

pub const SCHEMA_VERSION: u32 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorPipeline {
    LegacySrgb,
    HqSrgb,
    AccurateSrgb,
    FloatSrgb,
}

#[derive(Clone, Debug)]
pub struct PhotoDocument {
    /// The originally selected source image path (kept unchanged on disk).
    pub source_path: PathBuf,
    /// A byte-for-byte private copy kept under the application's originals folder.
    pub original_copy_path: Option<PathBuf>,
    /// SHA-256 of the original source bytes, used to detect missing/changed files.
    pub source_sha256: String,
    pub(crate) color_pipeline: ColorPipeline,
    /// EXIF-oriented, opaque sRGB RGB pixels. Exposure is applied only at render time.
    pub original_rgb: Arc<RgbImage>,
    /// Full-precision encoded sRGB working source, shared by parameter snapshots.
    working_image: WorkingImage,
    /// Read-only capture/file facts parsed from the source. GPS and serial numbers are omitted.
    photo_info: PhotoInfo,
    pub exposure_ev: f32,
    pub settings: AdjustmentSettings,
    pub lut: Option<Arc<Lut3d>>,
    pub lut_path: Option<PathBuf>,
    /// Optional pinned legacy RAW development used to preserve old project rendering.
    pub render_source_path: Option<PathBuf>,
    pub render_source_sha256: Option<String>,
    /// Neutral display sources; shared snapshots reuse pixels rather than resampling.
    pub(crate) preview_source: Option<Arc<Rgb32FImage>>,
    pub thumbnail_rgb: Option<Arc<RgbImage>>,
}

#[derive(Clone, Debug)]
pub struct WorkingImage {
    pub pixels: Arc<Rgb32FImage>,
    pub metadata: WorkingImageMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ProjectFile {
    pub schema_version: u32,
    pub source_path: PathBuf,
    pub original_copy_path: Option<PathBuf>,
    pub source_sha256: String,
    pub width: u32,
    pub height: u32,
    pub exposure_ev: f32,
}

impl PhotoDocument {
    #[cfg(test)]
    pub(crate) fn from_decoded(
        source_path: PathBuf,
        original_copy_path: Option<PathBuf>,
        source_sha256: String,
        pixels: Arc<RgbImage>,
        photo_info: PhotoInfo,
        exposure_ev: f32,
    ) -> Self {
        let floats = Rgb32FImage::from_fn(pixels.width(), pixels.height(), |x, y| {
            image::Rgb(pixels.get_pixel(x, y).0.map(|v| v as f32 / 255.0))
        });
        Self::from_float_decoded(source_path, original_copy_path, source_sha256,
            Arc::new(floats), photo_info, exposure_ev)
    }

    #[cfg(test)]
    pub(crate) fn from_float_decoded(
        source_path: PathBuf,
        original_copy_path: Option<PathBuf>,
        source_sha256: String,
        pixels: Arc<Rgb32FImage>,
        photo_info: PhotoInfo,
        exposure_ev: f32,
    ) -> Self {
        Self::from_float_decoded_with_pipeline(source_path, original_copy_path, source_sha256,
            pixels, photo_info, exposure_ev, ColorPipeline::LegacySrgb)
    }

    pub(crate) fn from_float_decoded_with_pipeline(
        source_path: PathBuf,
        original_copy_path: Option<PathBuf>,
        source_sha256: String,
        pixels: Arc<Rgb32FImage>,
        photo_info: PhotoInfo,
        exposure_ev: f32,
        color_pipeline: ColorPipeline,
    ) -> Self {
        let rgb8 = RgbImage::from_fn(pixels.width(), pixels.height(), |x, y| {
            image::Rgb(pixels.get_pixel(x, y).0.map(|v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8))
        });
        let working_image = WorkingImage {
            pixels,
            metadata: WorkingImageMetadata::for_capture(&photo_info),
        };
        let settings = AdjustmentSettings { exposure: exposure_ev, ..Default::default() };
        Self {
            source_path,
            original_copy_path,
            source_sha256,
            color_pipeline,
            original_rgb: Arc::new(rgb8),
            working_image,
            photo_info,
            exposure_ev,
            settings,
            lut: None,
            lut_path: None,
            render_source_path: None,
            render_source_sha256: None,
            preview_source: None,
            thumbnail_rgb: None,
        }
    }

    pub fn width(&self) -> u32 {
        self.photo_info.oriented_dimensions.0
    }
    pub fn height(&self) -> u32 {
        self.photo_info.oriented_dimensions.1
    }
    pub fn photo_info(&self) -> &PhotoInfo {
        &self.photo_info
    }
    pub fn color_pipeline(&self) -> ColorPipeline {
        self.color_pipeline
    }
    pub fn capture_info(&self) -> &PhotoInfo {
        &self.photo_info
    }
    pub fn working_image(&self) -> &WorkingImage {
        &self.working_image
    }

    /// Keep the source identity and parameters while freeing inactive full-size buffers.
    pub fn evict_pixels(&mut self) {
        self.original_rgb = Arc::new(RgbImage::new(0, 0));
        self.working_image.pixels = Arc::new(Rgb32FImage::new(0, 0));
        self.preview_source = None;
    }

    pub(crate) fn restore_pixels(&mut self, pixels: Rgb32FImage) {
        self.original_rgb = Arc::new(RgbImage::from_fn(pixels.width(), pixels.height(), |x, y| {
            image::Rgb(pixels.get_pixel(x, y).0.map(|v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8))
        }));
        self.working_image.pixels = Arc::new(pixels);
        self.preview_source = None;
    }

    /// A renderer may finish loading after newer parameters arrive. Reuse only
    /// its immutable source caches, never its stale adjustment state.
    pub fn reuse_source_cache(&mut self, loaded: &Self) {
        if self.source_sha256 == loaded.source_sha256
            && self.color_pipeline == loaded.color_pipeline
            && self.render_source_path == loaded.render_source_path
            && self.render_source_sha256 == loaded.render_source_sha256
            && loaded.working_image.pixels.dimensions() == (self.width(), self.height())
        {
            self.original_rgb = loaded.original_rgb.clone();
            self.working_image = loaded.working_image.clone();
            self.preview_source = loaded.preview_source.clone();
            if loaded.thumbnail_rgb.is_some() { self.thumbnail_rgb = loaded.thumbnail_rgb.clone(); }
        }
    }
}
