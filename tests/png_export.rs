use std::{fs, io::BufReader};

use chromora::core::{self, GeometrySettings, LocalAdjustments, LocalMask, MaskShape, NormalizedRect};
use image::{ImageDecoder, ImageReader, Rgb, RgbImage};

#[test]
fn png_is_full_resolution_rgb16_with_srgb_icc_and_exact_deep_samples() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("高位深原片.tif");
    let samples: Vec<u16> = (0..1601 * 3).map(|index| (1000 + index * 13 % 60000) as u16).collect();
    tiff::encoder::TiffEncoder::new(fs::File::create(&source).unwrap()).unwrap()
        .write_image::<tiff::encoder::colortype::RGB16>(1601, 1, &samples).unwrap();
    let doc = core::open_image(&source).unwrap();
    let output = temp.path().join("中文 无损成片.png");
    core::export_png(&doc, &output).unwrap();

    let bytes = fs::read(&output).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&bytes[12..16], b"IHDR");
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 1601);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 1);
    assert_eq!((bytes[24], bytes[25]), (16, 2)); // 16-bit truecolour, without alpha.
    let mut decoder = ImageReader::open(&output).unwrap().into_decoder().unwrap();
    assert_eq!(decoder.color_type(), image::ColorType::Rgb16);
    let profile = decoder.icc_profile().unwrap().expect("PNG needs an embedded profile");
    assert_eq!(&profile[16..20], b"RGB ");
    assert_eq!(&profile[36..40], b"acsp");
    let parsed = moxcms::ColorProfile::new_from_slice(&profile).unwrap();
    match parsed.description {
        Some(moxcms::ProfileText::Localizable(names)) => assert!(names.iter().any(|name| name.value == "sRGB IEC61966-2.1")),
        _ => panic!("PNG profile must identify sRGB"),
    }
    let decoded = image::open(&output).unwrap().into_rgb16();
    assert_eq!(decoded.dimensions(), (1601, 1));
    assert_eq!(decoded.as_raw(), &samples);
    assert!(decoded.as_raw().iter().any(|value| value % 257 != 0));
}

#[test]
fn png_applies_crop_rotation_and_local_mask_and_matches_rgb16_tiff_after_eviction() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("风景.png");
    RgbImage::from_fn(16, 12, |x, y| Rgb([(40 + x * 4) as u8, (50 + y * 4) as u8, 90]))
        .save(&source).unwrap();
    let mut doc = core::open_image(&source).unwrap();
    let mut settings = doc.settings.clone();
    settings.geometry = GeometrySettings {
        quarter_turns: 1,
        crop: Some(NormalizedRect { x: 0.25, y: 0.25, width: 0.5, height: 0.5 }),
        straighten_degrees: 0.0,
    };
    core::set_settings(&mut doc, settings.clone()).unwrap();
    let unmasked = temp.path().join("unmasked.png");
    core::export_png(&doc, &unmasked).unwrap();
    settings.masks = vec![LocalMask {
        id: "sky".into(), name: "天空".into(), enabled: true, invert: false,
        shape: MaskShape::Linear { start: [0.0, 0.2], end: [0.0, 0.8] },
        adjustments: LocalAdjustments { exposure: 0.5, ..Default::default() },
    }];
    core::set_settings(&mut doc, settings).unwrap();
    let output = temp.path().join("edited.png");
    let tiff = temp.path().join("edited.tif");
    core::export_png(&doc, &output).unwrap();
    core::export_tiff(&doc, &tiff).unwrap();
    let edited = image::open(&output).unwrap().into_rgb16();
    assert_eq!(edited.dimensions(), (6, 8));
    assert_eq!((doc.width(), doc.height()), (16, 12));
    let before = image::open(&unmasked).unwrap().into_rgb16();
    assert_ne!(edited, before);
    assert!(edited.pixels().zip(before.pixels())
        .any(|(actual, before)| actual[0] > before[0]));
    let mut tiff_decoder = tiff::decoder::Decoder::new(BufReader::new(fs::File::open(tiff).unwrap())).unwrap();
    match tiff_decoder.read_image().unwrap() {
        tiff::decoder::DecodingResult::U16(samples) => assert_eq!(edited.as_raw(), &samples),
        _ => panic!("TIFF reference must retain RGB16"),
    }
    doc.evict_pixels();
    let evicted = temp.path().join("evicted.png");
    core::export_png(&doc, &evicted).unwrap();
    assert_eq!(image::open(evicted).unwrap().into_rgb16(), edited);
}

#[test]
fn png_rejects_existing_destinations_originals_and_managed_copies_without_changes() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("original.png");
    RgbImage::from_pixel(3, 2, Rgb([80, 100, 120])).save(&source).unwrap();
    let doc = core::import_image(&source, &temp.path().join("originals")).unwrap();
    let output = temp.path().join("edited.png");
    core::export_png(&doc, &output).unwrap();
    for path in [&source, doc.original_copy_path.as_ref().unwrap(), &output] {
        let before = fs::read(path).unwrap();
        assert!(core::export_png(&doc, path).is_err());
        assert_eq!(fs::read(path).unwrap(), before);
    }
    fs::remove_file(&source).unwrap();
    assert!(core::export_png(&doc, &source).is_err());
    assert!(!source.exists());
}
