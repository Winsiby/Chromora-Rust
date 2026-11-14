use std::fs;

use chromora::core::{self, CoreError, ExportKind, ExportOptions, RenderCancellation};
use image::{Rgb, RgbImage};

#[test]
fn configured_exports_resize_after_geometry_and_never_enlarge() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.png");
    RgbImage::from_fn(12, 8, |x, y| Rgb([(x * 17) as u8, (y * 23) as u8, 80])).save(&source).unwrap();
    let mut doc = core::open_image(&source).unwrap();
    let mut settings = doc.settings.clone();
    settings.geometry.quarter_turns = 1;
    settings.geometry.crop = Some(core::NormalizedRect { x: 0.25, y: 0.25, width: 0.5, height: 0.5 });
    settings.exposure = 0.5;
    core::set_settings(&mut doc, settings).unwrap();
    let cancel = RenderCancellation::new();
    for (kind, extension) in [(ExportKind::Jpeg, "jpg"), (ExportKind::Png, "png"), (ExportKind::Tiff, "tif")] {
        let reduced = temp.path().join(format!("reduced.{extension}"));
        core::export_with_options(&doc, &reduced, kind,
            &ExportOptions { jpeg_quality: 80, max_long_edge: Some(3), ..Default::default() }, &cancel).unwrap();
        assert_eq!(image::image_dimensions(&reduced).unwrap(), (2, 3));
        let larger_limit = temp.path().join(format!("not-enlarged.{extension}"));
        core::export_with_options(&doc, &larger_limit, kind,
            &ExportOptions { jpeg_quality: 96, max_long_edge: Some(100), ..Default::default() }, &cancel).unwrap();
        assert_eq!(image::image_dimensions(&larger_limit).unwrap(), (4, 6));
    }
    assert_eq!((doc.width(), doc.height()), (12, 8));
    assert_eq!(doc.settings.exposure, 0.5);
}

#[test]
fn default_and_unneeded_resize_preserve_deep_samples_in_png_and_tiff() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("deep.tif");
    let samples = vec![1000u16, 1001, 1002, 30000, 30001, 30002];
    tiff::encoder::TiffEncoder::new(fs::File::create(&source).unwrap()).unwrap()
        .write_image::<tiff::encoder::colortype::RGB16>(2, 1, &samples).unwrap();
    let doc = core::open_image(&source).unwrap();
    let cancel = RenderCancellation::new();
    for (kind, ext) in [(ExportKind::Png, "png"), (ExportKind::Tiff, "tif")] {
        for (name, edge) in [("default", None), ("large-limit", Some(100))] {
            let path = temp.path().join(format!("{name}.{ext}"));
            core::export_with_options(&doc, &path, kind, &ExportOptions { max_long_edge: edge, ..Default::default() }, &cancel).unwrap();
            let decoded = image::open(&path).unwrap().to_rgb16();
            assert_eq!(decoded.dimensions(), (2, 1));
            assert_eq!(decoded.as_raw(), &samples);
        }
    }
}

#[test]
fn selected_jpeg_quality_changes_the_encoded_result() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("texture.png");
    RgbImage::from_fn(64, 64, |x, y| Rgb([
        ((x * 31 + y * 17) % 256) as u8,
        ((x * 13 + y * 47) % 256) as u8,
        ((x * 59 + y * 7) % 256) as u8,
    ])).save(&source).unwrap();
    let doc = core::open_image(&source).unwrap();
    let low = temp.path().join("quality-1.jpg");
    let high = temp.path().join("quality-100.jpg");
    for (path, quality) in [(&low, 1), (&high, 100)] {
        core::export_with_options(&doc, path, ExportKind::Jpeg,
            &ExportOptions { jpeg_quality: quality, ..Default::default() }, &RenderCancellation::new()).unwrap();
        assert_eq!(image::open(path).unwrap().to_rgb8().dimensions(), (64, 64));
    }
    assert_ne!(fs::read(&low).unwrap(), fs::read(&high).unwrap());
    assert!(fs::metadata(&high).unwrap().len() > fs::metadata(&low).unwrap().len());
}

