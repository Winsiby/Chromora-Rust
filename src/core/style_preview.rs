//! Style-list icons for the open photo.
//! Chips stay in place until a photo is open and a thumbnail has been rendered.
//! Rendering calls the existing preview path on a small copy of that photo.

use std::path::Path;

use image::RgbImage;

use super::{
    AdjustmentSettings, CoreResult, PhotoDocument, load_preset, render_preview_at, set_lut,
    set_settings,
};

/// Long edge of a style-row thumbnail. The row frame stays 58×40; this only caps the bitmap.
pub const STYLE_PREVIEW_LONG_EDGE: u32 = 128;

/// Photo icons replace chips only while a photo is open and that row's bitmap is ready.
pub fn style_icon_shows_photo(open_photos: usize, preview_ready: bool) -> bool {
    open_photos > 0 && preview_ready
}

pub fn style_preview_key(kind: &str, path: &Path) -> String {
    format!("{kind}:{}", path.display())
}

/// Exposure, white balance, and crop describe the open photo.
/// Creative settings come from the style, not from edits already on the photo.
pub fn style_preview_settings(current: &AdjustmentSettings) -> AdjustmentSettings {
    AdjustmentSettings {
        exposure: current.exposure,
        temperature: current.temperature,
        tint: current.tint,
        geometry: current.geometry.clone(),
        ..AdjustmentSettings::default()
    }
}

pub fn render_preset_style_preview(doc: &PhotoDocument, preset: &Path) -> CoreResult<RgbImage> {
    let mut preview = doc.clone();
    let settings = load_preset(&style_preview_settings(&doc.settings), preset, false)?;
    set_settings(&mut preview, settings)?;
    set_lut(&mut preview, None)?;
    render_preview_at(&preview, Some(STYLE_PREVIEW_LONG_EDGE))
}

pub fn render_lut_style_preview(doc: &PhotoDocument, lut: &Path) -> CoreResult<RgbImage> {
    let mut preview = doc.clone();
    set_settings(&mut preview, style_preview_settings(&doc.settings))?;
    set_lut(&mut preview, Some(lut))?;
    render_preview_at(&preview, Some(STYLE_PREVIEW_LONG_EDGE))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::{ImageBuffer, RgbImage};

    use super::*;
    use crate::core::{open_image, thumbnail_document};

    fn recipe(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("recipes/chromora-film-styles-v1")
            .join(name)
    }

    fn save(path: &std::path::Path, image: &RgbImage) {
        image.save(path).unwrap();
    }

    #[test]
    fn icons_stay_chips_without_a_photo_and_switch_only_when_a_preview_exists() {
        assert!(!style_icon_shows_photo(0, false));
        assert!(!style_icon_shows_photo(0, true));
        assert!(!style_icon_shows_photo(2, false));
        assert!(style_icon_shows_photo(1, true));
        assert_ne!(
            style_preview_key("preset", Path::new("a.json")),
            style_preview_key("lut", Path::new("a.json"))
        );
    }

    #[test]
    fn preview_settings_keep_photo_corrections_and_drop_creative_edits() {
        let mut current = AdjustmentSettings::default();
        current.exposure = 1.0;
        current.temperature = 12.0;
        current.tint = -4.0;
        current.contrast = 40.0;
        current.saturation = -30.0;
        current.geometry.quarter_turns = 1;
        let preview = style_preview_settings(&current);
        assert_eq!(preview.exposure, 1.0);
        assert_eq!(preview.temperature, 12.0);
        assert_eq!(preview.tint, -4.0);
        assert_eq!(preview.geometry.quarter_turns, 1);
        assert_eq!(preview.contrast, 0.0);
        assert_eq!(preview.saturation, 0.0);
        assert_eq!(preview.lut_strength, 100.0);
    }

    #[test]
    fn style_previews_use_the_open_photo_and_the_existing_pipeline() {
        let temp = tempfile::tempdir().unwrap();
        let red_path = temp.path().join("red.png");
        let blue_path = temp.path().join("blue.png");
        save(
            &red_path,
            &ImageBuffer::from_pixel(48, 32, image::Rgb([180, 40, 30])),
        );
        save(
            &blue_path,
            &ImageBuffer::from_pixel(48, 32, image::Rgb([30, 50, 170])),
        );
        let red =
            thumbnail_document(&open_image(&red_path).unwrap(), STYLE_PREVIEW_LONG_EDGE).unwrap();
        let blue =
            thumbnail_document(&open_image(&blue_path).unwrap(), STYLE_PREVIEW_LONG_EDGE).unwrap();
        let chrome = recipe("01_富士风格_Classic-Chrome_纪实.json");
        let bw = recipe("08_理光风格_Hi-Contrast-BW_高反差黑白.json");
        let red_chrome = render_preset_style_preview(&red, &chrome).unwrap();
        let red_bw = render_preset_style_preview(&red, &bw).unwrap();
        let blue_chrome = render_preset_style_preview(&blue, &chrome).unwrap();
        assert_ne!(red_chrome.as_raw(), red_bw.as_raw());
        assert_ne!(red_chrome.as_raw(), blue_chrome.as_raw());
        assert!(red_chrome.width().max(red_chrome.height()) <= STYLE_PREVIEW_LONG_EDGE);
        assert!(chroma(&red_bw) < chroma(&red_chrome));
        assert!(
            chroma(&red_bw) < 12.0,
            "black-and-white preview chroma {}",
            chroma(&red_bw)
        );
    }

    fn chroma(image: &RgbImage) -> f32 {
        let mut total = 0.0_f32;
        for pixel in image.pixels() {
            let max = pixel[0].max(pixel[1]).max(pixel[2]) as f32;
            let min = pixel[0].min(pixel[1]).min(pixel[2]) as f32;
            total += max - min;
        }
        total / image.pixels().len().max(1) as f32
    }
}
