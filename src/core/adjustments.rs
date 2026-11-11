//! CPU implementation of the original Chromora 0.3 sRGB adjustment order.
use image::Rgb32FImage;

use super::{lut::Lut3d, settings::AdjustmentSettings, ColorPipeline, CoreResult, RenderCancellation};

const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
fn luma(p: &[f32]) -> f32 { p[0] * LUMA[0] + p[1] * LUMA[1] + p[2] * LUMA[2] }
fn clamp_image(pixels: &mut [f32]) { for x in pixels { *x = x.clamp(0.0, 1.0); } }
fn smoothstep(x: f32) -> f32 { x * x * (3.0 - 2.0 * x) }

#[cfg(test)]
pub(crate) fn apply(image: &mut Rgb32FImage, settings: &AdjustmentSettings, lut: Option<&Lut3d>) {
    apply_with_pipeline(image, settings, lut, ColorPipeline::LegacySrgb);
}

#[cfg(test)]
fn apply_with_pipeline(image: &mut Rgb32FImage, settings: &AdjustmentSettings, lut: Option<&Lut3d>, pipeline: ColorPipeline) {
    apply_with_pipeline_cancellable(image, settings, lut, pipeline, &RenderCancellation::new()).unwrap();
}

pub(crate) fn apply_with_pipeline_cancellable(image: &mut Rgb32FImage, settings: &AdjustmentSettings,
    lut: Option<&Lut3d>, pipeline: ColorPipeline, cancel: &RenderCancellation) -> CoreResult<()> {
    cancel.check()?;
    let high_precision = pipeline == ColorPipeline::FloatSrgb;
    let (width, height) = image.dimensions();
    let (w, h) = (width as usize, height as usize);
    let pixels = image.as_mut();
    base(pixels, w, h, settings.exposure, settings.temperature, settings.shadows,
        settings.highlights, settings.contrast, settings.saturation, settings.clarity, high_precision, cancel)?;
    cancel.check()?;
    detail(pixels, w, h, settings, cancel)?;
    cancel.check()?;
    retouch(pixels, w, h, settings, high_precision, cancel)?;
    cancel.check()?;
    color(pixels, settings, cancel)?;
    cancel.check()?;
    if let Some(lut) = lut.filter(|_| settings.lut_strength != 0.0) {
        lut.apply(pixels, settings.lut_strength / 100.0);
    }
    if settings.post_exposure != 0.0 || settings.post_temperature != 0.0 || settings.post_contrast != 0.0 {
        base(pixels, w, h, settings.post_exposure, settings.post_temperature,
            0.0, 0.0, settings.post_contrast, 0.0, 0.0, high_precision, cancel)?;
    }
    cancel.check()?;
    apply_masks(pixels,w,h,&settings.masks,high_precision,cancel)?;
    cancel.check()
}

fn apply_masks(pixels:&mut [f32],width:usize,height:usize,masks:&[super::masks::LocalMask],high_precision:bool,cancel:&RenderCancellation) -> CoreResult<()> {
    if width==0 || height==0 {return Ok(());}
    for mask in masks.iter().filter(|mask|mask.enabled&&!mask.adjustments.is_neutral()) {
        let a=mask.adjustments;
        for (index,pixel) in pixels.chunks_exact_mut(3).enumerate() {
            if index % 16384 == 0 { cancel.check()?; }
            let point=[((index%width) as f32+0.5)/width as f32,((index/width) as f32+0.5)/height as f32];
            let weight=mask.coverage_at_with_size(point,width as u32,height as u32);
            if weight<=0.0 {continue;}
            let mut adjusted=[pixel[0],pixel[1],pixel[2]];
            base(&mut adjusted,1,1,a.exposure,a.temperature,a.shadows,a.highlights,0.0,a.saturation,0.0,high_precision,cancel)?;
            for channel in 0..3 {pixel[channel]+=(adjusted[channel]-pixel[channel])*weight;}
        }
    }
    Ok(())
}

