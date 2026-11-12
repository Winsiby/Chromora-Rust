//! Pure UI interaction helpers: filter swatches, wheel zoom, and filmstrip stepping.
//! Swatches illustrate a style for the list. They are not the export color pipeline.

use std::path::Path;

use chromora::core::{load_cube, load_preset, AdjustmentSettings, Lut3d};

use crate::app::ZoomMode;

/// Existing product wording: these packs are approximate, not official filters.
pub const APPROXIMATE_STYLE_NOTE: &str =
    "01–08为八款近似风格，00为撤去本包风格；它们不代表原厂官方滤镜或逐像素复刻。";

pub const PARAMETER_STYLE_LABEL: &str = "参数风格";
pub const LUT_STYLE_LABEL: &str = "LUT";

const MIN_ZOOM_PERCENT: f32 = 10.;
const MAX_ZOOM_PERCENT: f32 = 800.;
/// One typical Windows wheel notch is three lines (`SPI_GETWHEELSCROLLLINES`).
const LINES_PER_NOTCH: f32 = 3.;
const ZOOM_PER_NOTCH: f32 = 1.1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeSpace {
    pub rail: f32,
    pub inspector: f32,
    pub filmstrip: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ZoomFrame {
    pub old_frame: (f32, f32),
    pub new_frame: (f32, f32),
    pub viewport: (f32, f32),
    pub old_offset: (f32, f32),
}

pub fn step_photo_index(index: usize, len: usize, delta: i32) -> Option<usize> {
    if len == 0 || delta == 0 {
        return None;
    }
    let next = index as i64 + i64::from(delta);
    if next < 0 || next >= len as i64 {
        None
    } else {
        Some(next as usize)
    }
}

pub fn effective_zoom_percent(image_width: u32, zoom: &ZoomMode, dpi: f32, fitted_logical_width: f32) -> f32 {
    match zoom {
        ZoomMode::Actual => 100.,
        ZoomMode::Percent(percent) => *percent,
        ZoomMode::Fit => {
            if image_width == 0 {
                100.
            } else {
                fitted_logical_width / image_width as f32 * dpi.max(1.) * 100.
            }
        }
    }
}

/// Positive `line_delta` zooms in (Windows wheel away from the user).
pub fn wheel_zoom_percent(current: f32, line_delta: f32) -> f32 {
    let current = if current.is_finite() && current > 0. { current } else { 100. };
    if !line_delta.is_finite() || line_delta == 0. {
        return snap_zoom(current).clamp(MIN_ZOOM_PERCENT, MAX_ZOOM_PERCENT);
    }
    let notches = line_delta / LINES_PER_NOTCH;
    snap_zoom(current * ZOOM_PER_NOTCH.powf(notches)).clamp(MIN_ZOOM_PERCENT, MAX_ZOOM_PERCENT)
}

fn snap_zoom(percent: f32) -> f32 {
    for mark in [10., 25., 50., 100., 200., 400., 800.] {
        let threshold = (mark * 0.035_f32).max(0.6_f32);
        if (percent - mark).abs() <= threshold {
            return mark;
        }
    }
    (percent * 2.).round() / 2.
}

pub fn zoom_button_flags(zoom: &ZoomMode) -> (bool, bool, bool) {
    match zoom {
        ZoomMode::Fit => (true, false, false),
        ZoomMode::Actual => (false, true, false),
        ZoomMode::Percent(percent) if (*percent - 100.).abs() < 0.51 => (false, true, false),
        ZoomMode::Percent(percent) if (*percent - 200.).abs() < 0.51 => (false, false, true),
        ZoomMode::Percent(_) => (false, false, false),
    }
}

pub fn zoom_readout(zoom: &ZoomMode) -> Option<String> {
    match zoom {
        ZoomMode::Percent(percent) if (*percent - 100.).abs() >= 0.51 && (*percent - 200.).abs() >= 0.51 => {
            Some(format!("{percent:.0}%"))
        }
        _ => None,
    }
}

pub fn zoom_frame_size(photo: (f32, f32), viewport: (f32, f32)) -> (f32, f32) {
    (photo.0.max(viewport.0).max(1.), photo.1.max(viewport.1).max(1.))
}

/// Keeps the viewport center on the same point of the zoom surface.
/// Offset is negative once the surface is scrolled down or right.
pub fn zoom_scroll_offset(frame: ZoomFrame) -> (f32, f32) {
    let (vw, vh) = frame.viewport;
    if vw < 1. || vh < 1. || !frame.old_frame.0.is_finite() || !frame.new_frame.0.is_finite() {
        return frame.old_offset;
    }
    let old_fits = frame.old_frame.0 <= vw + 0.5 && frame.old_frame.1 <= vh + 0.5;
    let new_fits = frame.new_frame.0 <= vw + 0.5 && frame.new_frame.1 <= vh + 0.5;
    if new_fits {
        return (0., 0.);
    }
    if old_fits {
        return ((vw - frame.new_frame.0) / 2., (vh - frame.new_frame.1) / 2.);
    }
    let (ox, oy) = frame.old_offset;
    let cx = -ox + vw / 2.;
    let cy = -oy + vh / 2.;
    let sx = frame.new_frame.0 / frame.old_frame.0.max(1.);
    let sy = frame.new_frame.1 / frame.old_frame.1.max(1.);
    (-(cx * sx - vw / 2.), -(cy * sy - vh / 2.))
}

const CHART: [[f32; 3]; 8] = [
    [0.16, 0.16, 0.16],
    [0.42, 0.42, 0.42],
    [0.84, 0.84, 0.84],
    [0.86, 0.62, 0.48],
    [0.40, 0.62, 0.84],
    [0.30, 0.55, 0.28],
    [0.78, 0.46, 0.22],
    [0.20, 0.28, 0.48],
];

pub fn preset_swatch(path: &Path) -> Vec<[u8; 3]> {
    match load_preset(&AdjustmentSettings::default(), path, false) {
        Ok(settings) => render_chart(&settings, None),
        Err(_) => render_chart(&AdjustmentSettings::default(), None),
    }
}

pub fn lut_swatch(path: &Path) -> Vec<[u8; 3]> {
    match load_cube(path) {
        Ok(lut) => render_chart(&AdjustmentSettings::default(), Some(&lut)),
        Err(_) => render_chart(&AdjustmentSettings::default(), None),
    }
}

fn render_chart(settings: &AdjustmentSettings, lut: Option<&Lut3d>) -> Vec<[u8; 3]> {
    CHART
        .into_iter()
        .map(|pixel| {
            let mut rgb = if lut.is_some() { pixel } else { apply_parameter_preview(pixel, settings) };
            if let Some(lut) = lut {
                rgb = sample_lut(lut, rgb, 1.);
            }
            rgb.map(|channel| (channel.clamp(0., 1.) * 255.).round() as u8)
        })
        .collect()
}

fn apply_parameter_preview(mut rgb: [f32; 3], settings: &AdjustmentSettings) -> [f32; 3] {
    let contrast = 1. + settings.contrast / 100. * 0.85;
    for channel in &mut rgb {
        *channel = ((*channel - 0.5) * contrast + 0.5).clamp(0., 1.);
    }
    let temperature = (settings.temperature + settings.post_temperature) / 100.;
    rgb[0] = (rgb[0] + temperature * 0.12).clamp(0., 1.);
    rgb[2] = (rgb[2] - temperature * 0.12).clamp(0., 1.);
    let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    let shadow_lift = settings.shadows / 100. * (1. - luma) * 0.18;
    let highlight_lift = settings.highlights / 100. * luma * 0.12;
    for channel in &mut rgb {
        *channel = (*channel + shadow_lift + highlight_lift).clamp(0., 1.);
    }
    let (hue, mut sat, mut value) = rgb_to_hsv(rgb);
    sat = (sat * (1. + settings.saturation / 100.).clamp(0., 2.2)).clamp(0., 1.);
    if let Some((hue_shift, sat_delta, lum_delta)) = band_delta(hue, settings) {
        sat = (sat * (1. + sat_delta / 140.)).clamp(0., 1.);
        value = (value * (1. + lum_delta / 220.)).clamp(0., 1.);
        rgb = hsv_to_rgb(hue + hue_shift * 0.4, sat, value);
    } else {
        rgb = hsv_to_rgb(hue, sat, value);
    }
    if let Some(curve) = settings.curves.get("rgb") {
        for channel in &mut rgb {
            *channel = eval_curve(curve, *channel);
        }
    }
    let y = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    tint(&mut rgb, settings.shadow_hue, settings.shadow_sat, (1. - y).powf(1.35));
    tint(&mut rgb, settings.highlight_hue, settings.highlight_sat, y.powf(1.1));
    rgb.map(|channel| channel.clamp(0., 1.))
}

fn band_delta(hue: f32, settings: &AdjustmentSettings) -> Option<(f32, f32, f32)> {
    let bands = [
        (0., settings.red_hue, settings.red_sat, settings.red_lum),
        (30., settings.orange_hue, settings.orange_sat, settings.orange_lum),
        (60., settings.yellow_hue, settings.yellow_sat, settings.yellow_lum),
        (120., settings.green_hue, settings.green_sat, settings.green_lum),
        (180., settings.aqua_hue, settings.aqua_sat, settings.aqua_lum),
        (240., settings.blue_hue, settings.blue_sat, settings.blue_lum),
        (280., settings.purple_hue, settings.purple_sat, settings.purple_lum),
        (320., settings.magenta_hue, settings.magenta_sat, settings.magenta_lum),
    ];
    let mut best: Option<(f32, f32, f32, f32)> = None;
    for (center, hue_shift, sat_delta, lum_delta) in bands {
        let distance = hue_distance(hue, center);
        if distance > 26. {
            continue;
        }
        if best.is_none_or(|current| distance < current.0) {
            best = Some((distance, hue_shift, sat_delta, lum_delta));
        }
    }
    best.map(|(_, hue_shift, sat_delta, lum_delta)| (hue_shift, sat_delta, lum_delta))
}

fn hue_distance(a: f32, b: f32) -> f32 {
    let delta = (a - b).rem_euclid(360.);
    delta.min(360. - delta)
}

fn tint(rgb: &mut [f32; 3], hue: f32, saturation: f32, weight: f32) {
    if saturation <= 0. || weight <= 0. {
        return;
    }
    let color = hsv_to_rgb(hue, 1., 1.);
    let amount = (saturation / 100. * 0.45 * weight).clamp(0., 0.45);
    for index in 0..3 {
        rgb[index] += (color[index] - rgb[index]) * amount;
    }
}

fn eval_curve(points: &[[f32; 2]], x: f32) -> f32 {
    if points.len() < 2 {
        return x.clamp(0., 1.);
    }
    let x = x.clamp(0., 1.);
    if x <= points[0][0] {
        return points[0][1].clamp(0., 1.);
    }
    for pair in points.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        if x <= right[0] {
            let span = (right[0] - left[0]).max(1e-6);
            let t = (x - left[0]) / span;
            return (left[1] + (right[1] - left[1]) * t).clamp(0., 1.);
        }
    }
    points[points.len() - 1][1].clamp(0., 1.)
}

