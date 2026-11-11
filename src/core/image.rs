use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

use image::{Rgb32FImage, RgbImage};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{adjustments, io::decode_source_with_pipeline, lut::load_cube, settings::AdjustmentSettings};
use super::{ColorPipeline, CoreError, CoreResult, PhotoDocument, RenderCancellation};

pub const MIN_EXPOSURE_EV: f32 = -3.0;
pub const MAX_EXPOSURE_EV: f32 = 3.0;
pub const EXPOSURE_STEP_EV: f32 = 0.05;
const EXPOSURE_EPSILON: f32 = 0.0001;

pub(super) fn sha256_file(path: &Path) -> CoreResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Opens a supported source without modifying it, preserving float sRGB working samples.
pub fn open_image(path: &Path) -> CoreResult<PhotoDocument> {
    open_image_with_pipeline(path, ColorPipeline::FloatSrgb)
}

pub fn open_image_with_pipeline(path: &Path, pipeline: ColorPipeline) -> CoreResult<PhotoDocument> {
    let source_sha256 = sha256_file(path)?;
    let decoded = decode_source_with_pipeline(path, pipeline)?;
    Ok(PhotoDocument::from_float_decoded_with_pipeline(
        path.to_path_buf(),
        None,
        source_sha256,
        Arc::new(decoded.pixels),
        decoded.capture_info,
        0.0,
        pipeline,
    ))
}

/// Opens an image and stores a byte-identical source copy in the application's originals folder.
pub fn import_image(path: &Path, originals_dir: &Path) -> CoreResult<PhotoDocument> {
    let mut doc = open_image(path)?;
    fs::create_dir_all(originals_dir)?;
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("img");
    let copy_path = originals_dir.join(format!("{}.{}", Uuid::new_v4(), extension));
    let mut input = File::open(path)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&copy_path)?;
    if let Err(error) = std::io::copy(&mut input, &mut output)
        .and_then(|_| output.flush())
        .and_then(|_| output.sync_all())
    {
        let _ = fs::remove_file(&copy_path);
        return Err(CoreError::Io(error));
    }
    if sha256_file(&copy_path)? != doc.source_sha256 {
        let _ = fs::remove_file(&copy_path);
        return Err(CoreError::MissingOrChangedSource(path.to_path_buf()));
    }
    doc.original_copy_path = Some(copy_path);
    Ok(doc)
}

pub fn set_exposure(doc: &mut PhotoDocument, ev: f32) -> CoreResult<()> {
    validate_exposure(ev)?;
    doc.exposure_ev = ev;
    doc.settings.exposure = ev;
    Ok(())
}

pub(super) fn validate_exposure(ev: f32) -> CoreResult<()> {
    if !ev.is_finite()
        || !(MIN_EXPOSURE_EV..=MAX_EXPOSURE_EV).contains(&ev)
        || ((ev / EXPOSURE_STEP_EV).round() * EXPOSURE_STEP_EV - ev).abs() > EXPOSURE_EPSILON
    {
        return Err(CoreError::InvalidExposure(ev));
    }
    Ok(())
}

pub fn set_settings(doc: &mut PhotoDocument, settings: AdjustmentSettings) -> CoreResult<()> {
    validate_exposure(settings.exposure)?;
    settings.validate()?;
    doc.exposure_ev = settings.exposure;
    doc.settings = settings;
    Ok(())
}

pub fn set_lut(doc: &mut PhotoDocument, path: Option<&Path>) -> CoreResult<()> {
    let lut = path.map(load_cube).transpose()?.map(Arc::new);
    doc.lut_path = path.map(Path::to_path_buf);
    doc.lut = lut;
    Ok(())
}

