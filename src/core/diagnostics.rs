use image::RgbImage;

/// Statistics of the displayed RGB8 preview, after edits and geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewDiagnostics {
    pub width: u32,
    pub height: u32,
    pub histograms: [[u64; 256]; 3],
    pub shadow_pixels: u64,
    pub highlight_pixels: u64,
}

impl PreviewDiagnostics {
    pub fn from_rgb8(image: &RgbImage) -> Self {
        let mut result = Self { width:image.width(), height:image.height(), histograms:[[0;256];3], shadow_pixels:0, highlight_pixels:0 };
        for pixel in image.pixels() {
            for channel in 0..3 { result.histograms[channel][pixel[channel] as usize] += 1; }
            result.shadow_pixels += u64::from(pixel.0.contains(&0));
            result.highlight_pixels += u64::from(pixel.0.contains(&255));
        }
        result
    }
    pub fn pixel_count(&self) -> u64 { u64::from(self.width) * u64::from(self.height) }
    pub fn shadow_percent(&self) -> f64 { self.percent(self.shadow_pixels) }
    pub fn highlight_percent(&self) -> f64 { self.percent(self.highlight_pixels) }
    fn percent(&self, count:u64) -> f64 {
        if self.pixel_count() == 0 { 0.0 } else { count as f64 * 100.0 / self.pixel_count() as f64 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn histogram_counts_channels_and_boundary_pixels_once() {
        let pixels = RgbImage::from_raw(4,1,vec![0,0,0,255,255,255,0,128,255,23,45,67]).unwrap();
        let stats = PreviewDiagnostics::from_rgb8(&pixels);
        assert_eq!(stats.pixel_count(),4);
        assert_eq!(stats.shadow_pixels,2);
        assert_eq!(stats.highlight_pixels,2);
        assert_eq!(stats.shadow_percent(),50.0);
        assert_eq!(stats.histograms[0][0],2);
        assert_eq!(stats.histograms[1][128],1);
        assert_eq!(stats.histograms[2][255],2);
        for histogram in stats.histograms { assert_eq!(histogram.iter().sum::<u64>(),4); }
    }
    #[test]
    fn empty_preview_has_finite_zero_percentages() {
        let stats=PreviewDiagnostics::from_rgb8(&RgbImage::new(0,0));
        assert_eq!(stats.shadow_percent(),0.0);
        assert_eq!(stats.highlight_percent(),0.0);
    }
}
