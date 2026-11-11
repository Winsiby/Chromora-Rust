use std::{fs, path::PathBuf};

use chromora::core::{
    CaptureFileFormat, WhiteBalance, WhiteBalanceMode, WorkingColorSpace, WorkingSampleFormat,
    import_image, open_image, render_preview, set_exposure,
};
use image::ImageReader;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn legacy_exposure_png_references_match_within_one_lsb() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let source_path = fixtures.join("exposure_reference_input.png");
    let expected_0 = fixtures.join("exposure_reference_0ev.png");
    let expected_1 = fixtures.join("exposure_reference_plus_1ev.png");

    assert_eq!(
        sha256(&fs::read(&source_path).unwrap()),
        "4ff2e2913baeee8c884447d9157c3bdfcf9d3c27a33cce1e405ba95eb912bdef"
    );
    assert_eq!(
        sha256(&fs::read(&expected_0).unwrap()),
        "4ff2e2913baeee8c884447d9157c3bdfcf9d3c27a33cce1e405ba95eb912bdef"
    );
    assert_eq!(
        sha256(&fs::read(&expected_1).unwrap()),
        "552e1520219a587b229f1e4705342184742f4f827f879a8df94ad66af39076e2"
    );

    let mut doc = open_image(&source_path).unwrap();
    let exact_zero = ImageReader::open(expected_0)
        .unwrap()
        .decode()
        .unwrap()
        .to_rgb8();
    assert_eq!(render_preview(&doc).unwrap(), exact_zero);

    set_exposure(&mut doc, 1.0).unwrap();
    let expected = ImageReader::open(expected_1)
        .unwrap()
        .decode()
        .unwrap()
        .to_rgb8();
    let actual = render_preview(&doc).unwrap();
    assert_eq!(actual.dimensions(), (8, 1));
    for (a, e) in actual.pixels().zip(expected.pixels()) {
        for channel in 0..3 {
            assert!(
                a[channel].abs_diff(e[channel]) <= 1,
                "actual={a:?}, expected={e:?}"
            );
        }
    }
}

#[test]
fn capture_info_reads_only_standard_exif_facts_and_orientation() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/capture_info_reference.jpg");
    let doc = open_image(&fixture).unwrap();
    let info = doc.photo_info();
    assert_eq!(info.make.as_deref(), Some("Canon"));
    assert_eq!(info.model.as_deref(), Some("EOS Test 123"));
    assert_eq!(
        info.lens_model.as_deref(),
        Some("RF 35mm F1.8 MACRO IS STM")
    );
    assert_eq!(
        info.captured_at_exif.as_deref(),
        Some("2026:11:05 12:34:56")
    );
    assert_eq!(info.captured_at_offset_exif.as_deref(), Some("+08:00"));
    assert_eq!(info.orientation, Some(6));
    assert_eq!(info.source_dimensions, (3, 2));
    assert_eq!(info.oriented_dimensions, (2, 3));
    assert_eq!(doc.width(), 2);
    assert_eq!(doc.height(), 3);
    assert_eq!(info.focal_length_mm, Some(35.0));
    assert_eq!(info.equivalent_focal_length_35mm, Some(50.0));
    assert_eq!(info.aperture_f_number, Some(2.8));
    assert_eq!(info.exposure_time_seconds, Some(1.0 / 125.0));
    assert_eq!(info.exposure_compensation_ev, Some(-0.5));
    assert_eq!(info.iso, Some(320));
    assert_eq!(
        info.white_balance,
        Some(WhiteBalance {
            mode: Some(WhiteBalanceMode::Auto),
            temperature_kelvin: None,
            tint: None
        })
    );
    assert_eq!(info.vendor_style_name, None);
    assert_eq!(info.file_format, CaptureFileFormat::Jpeg);
    assert_eq!(
        info.file_size_bytes,
        Some(fs::metadata(&fixture).unwrap().len())
    );
    assert_eq!(info.icc_profile.embedded, false);
    assert_eq!(
        doc.working_image().metadata.color_space,
        WorkingColorSpace::SRgb
    );
    assert_eq!(
        doc.working_image().metadata.sample_format,
        WorkingSampleFormat::F32
    );
    assert_eq!(doc.working_image().metadata.has_alpha, false);
}