#[test]
fn invalid_options_and_psd_resize_are_rejected_without_publishing() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.png");
    RgbImage::from_pixel(4, 2, Rgb([80, 100, 120])).save(&source).unwrap();
    let doc = core::open_image(&source).unwrap();
    for (index, options) in [
        ExportOptions { jpeg_quality: 0, ..Default::default() },
        ExportOptions { jpeg_quality: 101, ..Default::default() },
        ExportOptions { max_long_edge: Some(0), ..Default::default() },
        ExportOptions { max_long_edge: Some(30001), ..Default::default() },
    ].iter().enumerate() {
        let path = temp.path().join(format!("invalid-{index}.jpg"));
        assert!(core::export_with_options(&doc, &path, ExportKind::Jpeg, options, &RenderCancellation::new()).is_err());
        assert!(!path.exists());
    }
    let path = temp.path().join("reduced.psd");
    assert!(matches!(core::export_with_options(&doc, &path, ExportKind::Psd,
        &ExportOptions { max_long_edge: Some(2), ..Default::default() }, &RenderCancellation::new()), Err(CoreError::InvalidExport(_))));
    assert!(!path.exists());
}

#[test]
fn cancellation_and_protection_apply_to_every_configured_format() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.png");
    RgbImage::from_pixel(4, 2, Rgb([80, 100, 120])).save(&source).unwrap();
    let doc = core::import_image(&source, &temp.path().join("originals")).unwrap();
    let frozen = fs::read(&source).unwrap();
    for (index, kind) in [ExportKind::Jpeg, ExportKind::Png, ExportKind::Tiff, ExportKind::Psd].iter().enumerate() {
        let cancel = RenderCancellation::new();
        cancel.cancel();
        let path = temp.path().join(format!("cancel-{index}")).join("result.data");
        assert!(matches!(core::export_with_options(&doc, &path, *kind, &ExportOptions::default(), &cancel), Err(CoreError::Cancelled)));
        assert!(!path.parent().unwrap().exists());
        assert!(matches!(core::export_with_options(&doc, &source, *kind, &ExportOptions::default(), &RenderCancellation::new()), Err(CoreError::ProtectedOriginalDestination(_))));
        assert_eq!(fs::read(&source).unwrap(), frozen);
        let copy = doc.original_copy_path.as_ref().unwrap();
        assert!(matches!(core::export_with_options(&doc, copy, *kind, &ExportOptions::default(), &RenderCancellation::new()), Err(CoreError::ProtectedOriginalDestination(_))));
    }
}

#[test]
fn existing_tiff_and_psd_are_rejected_before_missing_source_is_decoded() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.png");
    RgbImage::from_pixel(4, 2, Rgb([80, 100, 120])).save(&source).unwrap();
    let mut doc = core::open_image(&source).unwrap();
    fs::remove_file(&source).unwrap();
    doc.evict_pixels();
    for kind in [ExportKind::Tiff, ExportKind::Psd] {
        let path = temp.path().join(format!("existing-{kind:?}.data"));
        fs::write(&path, b"preserve").unwrap();
        assert!(matches!(core::export_with_options(&doc, &path, kind, &ExportOptions::default(), &RenderCancellation::new()),
            Err(CoreError::Io(ref error)) if error.kind() == std::io::ErrorKind::AlreadyExists));
        assert_eq!(fs::read(&path).unwrap(), b"preserve");
    }
}

#[test]
fn batch_destination_protects_another_photos_missing_source_with_valid_managed_copy() {
    let temp = tempfile::tempdir().unwrap();
    let source_a = temp.path().join("a.png");
    RgbImage::from_pixel(4, 2, Rgb([60, 80, 100])).save(&source_a).unwrap();
    let output_directory = temp.path().join("batch");
    fs::create_dir(&output_directory).unwrap();
    let source_b = output_directory.join("0001_a_编辑.png");
    RgbImage::from_pixel(4, 2, Rgb([100, 120, 140])).save(&source_b).unwrap();
    let originals = temp.path().join("originals");
    let a = core::import_image(&source_a, &originals).unwrap();
    let b = core::import_image(&source_b, &originals).unwrap();
    let managed_copy = b.original_copy_path.clone().unwrap();
    let frozen = fs::read(&managed_copy).unwrap();
    fs::remove_file(&source_b).unwrap();

    let documents = [a, b];
    assert!(matches!(core::protect_export_destinations(&documents, &source_b, &RenderCancellation::new()),
        Err(CoreError::ProtectedOriginalDestination(_))));
    assert!(!source_b.exists());
    assert_eq!(fs::read(&managed_copy).unwrap(), frozen);

    let cancel = RenderCancellation::new();
    cancel.cancel();
    assert!(matches!(core::protect_export_destinations(&documents, &output_directory.join("safe.png"), &cancel), Err(CoreError::Cancelled)));
    assert!(!output_directory.join("safe.png").exists());
}