fn decode_document_source(doc: &PhotoDocument) -> CoreResult<Rgb32FImage> {
    let path = if let Some(path) = &doc.render_source_path {
        if !doc.render_source_sha256.as_ref().is_some_and(|hash| sha256_file(path).is_ok_and(|actual| actual == *hash)) {
            return Err(CoreError::MissingOrChangedSource(path.clone()));
        }
        path.as_path()
    } else if let Some(path) = doc.original_copy_path.as_ref().filter(|path| {
        sha256_file(path).is_ok_and(|hash| hash == doc.source_sha256)
    }) {
        path.as_path()
    } else if sha256_file(&doc.source_path)? == doc.source_sha256 {
        doc.source_path.as_path()
    } else {
        return Err(CoreError::MissingOrChangedSource(doc.source_path.clone()));
    };
    let decoded = decode_source_with_pipeline(path, doc.color_pipeline())?;
    if decoded.pixels.dimensions() != (doc.width(), doc.height()) {
        return Err(CoreError::MissingOrChangedSource(path.to_path_buf()));
    }
    Ok(decoded.pixels)
}

pub fn hydrate_document(doc: &mut PhotoDocument) -> CoreResult<()> {
    if doc.working_image().pixels.width() == 0 {
        doc.restore_pixels(decode_document_source(doc)?);
    }
    Ok(())
}

fn resize_source(source: &Rgb32FImage, edge: u32) -> Rgb32FImage {
    if source.width().max(source.height()) <= edge { return source.clone(); }
    let ratio = edge as f64 / source.width().max(source.height()) as f64;
    let width = (source.width() as f64 * ratio).round_ties_even().max(1.0) as u32;
    let height = (source.height() as f64 * ratio).round_ties_even().max(1.0) as u32;
    let mut pixels = image::imageops::resize(source, width, height, image::imageops::FilterType::Lanczos3);
    for value in pixels.as_mut() { *value = value.clamp(0.0, 1.0); }
    pixels
}

fn rgb8(rgb: &Rgb32FImage) -> RgbImage {
    RgbImage::from_fn(rgb.width(), rgb.height(), |x, y| {
        image::Rgb(rgb.get_pixel(x, y).0.map(|v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8))
    })
}

pub fn prepare_preview(doc: &mut PhotoDocument) -> CoreResult<()> {
    prepare_preview_cancellable(doc, &RenderCancellation::new())
}

pub fn prepare_preview_cancellable(doc: &mut PhotoDocument, cancel: &RenderCancellation) -> CoreResult<()> {
    cancel.check()?;
    hydrate_document(doc)?;
    cancel.check()?;
    if doc.preview_source.is_none() {
        let preview = resize_source(doc.working_image().pixels.as_ref(), 1440);
        cancel.check()?;
        doc.preview_source = Some(Arc::new(preview));
    }
    cancel.check()
}

pub fn prepare_thumbnail(doc: &mut PhotoDocument) -> CoreResult<()> {
    if doc.thumbnail_rgb.is_none() {
        hydrate_document(doc)?;
        doc.thumbnail_rgb = Some(Arc::new(rgb8(&resize_source(doc.working_image().pixels.as_ref(), 240))));
    }
    Ok(())
}

/// Returns encoded sRGB float samples. The same operations serve preview and export.
pub fn render_rgb_f32(doc: &PhotoDocument, preview_edge: Option<u32>) -> CoreResult<Rgb32FImage> {
    render_rgb_f32_cancellable(doc, preview_edge, &RenderCancellation::new())
}

pub fn render_rgb_f32_cancellable(doc: &PhotoDocument, preview_edge: Option<u32>, cancel: &RenderCancellation) -> CoreResult<Rgb32FImage> {
    cancel.check()?;
    doc.settings.validate()?;
    let decoded;
    let source = if preview_edge == Some(1440) && doc.preview_source.is_some() {
        doc.preview_source.as_ref().unwrap().as_ref()
    } else if doc.working_image().pixels.width() == 0 {
        decoded = decode_document_source(doc)?;
        &decoded
    } else {
        doc.working_image().pixels.as_ref()
    };
    cancel.check()?;
    let mut pixels = if let Some(edge) = preview_edge.filter(|edge| *edge > 0 && source.width().max(source.height()) > *edge) {
        resize_source(source, edge)
    } else {
        source.clone()
    };
    // Lanczos can overshoot; decoded working samples use the original 0..1 sRGB domain.
    for (index, value) in pixels.as_mut().iter_mut().enumerate() {
        if index % 65536 == 0 { cancel.check()?; }
        *value = value.clamp(0.0, 1.0);
    }
    adjustments::apply_with_pipeline_cancellable(&mut pixels, &doc.settings, doc.lut.as_deref(), doc.color_pipeline(), cancel)?;
    cancel.check()?;
    super::geometry::apply_geometry(&mut pixels, &doc.settings.geometry)?;
    cancel.check()?;
    Ok(pixels)
}