#[cfg(test)]
mod local_mask_tests {
    use super::*;
    use crate::core::masks::{LocalAdjustments,LocalMask,MaskShape};
    #[test]
    fn radial_exposure_only_changes_covered_pixels_and_inversion_reverses_it() {
        let original=Rgb32FImage::from_pixel(5,5,image::Rgb([0.2,0.2,0.2]));
        let mut settings=AdjustmentSettings::default();
        settings.masks.push(LocalMask{adjustments:LocalAdjustments{exposure:1.0,..Default::default()},
            shape:MaskShape::Radial{center:[0.5,0.5],radius:[0.25,0.25],feather:0.4},..Default::default()});
        let mut image=original.clone();apply(&mut image,&settings,None);
        assert_eq!(image.get_pixel(0,0),original.get_pixel(0,0));
        let expected=1.055*((0.2_f32+0.055)/1.055).powf(2.4).mul_add(2.0,0.0).powf(1.0/2.4)-0.055;
        assert!((image.get_pixel(2,2)[0]-expected).abs()<0.000001);
        settings.masks[0].invert=true;
        let mut inverse=original.clone();apply(&mut inverse,&settings,None);
        assert_eq!(inverse.get_pixel(2,2),original.get_pixel(2,2));
        assert!((inverse.get_pixel(0,0)[0]-expected).abs()<0.000001);
    }
    #[test]
    fn neutral_or_disabled_masks_preserve_float_samples() {
        let mut image=Rgb32FImage::from_pixel(3,2,image::Rgb([0.123456,0.234567,0.345678]));
        let before=image.clone();let mut settings=AdjustmentSettings::default();
        settings.masks.push(LocalMask::default());
        settings.masks.push(LocalMask{enabled:false,adjustments:LocalAdjustments{exposure:3.0,..Default::default()},..Default::default()});
        apply(&mut image,&settings,None);assert_eq!(image,before);
    }
}

#[allow(clippy::too_many_arguments)]
fn base(pixels: &mut [f32], w: usize, h: usize, exposure: f32, temperature: f32,
    shadows: f32, highlights: f32, contrast: f32, saturation: f32, clarity: f32, high_precision: bool, cancel: &RenderCancellation) -> CoreResult<()> {
    if exposure != 0.0 || temperature != 0.0 {
        let t = temperature / 100.0;
        let gains = [2.0_f32.powf(exposure + t * 0.35), 2.0_f32.powf(exposure), 2.0_f32.powf(exposure - t * 0.45)];
        for (index, p) in pixels.chunks_exact_mut(3).enumerate() {
            if index % 16384 == 0 { cancel.check()?; }
            for c in 0..3 {
                let linear = if p[c] <= 0.04045 { p[c] / 12.92 } else { ((p[c] + 0.055) / 1.055).powf(2.4) } * gains[c];
                p[c] = if linear <= 0.0031308 { 12.92 * linear } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
            }
        }
    }
    if shadows != 0.0 || highlights != 0.0 || contrast != 0.0 || saturation != 0.0 {
        for (index, p) in pixels.chunks_exact_mut(3).enumerate() {
            if index % 16384 == 0 { cancel.check()?; }
            let y = luma(p).clamp(0.0, 1.0);
            let shift = shadows / 100.0 * 0.42 * y * (1.0 - y).powi(2)
                + highlights / 100.0 * 0.42 * y.powi(2) * (1.0 - y);
            for c in p.iter_mut() { *c += shift; *c += contrast / 100.0 * 0.52 * (*c - 0.5); }
            if saturation != 0.0 {
                let y = luma(p);
                for c in p { *c += saturation / 100.0 * (*c - y); }
            }
        }
    }
    if clarity != 0.0 {
        let y = pixels.chunks_exact(3).map(|p| luma(p).clamp(0.0, 1.0)).collect::<Vec<_>>();
        cancel.check()?;
        let sigma = 1.5_f32.max(w.min(h) as f32 / 160.0);
        let blurred = if high_precision { float_blur(&y, w, h, sigma, cancel)? }
            else { legacy_blur(&y, w, h, sigma, cancel)? };
        for (p, (y, b)) in pixels.chunks_exact_mut(3).zip(y.into_iter().zip(blurred)) {
            let change = (y - b) * clarity / 100.0 * 0.85;
            for c in p { *c += change; }
        }
    }
    clamp_image(pixels);
    Ok(())
}