fn rgb_to_hsv(rgb: [f32; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta <= f32::EPSILON {
        0.
    } else if max == r {
        60. * (((g - b) / delta).rem_euclid(6.))
    } else if max == g {
        60. * ((b - r) / delta + 2.)
    } else {
        60. * ((r - g) / delta + 4.)
    };
    let saturation = if max <= f32::EPSILON { 0. } else { delta / max };
    (hue.rem_euclid(360.), saturation, max)
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let hue = hue.rem_euclid(360.);
    let chroma = value * saturation.clamp(0., 1.);
    let x = chroma * (1. - ((hue / 60.).rem_euclid(2.) - 1.).abs());
    let m = value - chroma;
    let (r, g, b) = match (hue / 60.) as i32 {
        0 => (chroma, x, 0.),
        1 => (x, chroma, 0.),
        2 => (0., chroma, x),
        3 => (0., x, chroma),
        4 => (x, 0., chroma),
        _ => (chroma, 0., x),
    };
    [r + m, g + m, b + m]
}

pub fn sample_lut(lut: &Lut3d, mut pixel: [f32; 3], strength: f32) -> [f32; 3] {
    let size = lut.size.max(2);
    let mut lo = [0usize; 3];
    let mut hi = [0usize; 3];
    let mut frac = [0.0; 3];
    for channel in 0..3 {
        let span = lut.domain_max[channel] - lut.domain_min[channel];
        let p = if span.abs() < 1e-12 {
            0.
        } else {
            ((pixel[channel] - lut.domain_min[channel]) / span).clamp(0., 1.) * (size - 1) as f32
        };
        lo[channel] = (p as usize).min(size - 1);
        hi[channel] = (lo[channel] + 1).min(size - 1);
        frac[channel] = p - lo[channel] as f32;
    }
    let mut output = [0.0; 3];
    for corner in 0..8 {
        let mut xyz = [0; 3];
        let mut weight = 1.0;
        for channel in 0..3 {
            let upper = corner & (1 << channel) != 0;
            xyz[channel] = if upper { hi[channel] } else { lo[channel] };
            weight *= if upper { frac[channel] } else { 1.0 - frac[channel] };
        }
        let index = (xyz[2] * size + xyz[1]) * size + xyz[0];
        let sample = lut.values.get(index).copied().unwrap_or(pixel);
        for channel in 0..3 {
            output[channel] += sample[channel] * weight;
        }
    }
    for channel in 0..3 {
        pixel[channel] = (pixel[channel] + (output[channel] - pixel[channel]) * strength).clamp(0., 1.);
    }
    pixel
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn recipe(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("recipes/chromora-film-styles-v1")
            .join(name)
    }

    #[test]
    fn arrow_step_stays_inside_the_open_set() {
        assert_eq!(step_photo_index(0, 3, -1), None);
        assert_eq!(step_photo_index(0, 3, 1), Some(1));
        assert_eq!(step_photo_index(2, 3, 1), None);
        assert_eq!(step_photo_index(1, 0, 1), None);
    }

    #[test]
    fn wheel_zoom_uses_notch_sized_steps_and_known_stops() {
        let zoomed_in = wheel_zoom_percent(100., 3.);
        assert!((zoomed_in - 110.).abs() < 0.2, "{zoomed_in}");
        let zoomed_out = wheel_zoom_percent(100., -3.);
        assert!(zoomed_out < 100. && zoomed_out > 85., "{zoomed_out}");
        assert_eq!(wheel_zoom_percent(98., 1.), 100.);
        assert_eq!(wheel_zoom_percent(10., -30.), 10.);
        assert_eq!(wheel_zoom_percent(800., 30.), 800.);
        assert_eq!(zoom_button_flags(&ZoomMode::Percent(200.)), (false, false, true));
        assert_eq!(zoom_button_flags(&ZoomMode::Percent(150.)), (false, false, false));
        assert_eq!(zoom_readout(&ZoomMode::Percent(150.)).as_deref(), Some("150%"));
        assert!(zoom_readout(&ZoomMode::Fit).is_none());
    }

    #[test]
    fn zoom_keeps_the_viewport_center_stable() {
        let centered = zoom_scroll_offset(ZoomFrame {
            old_frame: (800., 600.),
            new_frame: (1000., 750.),
            viewport: (800., 600.),
            old_offset: (0., 0.),
        });
        assert!((centered.0 - (-100.)).abs() < 0.1);
        assert!((centered.1 - (-75.)).abs() < 0.1);
        let continued = zoom_scroll_offset(ZoomFrame {
            old_frame: (1000., 750.),
            new_frame: (1200., 900.),
            viewport: (800., 600.),
            old_offset: centered,
        });
        assert!((continued.0 - (-200.)).abs() < 0.1, "{continued:?}");
        assert!((continued.1 - (-150.)).abs() < 0.1, "{continued:?}");
    }

    #[test]
    fn parameter_swatches_keep_names_distinct_and_black_and_white_neutral() {
        let bw = preset_swatch(&recipe("08_理光风格_Hi-Contrast-BW_高反差黑白.json"));
        let chrome = preset_swatch(&recipe("01_富士风格_Classic-Chrome_纪实.json"));
        let velvia = preset_swatch(&recipe("05_富士风格_Velvia_鲜艳风光.json"));
        assert_eq!(bw.len(), 8);
        assert!(bw.iter().all(|pixel| {
            (pixel[0] as i16 - pixel[1] as i16).abs() <= 2 && (pixel[1] as i16 - pixel[2] as i16).abs() <= 2
        }));
        assert_ne!(chrome, velvia);
        assert_ne!(chrome, bw);
        let neutral = render_chart(&AdjustmentSettings::default(), None);
        assert!(neutral[3][0] > neutral[3][2]);
    }

    #[test]
    fn lut_sampling_matches_the_cube_strength_vector() {
        let lut = Lut3d {
            size: 2,
            domain_min: [0.; 3],
            domain_max: [1.; 3],
            values: vec![
                [0., 0., 0.],
                [0., 1., 0.],
                [1., 0., 0.],
                [1., 1., 0.],
                [0., 0., 1.],
                [0., 1., 1.],
                [1., 0., 1.],
                [1., 1., 1.],
            ],
            source_path: PathBuf::new(),
            sha256: String::new(),
        };
        let sampled = sample_lut(&lut, [0.2, 0.7, 0.4], 0.5);
        assert!((sampled[0] - 0.45).abs() < 1e-5);
        assert!((sampled[1] - 0.45).abs() < 1e-5);
        assert!((sampled[2] - 0.4).abs() < 1e-5);
    }
}