pub fn render_preview_at(doc: &PhotoDocument, preview_edge: Option<u32>) -> CoreResult<RgbImage> {
    render_preview_at_cancellable(doc, preview_edge, &RenderCancellation::new())
}

pub fn render_preview_at_cancellable(doc: &PhotoDocument, preview_edge: Option<u32>, cancel: &RenderCancellation) -> CoreResult<RgbImage> {
    let rgb = render_rgb_f32_cancellable(doc, preview_edge, cancel)?;
    let mut output = RgbImage::new(rgb.width(), rgb.height());
    for y in 0..rgb.height() {
        cancel.check()?;
        for x in 0..rgb.width() {
            output.put_pixel(x, y, image::Rgb(rgb.get_pixel(x, y).0.map(|v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8)));
        }
    }
    cancel.check()?;
    Ok(output)
}

pub fn render_original(doc: &PhotoDocument, preview_edge: Option<u32>) -> CoreResult<RgbImage> {
    render_original_cancellable(doc, preview_edge, &RenderCancellation::new())
}

pub fn render_original_cancellable(doc: &PhotoDocument, preview_edge: Option<u32>, cancel: &RenderCancellation) -> CoreResult<RgbImage> {
    cancel.check()?;
    let mut neutral = doc.clone();
    neutral.settings = AdjustmentSettings { geometry: doc.settings.geometry, ..Default::default() };
    neutral.exposure_ev = 0.0;
    neutral.lut = None;
    neutral.lut_path = None;
    render_preview_at_cancellable(&neutral, preview_edge, cancel)
}