// Match the original Pillow GaussianBlur's three fractional box passes, including
// its 8-bit intermediate rounding. Formula: Gwosdek et al., SSVM 2011; also used
// by Pillow src/libImaging/BoxBlur.c (Pillow/HPND license).
fn legacy_blur(plane: &[f32], w: usize, h: usize, sigma: f32, cancel: &RenderCancellation) -> CoreResult<Vec<f32>> {
    cancel.check()?;
    let mut data = plane.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8).collect::<Vec<_>>();
    if w == 0 || h == 0 { return Ok(Vec::new()); }
    let sigma2 = sigma * sigma / 3.0;
    let length = ((12.0_f64 * sigma2 as f64 + 1.0).sqrt()) as f32;
    let integer = ((length - 1.0) / 2.0).floor();
    let fraction = (2.0 * integer + 1.0) * (integer * (integer + 1.0) - 3.0 * sigma2)
        / (6.0 * (sigma2 - (integer + 1.0).powi(2)));
    let radius_f = integer + fraction;
    let radius = radius_f as usize;
    let whole = ((1u32 << 24) as f32 / (radius_f * 2.0 + 1.0)) as u64;
    let far = ((1u64 << 24) - (radius as u64 * 2 + 1) * whole) / 2;
    let mut line = vec![0u8; w.max(h)];
    let mut output = vec![0u8; w.max(h)];
    for vertical in [false, true] {
        let (length, lines) = if vertical { (h, w) } else { (w, h) };
        for _ in 0..3 {
            for row in 0..lines {
                cancel.check()?;
                for x in 0..length { line[x] = data[if vertical { x * w + row } else { row * w + x }]; }
                let at = |x: isize| -> u64 { line[x.clamp(0, length as isize - 1) as usize] as u64 };
                let mut sum = (-(radius as isize)..=radius as isize).map(at).sum::<u64>();
                for x in 0..length {
                    let bulk = sum * whole + (at(x as isize - radius as isize - 1) + at(x as isize + radius as isize + 1)) * far;
                    output[x] = ((bulk + (1 << 23)) >> 24) as u8;
                    sum = sum + at(x as isize + radius as isize + 1) - at(x as isize - radius as isize);
                }
                for x in 0..length { data[if vertical { x * w + row } else { row * w + x }] = output[x]; }
            }
        }
    }
    Ok(data.into_iter().map(|v| v as f32 / 255.0).collect())
}

// Keep the same fractional-box shape without RGB8 intermediate quantization.
// Accumulating each line in f64 prevents a constant field from drifting.
fn float_blur(plane: &[f32], w: usize, h: usize, sigma: f32, cancel: &RenderCancellation) -> CoreResult<Vec<f32>> {
    cancel.check()?;
    if w == 0 || h == 0 { return Ok(Vec::new()); }
    let sigma2 = f64::from(sigma).powi(2) / 3.0;
    let integer = (((12.0 * sigma2 + 1.0).sqrt() - 1.0) / 2.0).floor();
    let fraction = (2.0 * integer + 1.0) * (integer * (integer + 1.0) - 3.0 * sigma2)
        / (6.0 * (sigma2 - (integer + 1.0).powi(2)));
    let radius_f = integer + fraction;
    let radius = radius_f as usize;
    let whole = 1.0 / (radius_f * 2.0 + 1.0);
    let far = (1.0 - (radius * 2 + 1) as f64 * whole) / 2.0;
    let mut data = plane.to_vec();
    let mut line = vec![0.0f64; w.max(h)];
    let mut output = vec![0.0f32; w.max(h)];
    for vertical in [false, true] {
        let (length, lines) = if vertical { (h, w) } else { (w, h) };
        for _ in 0..3 {
            for row in 0..lines {
                cancel.check()?;
                for x in 0..length { line[x] = f64::from(data[if vertical { x * w + row } else { row * w + x }]); }
                let at = |x: isize| line[x.clamp(0, length as isize - 1) as usize];
                let mut sum = (-(radius as isize)..=radius as isize).map(at).sum::<f64>();
                for x in 0..length {
                    let value = sum * whole + (at(x as isize - radius as isize - 1) + at(x as isize + radius as isize + 1)) * far;
                    output[x] = value.clamp(0.0, 1.0) as f32;
                    sum = sum + at(x as isize + radius as isize + 1) - at(x as isize - radius as isize);
                }
                for x in 0..length { data[if vertical { x * w + row } else { row * w + x }] = output[x]; }
            }
        }
    }
    Ok(data)
}