#[test]
fn import_keeps_source_copy_byte_identical() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let source = fixtures.join("exposure_reference_input.png");
    let temp = tempdir().unwrap();
    let doc = import_image(&source, &temp.path().join("originals")).unwrap();
    let copy = doc.original_copy_path.as_ref().unwrap();
    assert_eq!(fs::read(&source).unwrap(), fs::read(copy).unwrap());
    assert_eq!(sha256(&fs::read(copy).unwrap()), doc.source_sha256);
}

#[test]
fn rgba_png_is_composited_over_white_like_the_original_editor() {
    let temp = tempdir().unwrap();
    let opaque_path = temp.path().join("opaque.png");
    let transparent_path = temp.path().join("transparent.png");
    image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 80, 90, 100, 255])
        .unwrap()
        .save(&opaque_path)
        .unwrap();
    image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 0, 80, 90, 100, 128])
        .unwrap()
        .save(&transparent_path)
        .unwrap();

    let doc = open_image(&opaque_path).unwrap();
    assert!(doc.capture_info().source_has_alpha);
    assert!(!doc.working_image().metadata.has_alpha);
    assert_eq!(doc.original_rgb.get_pixel(1, 0).0, [80, 90, 100]);

    let transparent = open_image(&transparent_path).unwrap();
    assert_eq!(transparent.original_rgb.get_pixel(0, 0).0, [255, 255, 255]);
    for (value, expected) in transparent.original_rgb.get_pixel(1, 0).0.into_iter().zip([167u8,172,177]) {
        assert!(value.abs_diff(expected) <= 1);
    }
}

#[test]
fn missing_metadata_is_none_and_embedded_icc_is_reported() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let plain = open_image(&fixtures.join("exposure_reference_input.png")).unwrap();
    assert!(plain.photo_info().make.is_none());
    assert!(plain.photo_info().captured_at_exif.is_none());
    assert!(plain.photo_info().focal_length_mm.is_none());
    assert!(plain.photo_info().white_balance.is_none());
    assert!(!plain.photo_info().icc_profile.embedded);

    let profiled = open_image(&fixtures.join("icc_srgb_reference.jpg")).unwrap();
    assert!(profiled.photo_info().icc_profile.embedded);
    assert!(profiled.photo_info().icc_profile.bytes.unwrap() > 0);
    // Independent Pillow/libjpeg-turbo RGB decode, frozen before ICC conversion.
    let direct = ImageReader::open(fixtures.join("color07/icc-srgb-pillow-rgb.png"))
        .unwrap()
        .decode()
        .unwrap()
        .to_rgb8();
    let converted = render_preview(&profiled).unwrap();
    for (a, b) in converted.pixels().zip(direct.pixels()) {
        for channel in 0..3 {
            assert!(a[channel].abs_diff(b[channel]) <= 1);
        }
    }

    assert!(matches!(open_image(&fixtures.join("invalid_icc_reference.jpg")), Err(chromora::core::CoreError::InvalidIcc { .. })));
    let invalid = chromora::core::open_image_with_pipeline(&fixtures.join("invalid_icc_reference.jpg"), chromora::core::ColorPipeline::LegacySrgb).unwrap();
    assert!(invalid.photo_info().icc_profile.embedded);
    assert!(invalid.photo_info().icc_profile.description.is_none());
    let direct_invalid = ImageReader::open(fixtures.join("invalid_icc_reference.jpg"))
        .unwrap()
        .decode()
        .unwrap()
        .to_rgb8();
    assert_eq!(render_preview(&invalid).unwrap(), direct_invalid);
}
