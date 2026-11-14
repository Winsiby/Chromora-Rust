use std::fs;

use chromora::core::{self, CoreError, ExportColorSpace, ExportKind, ExportOptions, RenderCancellation};
use image::{Rgb, RgbImage};

fn export(source_color: [u8; 3], name: &str, kind: ExportKind, space: ExportColorSpace) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.png");
    RgbImage::from_fn(16, 16, |_, _| Rgb(source_color)).save(&source).unwrap();
    let doc = core::open_image(&source).unwrap();
    let path = temp.path().join(format!("{name}.{}", match kind { ExportKind::Jpeg => "jpg", ExportKind::Png => "png", _ => "tif" }));
    core::export_with_options(&doc, &path, kind,
        &ExportOptions { color_space: space, ..Default::default() }, &RenderCancellation::new()).unwrap();
    (temp, path)
}

fn pixels16(path: &std::path::Path) -> Vec<u16> {
    image::open(path).unwrap().to_rgb16().as_raw().clone()
}

#[test]
fn neutral_grays_stay_neutral_in_every_output_space() {
    let srgb = export([128, 128, 128], "srgb", ExportKind::Png, ExportColorSpace::Srgb);
    let baseline = pixels16(&srgb.1);
    for space in ExportColorSpace::ALL {
        let (temp, path) = export([128, 128, 128], "gray", ExportKind::Png, space);
        for pixel in pixels16(&path).chunks_exact(3) {
            assert!(pixel[0] == pixel[1] && pixel[1] == pixel[2], "{space:?} broke neutrality: {pixel:?}");
        }
        if space == ExportColorSpace::DisplayP3 {
            assert_eq!(pixels16(&path), baseline, "P3 shares the sRGB curve, so grays must not move");
        }
        drop(temp);
    }
}

#[test]
fn display_p3_remaps_saturated_primaries_and_embeds_an_icc_profile() {
    let srgb = export([255, 0, 0], "srgb", ExportKind::Png, ExportColorSpace::Srgb);
    let p3 = export([255, 0, 0], "p3", ExportKind::Png, ExportColorSpace::DisplayP3);
    let srgb_bytes = fs::read(&srgb.1).unwrap();
    let p3_bytes = fs::read(&p3.1).unwrap();
    assert!(srgb_bytes.windows(4).any(|w| w == b"iCCP"), "sRGB export should carry an ICC profile");
    assert!(p3_bytes.windows(4).any(|w| w == b"iCCP"), "P3 export should carry an ICC profile");
    assert_ne!(p3_bytes, srgb_bytes, "embedding a different ICC must change the file");
    let pixel = pixels16(&p3.1);
    assert!((59_000..61_000).contains(&pixel[0]), "P3 red R is about 0.917 encoded, got {}", pixel[0]);
    assert!((12_500..13_800).contains(&pixel[1]), "P3 red G is about 0.200 encoded, got {}", pixel[1]);
    assert!((8_500..9_700).contains(&pixel[2]), "P3 red B is about 0.139 encoded, got {}", pixel[2]);
}

#[test]
fn wide_gamut_export_works_for_jpeg_and_tiff_without_changing_pixel_geometry() {
    for kind in [ExportKind::Jpeg, ExportKind::Tiff] {
        let (temp, path) = export([255, 0, 0], "wide", kind, ExportColorSpace::AdobeRgb);
        assert_eq!(image::image_dimensions(&path).unwrap(), (16, 16));
        let bytes = fs::read(&path).unwrap();
        if kind == ExportKind::Jpeg {
            assert!(bytes.windows(11).any(|w| w == b"ICC_PROFILE"), "JPEG must embed an ICC APP2 marker");
        }
        drop(temp);
    }
}

#[test]
fn psd_keeps_the_strict_srgb_contract() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.png");
    RgbImage::from_fn(4, 4, |_, _| Rgb([200u8, 40, 10])).save(&source).unwrap();
    let doc = core::open_image(&source).unwrap();
    let path = temp.path().join("wide.psd");
    let error = core::export_with_options(&doc, &path, ExportKind::Psd,
        &ExportOptions { color_space: ExportColorSpace::DisplayP3, ..Default::default() }, &RenderCancellation::new()).unwrap_err();
    match error {
        CoreError::InvalidExport(message) => assert!(message.contains("sRGB"), "{message}"),
        other => panic!("expected InvalidExport, got {other:?}"),
    }
    assert!(!path.exists(), "a rejected export must not create the destination");
}