fn shift_luma(p: &mut [f32], y: f32, shift: f32) {
    let positive = shift >= 0.0;
    let room = if positive { 1.0 - y } else { y };
    let mix = (shift.abs() / room.max(1e-6)).min(1.0);
    for c in p { *c += mix * (if positive { 1.0 } else { 0.0 } - *c); }
}

fn quantile(values: &mut [f32], fraction: f32) -> f32 {
    values.sort_unstable_by(f32::total_cmp);
    let index = (values.len() - 1) as f32 * fraction;
    let low = index.floor() as usize;
    let high = index.ceil() as usize;
    values[low] + (values[high] - values[low]) * (index - low as f32)
}

fn detail(pixels: &mut [f32], w: usize, h: usize, s: &AdjustmentSettings, cancel: &RenderCancellation) -> CoreResult<()> {
    let [tint, whites, blacks, vibrance, dehaze] = [s.tint, s.whites, s.blacks, s.vibrance, s.dehaze].map(|v| v / 100.0);
    let (mut airlight, mut haze_shift) = (0.0, 0.0);
    if dehaze != 0.0 {
        let step = w.max(h).div_ceil(256).max(1);
        let mut ys = Vec::new();
        let mut mins = Vec::new();
        for y in (0..h).step_by(step) { for x in (0..w).step_by(step) {
            let p = &pixels[(y * w + x) * 3..][..3];
            ys.push(luma(p));
            mins.push(p[0].min(p[1]).min(p[2]));
        }}
        airlight = 0.85_f32.max(quantile(&mut ys, 0.96));
        let veil = (quantile(&mut mins, 0.12) * 0.9 + 0.04).clamp(0.05, 0.36);
        haze_shift = if dehaze > 0.0 { let attenuation = veil * dehaze; -attenuation / (1.0 - attenuation) } else { -0.20 * dehaze };
    }
    if [tint, whites, blacks, vibrance, dehaze].iter().any(|v| *v != 0.0) {
        for (index, p) in pixels.chunks_exact_mut(3).enumerate() {
            if index % 16384 == 0 { cancel.check()?; }
            let mut y = luma(p);
            if tint != 0.0 {
                let weight = tint * 0.58 * y * (1.0 - y);
                p[0] += weight; p[1] -= weight * (LUMA[0] + LUMA[2]) / LUMA[1]; p[2] += weight;
                for c in p.iter_mut() { *c = c.clamp(0.0, 1.0); }
                y = luma(p);
            }
            if whites != 0.0 || blacks != 0.0 || dehaze != 0.0 {
                let mut shift = 0.0;
                if whites != 0.0 { shift += whites * ((y - 0.46) / 0.54).max(0.0).powi(2) * if whites > 0.0 { (1.0 - y) * 0.8 } else { 0.23 }; }
                if blacks != 0.0 { shift += blacks * ((0.54 - y) / 0.54).max(0.0).powi(2) * if blacks > 0.0 { 0.23 } else { y * 0.8 }; }
                if dehaze != 0.0 { shift += haze_shift * (airlight - y).max(0.0); }
                shift_luma(p, y, shift);
                y = luma(p);
            }
            if vibrance != 0.0 {
                let high = p[0].max(p[1]).max(p[2]);
                let low = p[0].min(p[1]).min(p[2]);
                let sat = (high - low) / high.max(1e-6);
                let weighting = if vibrance > 0.0 { (1.0 - sat).powf(1.5) } else { 0.5 + 0.5 * sat };
                let mut change = vibrance * 0.86 * weighting;
                if vibrance > 0.0 {
                    let room_up = (1.0 - y) / (high - y).max(1e-6);
                    let room_down = y / (y - low).max(1e-6);
                    change = change.min((room_up.min(room_down) - 1.0).max(0.0));
                }
                for c in p { *c += change * (*c - y); }
            }
        }
    }
    if s.sharpening != 0.0 || s.denoise != 0.0 { spatial_detail(pixels, w, h, s.sharpening / 100.0, s.denoise / 100.0, cancel)?; }
    if [tint, whites, blacks, vibrance, dehaze, s.sharpening, s.denoise].iter().any(|v| *v != 0.0) { clamp_image(pixels); }
    Ok(())
}