pub fn render_preview(doc: &PhotoDocument) -> CoreResult<RgbImage> {
    render_preview_at(doc, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use super::super::metadata::parse_capture_info;
    use image::ImageFormat;

    #[test]
    fn prepared_preview_reuses_source_and_matches_uncached_rendering() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("preview.png");
        RgbImage::from_fn(1800,12,|x,y|image::Rgb([(x%256) as u8,(y*17) as u8,90])).save(&source).unwrap();
        let mut doc = open_image(&source).unwrap();
        prepare_thumbnail(&mut doc).unwrap(); prepare_preview(&mut doc).unwrap();
        let cached = doc.preview_source.clone().unwrap();
        prepare_preview(&mut doc).unwrap();
        assert!(Arc::ptr_eq(&cached,doc.preview_source.as_ref().unwrap()));
        set_exposure(&mut doc,0.5).unwrap();
        let mut uncached = doc.clone(); uncached.preview_source=None;
        assert_eq!(render_preview_at(&doc,Some(1440)).unwrap(),render_preview_at(&uncached,Some(1440)).unwrap());
        doc.evict_pixels();
        assert!(doc.preview_source.is_none()); assert!(doc.thumbnail_rgb.is_some());
        assert_eq!((doc.width(),doc.height()),(1800,12));
    }

    #[test]
    fn late_source_cache_does_not_restore_stale_parameters() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.png");
        RgbImage::from_pixel(3,2,image::Rgb([30,60,90])).save(&source).unwrap();
        let mut loaded = open_image(&source).unwrap(); prepare_preview(&mut loaded).unwrap();
        let mut current = loaded.clone(); current.evict_pixels(); set_exposure(&mut current,1.0).unwrap();
        current.reuse_source_cache(&loaded);
        assert_eq!(current.exposure_ev,1.0); assert_eq!(current.settings.exposure,1.0);
        assert!(Arc::ptr_eq(current.preview_source.as_ref().unwrap(),loaded.preview_source.as_ref().unwrap()));
        current.evict_pixels(); current.source_sha256="different".into(); current.reuse_source_cache(&loaded);
        assert_eq!(current.working_image().pixels.width(),0);
        let mut legacy = open_image_with_pipeline(&source, ColorPipeline::LegacySrgb).unwrap();
        legacy.evict_pixels();
        legacy.reuse_source_cache(&loaded);
        assert_eq!(legacy.working_image().pixels.width(),0);
    }

    #[test]
    fn evicted_document_refuses_changed_or_unpinned_legacy_render_cache() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.png");
        let cache = temp.path().join("cache.png");
        RgbImage::from_pixel(2, 2, image::Rgb([10, 30, 50])).save(&source).unwrap();
        std::fs::copy(&source, &cache).unwrap();
        let mut document = open_image(&source).unwrap();
        document.render_source_path = Some(cache.clone());
        document.render_source_sha256 = Some(sha256_file(&cache).unwrap());
        document.evict_pixels();
        assert_eq!((document.width(), document.height()), (2, 2));
        hydrate_document(&mut document).unwrap();
        document.evict_pixels();
        RgbImage::from_pixel(2, 2, image::Rgb([90, 70, 50])).save(&cache).unwrap();
        assert!(matches!(hydrate_document(&mut document), Err(CoreError::MissingOrChangedSource(_))));
        document.render_source_sha256 = None;
        assert!(matches!(hydrate_document(&mut document), Err(CoreError::MissingOrChangedSource(_))));
    }

    #[test]
    fn exposure_zero_is_identity_and_one_ev_matches_legacy_reference() {
        let pixels = [
            [0, 0, 0],
            [16, 32, 64],
            [32, 64, 128],
            [64, 128, 192],
            [96, 160, 224],
            [128, 192, 240],
            [192, 224, 255],
            [255, 255, 255],
        ];
        let rgb = RgbImage::from_fn(pixels.len() as u32, 1, |x, _| {
            image::Rgb(pixels[x as usize])
        });
        let mut doc = PhotoDocument::from_decoded(
            PathBuf::new(),
            None,
            String::new(),
            Arc::new(rgb),
            parse_capture_info(
                Path::new("."),
                ImageFormat::Png,
                (8, 1),
                (8, 1),
                image::ColorType::Rgb8,
                None,
            ),
            0.0,
        );
        let baseline = pixels.iter().flatten().copied().collect::<Vec<_>>();
        assert_eq!(
            render_preview(&doc).unwrap().as_raw().as_slice(),
            baseline.as_slice()
        );
        set_exposure(&mut doc, 1.0).unwrap();
        let got = render_preview(&doc).unwrap();
        let expected: [[u8; 3]; 8] = [
            [0, 0, 0],
            [26, 47, 90],
            [47, 90, 176],
            [90, 176, 255],
            [133, 218, 255],
            [176, 255, 255],
            [255, 255, 255],
            [255, 255, 255],
        ];
        for (actual, expected) in got.pixels().zip(expected) {
            for c in 0..3 {
                assert!(
                    actual[c].abs_diff(expected[c]) <= 1,
                    "got {:?}, expected {:?}",
                    actual,
                    expected
                );
            }
        }
    }

    #[test]
    fn rejects_out_of_range_and_off_step_exposure() {
        let mut doc = PhotoDocument::from_decoded(
            PathBuf::new(),
            None,
            String::new(),
            Arc::new(RgbImage::new(1, 1)),
            parse_capture_info(
                Path::new("."),
                ImageFormat::Png,
                (1, 1),
                (1, 1),
                image::ColorType::Rgb8,
                None,
            ),
            0.0,
        );
        assert!(set_exposure(&mut doc, 3.05).is_err());
        assert!(set_exposure(&mut doc, 0.03).is_err());
        assert!(set_exposure(&mut doc, f32::NAN).is_err());
    }
}