fn spatial_detail(pixels: &mut [f32], w: usize, h: usize, sharpening: f32, denoise: f32, cancel: &RenderCancellation) -> CoreResult<()> {
    let luminance = pixels.chunks_exact(3).map(luma).collect::<Vec<_>>();
    let sigma = (w.max(h) as f32 / 1800.0).clamp(0.65, 3.4);
    let radius = (2.4 * sigma).ceil() as usize;
    let mut weights = (0..=radius * 2).map(|i| (-0.5 * ((i as f32 - radius as f32) / sigma).powi(2)).exp()).collect::<Vec<_>>();
    let total: f32 = weights.iter().sum();
    for v in &mut weights { *v /= total; }
    for start in (0..h).step_by(192) {
        let stop = (start + 192).min(h);
        let rows = stop - start + 2 * radius;
        let mut horizontal = vec![0.0; rows * w];
        for row in 0..rows {
            cancel.check()?;
            let source_y = (start as isize + row as isize - radius as isize).clamp(0, h as isize - 1) as usize;
            for x in 0..w {
                for (offset, &weight) in weights.iter().enumerate() {
                    let source_x = (x as isize + offset as isize - radius as isize).clamp(0, w as isize - 1) as usize;
                    horizontal[row * w + x] += luminance[source_y * w + source_x] * weight;
                }
            }
        }
        for y in start..stop { cancel.check()?; for x in 0..w {
            let mut smoothed = 0.0;
            for (offset, &weight) in weights.iter().enumerate() { smoothed += horizontal[((y - start) + offset) * w + x] * weight; }
            let original = luminance[y * w + x];
            let detail = original - smoothed;
            let magnitude = detail.abs();
            let suppression = denoise * 0.91 * (1.0 - ((magnitude - 0.025) / 0.075).clamp(0.0, 1.0)).powi(2);
            let emphasis = sharpening * ((magnitude - 0.003) / 0.03).clamp(0.0, 1.0) * (1.0 - suppression);
            let sharpened = (detail * emphasis).clamp(-0.075 * sharpening, 0.075 * sharpening);
            shift_luma(&mut pixels[(y * w + x) * 3..][..3], original, sharpened - detail * suppression);
        }}
    }
    Ok(())
}

fn retouch(pixels: &mut [f32], w: usize, h: usize, s: &AdjustmentSettings, high_precision: bool, cancel: &RenderCancellation) -> CoreResult<()> {
    let Some(region) = s.skin_region else { return Ok(()); };
    if [s.blemishes, s.smoothing, s.skin_tone, s.texture].iter().all(|v| *v == 0.0) { return Ok(()); }
    let left = (region.x * w as f32).floor() as usize;
    let top = (region.y * h as f32).floor() as usize;
    let right = (((region.x + region.width) * w as f32).ceil() as usize).min(w);
    let bottom = (((region.y + region.height) * h as f32).ceil() as usize).min(h);
    if right.saturating_sub(left) < 4 || bottom.saturating_sub(top) < 4 { return Ok(()); }
    let (pw, ph) = (right - left, bottom - top);
    let radius = 1.0_f32.max(pw.min(ph) as f32 / 65.0);
    let softened = (0..3).map(|c| {
        let plane = (top..bottom).flat_map(|y| (left..right).map(move |x| (y * w + x) * 3 + c)).map(|i| pixels[i]).collect::<Vec<_>>();
        cancel.check()?;
        if high_precision { float_blur(&plane, pw, ph, radius, cancel) }
            else { legacy_blur(&plane, pw, ph, radius, cancel) }
    }).collect::<CoreResult<Vec<_>>>()?;
    for y in 0..ph { cancel.check()?; for x in 0..pw {
        let xx = -1.0 + 2.0 * x as f32 / (pw - 1) as f32;
        let yy = -1.0 + 2.0 * y as f32 / (ph - 1) as f32;
        let mask = smoothstep(((1.0 - yy * yy - xx * xx) * 3.0).clamp(0.0, 1.0));
        let p = &mut pixels[((y + top) * w + x + left) * 3..][..3];
        let d = [p[0] - softened[0][y * pw + x], p[1] - softened[1][y * pw + x], p[2] - softened[2][y * pw + x]];
        let yd = luma(&d);
        let fine = (1.0 - yd.abs() * 9.0).clamp(0.0, 1.0);
        let marks = ((d[0] - d[1] - 0.015) * 12.0).clamp(0.0, 1.0);
        for c in 0..3 {
            let change = -d[c] * (s.smoothing / 100.0 * 0.75) * fine
                - (d[c] - yd) * (s.skin_tone / 100.0) * 0.82
                + yd * (s.texture / 100.0) * 0.5 * fine
                - d[c] * marks * (s.blemishes / 100.0) * 0.95;
            p[c] = (p[c] + change * mask).clamp(0.0, 1.0);
        }
    }}
    Ok(())
}

fn interpolate(x: f32, points: &[[f32; 2]]) -> f32 {
    if x <= points[0][0] { return points[0][1]; }
    for pair in points.windows(2) {
        if x <= pair[1][0] { let t = (x - pair[0][0]) / (pair[1][0] - pair[0][0]); return pair[0][1] + (pair[1][1] - pair[0][1]) * t; }
    }
    points.last().unwrap()[1]
}

fn hsv(hue: f32) -> [f32; 3] {
    let h = hue.rem_euclid(1.0) * 6.0;
    [0.0, 4.0, 2.0].map(|offset| (((h + offset).rem_euclid(6.0) - 3.0).abs() - 1.0).clamp(0.0, 1.0))
}

fn color(pixels: &mut [f32], s: &AdjustmentSettings, cancel: &RenderCancellation) -> CoreResult<()> {
    let hue = [s.red_hue, s.orange_hue, s.yellow_hue, s.green_hue, s.aqua_hue, s.blue_hue, s.purple_hue, s.magenta_hue, s.red_hue].map(|v| v / 100.0);
    let sat = [s.red_sat, s.orange_sat, s.yellow_sat, s.green_sat, s.aqua_sat, s.blue_sat, s.purple_sat, s.magenta_sat, s.red_sat].map(|v| v / 100.0);
    let lum = [s.red_lum, s.orange_lum, s.yellow_lum, s.green_lum, s.aqua_lum, s.blue_lum, s.purple_lum, s.magenta_lum, s.red_lum].map(|v| v / 100.0);
    let hsl_active = hue.iter().chain(sat.iter()).chain(lum.iter()).any(|v| *v != 0.0);
    let grading_active = s.shadow_sat != 0.0 || s.midtone_sat != 0.0 || s.highlight_sat != 0.0;
    if s.curves.is_empty() && !hsl_active && !grading_active { return Ok(()); }
    let wheels = [(s.shadow_hue, s.shadow_sat), (s.midtone_hue, s.midtone_sat), (s.highlight_hue, s.highlight_sat)].map(|(h, a)| {
        let color = hsv(h / 360.0);
        let y = luma(&color);
        color.map(|v| (v - y) * (0.24 * a / 100.0))
    });
    let channels = ["rgb", "red", "green", "blue"].map(|name| s.curves.get(name));
    for (index, p) in pixels.chunks_exact_mut(3).enumerate() {
            if index % 16384 == 0 { cancel.check()?; }
        for (channel, points) in channels.iter().enumerate() {
            if let Some(points) = points {
                if channel == 0 { for c in p.iter_mut() { *c = interpolate(*c, points); } }
                else { p[channel - 1] = interpolate(p[channel - 1], points); }
            }
        }
        if hsl_active { apply_hsl(p, &hue, &sat, &lum); }
        if grading_active {
            let y = luma(p).clamp(0.0, 1.0);
            let amount = 4.0 * y * (1.0 - y);
            let tone = (y + 0.22 * s.color_balance / 100.0 * amount).clamp(0.0, 1.0);
            let shadows = smoothstep((1.0 - 2.0 * tone).max(0.0));
            let highlights = smoothstep((2.0 * tone - 1.0).max(0.0));
            let midtones = 1.0 - shadows - highlights;
            for c in 0..3 { p[c] += amount * (shadows * wheels[0][c] + midtones * wheels[1][c] + highlights * wheels[2][c]); }
        }
        for c in p { *c = c.clamp(0.0, 1.0); }
    }
    Ok(())
}

fn apply_hsl(p: &mut [f32], hues: &[f32; 9], sats: &[f32; 9], lums: &[f32; 9]) {
    let low = p[0].min(p[1]).min(p[2]);
    let high = p[0].max(p[1]).max(p[2]);
    let chroma = high - low;
    if chroma <= 1e-6 { return; }
    let lead = if p[0] >= p[1] && p[0] >= p[2] { 0 } else if p[1] >= p[2] { 1 } else { 2 };
    let hue = (match lead { 0 => (p[1] - p[2]) / chroma, 1 => 2.0 + (p[2] - p[0]) / chroma, _ => 4.0 + (p[0] - p[1]) / chroma } * 60.0).rem_euclid(360.0);
    let centers = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 270.0, 300.0, 360.0];
    let index = centers.windows(2).position(|v| hue <= v[1]).unwrap_or(7);
    let t = (hue - centers[index]) / (centers[index + 1] - centers[index]);
    let [dh, ds, dl] = [hues, sats, lums].map(|v| v[index] + (v[index + 1] - v[index]) * t);
    if dh == 0.0 && ds == 0.0 && dl == 0.0 { return; }
    let mut lightness = (high + low) * 0.5;
    let mut saturation = (chroma / (1.0 - (2.0 * lightness - 1.0).abs()).max(1e-7)).clamp(0.0, 1.0);
    saturation += ds * if ds > 0.0 { 1.0 - saturation } else { saturation };
    lightness += 0.5 * dl * if dl > 0.0 { 1.0 - lightness } else { lightness };
    let hue = (hue + 30.0 * dh).rem_euclid(360.0) / 60.0;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    for (c, offset) in [0.0, 4.0, 2.0].iter().enumerate() {
        let unit = (((hue + offset).rem_euclid(6.0) - 3.0).abs() - 1.0).clamp(0.0, 1.0);
        p[c] = unit * chroma + lightness - 0.5 * chroma;
    }
}

#[cfg(test)]
mod float_blur_tests {
    use super::*;

    #[test]
    fn floating_blur_preserves_constant_fields_at_small_and_large_radii() {
        for (w,h) in [(1,1),(2,3),(16,16)] {
            for value in [0.0,1000.0/65535.0,0.33333334,1.0] {
                for sigma in [1.0,1.5,3.4,30.0] {
                    let input=vec![value;w*h];
                    let output=float_blur(&input,w,h,sigma,&RenderCancellation::new()).unwrap();
                    assert!(output.iter().all(|v|(*v-value).abs()<=f32::EPSILON), "{w}x{h}, value={value}, sigma={sigma}");
                }
            }
        }
    }

    #[test]
    fn floating_blur_keeps_sub_rgb8_gradient_steps_and_handles_edges() {
        let (w,h)=(512,3);
        let input=(0..w*h).map(|i|(1000+(i%w)) as f32/65535.0).collect::<Vec<_>>();
        let output=float_blur(&input,w,h,1.5,&RenderCancellation::new()).unwrap();
        assert!(output.iter().all(|v|v.is_finite()&&(0.0..=1.0).contains(v)));
        assert!(output[..w].windows(2).all(|v|v[0]<v[1]));
        let unique=output[..w].iter().map(|v|v.to_bits()).collect::<std::collections::HashSet<_>>();
        assert!(unique.len()>450);
        for x in 32..w-32 {assert!((output[x]-input[x]).abs()*65535.0<1.0);}
    }

    #[test]
    fn floating_portrait_processing_respects_the_selected_region() {
        let source=Rgb32FImage::from_fn(32,24,|x,y|image::Rgb([
            (1000+x*7+y*3) as f32/65535.0,
            (1100+x*3+y*11) as f32/65535.0,
            (1200+x*13+y*5) as f32/65535.0,
        ]));
        let mut image=source.clone();
        let mut settings=AdjustmentSettings::default();
        settings.skin_region=Some(super::super::SkinRegion{x:0.25,y:0.25,width:0.5,height:0.5});
        settings.smoothing=100.;settings.skin_tone=80.;settings.texture=50.;settings.blemishes=100.;
        apply_with_pipeline(&mut image,&settings,None,ColorPipeline::FloatSrgb);
        for y in 0..24 {for x in 0..32 {
            assert!(image.get_pixel(x,y).0.iter().all(|v|v.is_finite()&&(0.0..=1.0).contains(v)));
            if !(8..24).contains(&x)||!(6..18).contains(&y) {assert_eq!(image.get_pixel(x,y),source.get_pixel(x,y));}
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(value: &str) -> Vec<u8> {
        value.as_bytes().chunks_exact(2).map(|v| u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16).unwrap()).collect()
    }

    #[test]
    fn original_python_reference_covers_each_group_and_combined_order() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../tests/fixtures/legacy-adjustment-reference.json")).unwrap();
        let bytes = unhex(fixture["input_rgb8_hex"].as_str().unwrap());
        let w = fixture["width"].as_u64().unwrap() as u32;
        let h = fixture["height"].as_u64().unwrap() as u32;
        let path = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(path.path(), fixture["cube"].as_str().unwrap()).unwrap();
        let lut = super::super::lut::load_cube(path.path()).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let mut image = Rgb32FImage::from_raw(w, h, bytes.iter().map(|v| *v as f32 / 255.0).collect()).unwrap();
            let settings: AdjustmentSettings = serde_json::from_value(case["settings"].clone()).unwrap();
            settings.validate().unwrap();
            apply(&mut image, &settings, case["use_lut"].as_bool().unwrap().then_some(&lut));
            let expected = unhex(case["expected_rgb8_hex"].as_str().unwrap());
            let errors = image.as_raw().iter().zip(expected).map(|(actual, expected)| {
                ((*actual * 255.0).round_ties_even() as u8).abs_diff(expected)
            }).collect::<Vec<_>>();
            let maximum = errors.iter().copied().max().unwrap();
            let mean = errors.iter().map(|v| *v as f64).sum::<f64>() / errors.len() as f64;
            assert!(maximum <= 1 && mean < 0.05,
                "{} differs from independent Python reference: max={maximum}, mean={mean}", case["name"]);
        }
    }

    #[test]
    fn neutral_processing_keeps_precision_above_eight_bits() {
        let input = vec![1000.0 / 65535.0, 1001.0 / 65535.0, 1002.0 / 65535.0];
        let mut image = Rgb32FImage::from_raw(1, 1, input.clone()).unwrap();
        apply(&mut image, &AdjustmentSettings::default(), None);
        assert_eq!(image.as_raw(), &input);
        assert_ne!(image.as_raw()[0], image.as_raw()[1]);
    }
}
