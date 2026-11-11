use std::path::Path;

use image::RgbImage;
use super::{CoreError, CoreResult};

#[cfg(windows)]
unsafe extern "C" {
    fn chromora_jpeg_decode(input: *const u8, length: usize, pixels: *mut *mut u8,
        width: *mut u32, height: *mut u32, message: *mut std::ffi::c_char, message_size: usize) -> i32;
    fn chromora_jpeg_free(pixels: *mut u8);
}

pub(super) fn decode(path: &Path) -> CoreResult<RgbImage> {
    #[cfg(windows)]
    {
        let input = std::fs::read(path)?;
        let mut pixels = std::ptr::null_mut();
        let (mut width, mut height) = (0, 0);
        let mut message = [0 as std::ffi::c_char; 256];
        let result = unsafe { chromora_jpeg_decode(input.as_ptr(), input.len(), &mut pixels,
            &mut width, &mut height, message.as_mut_ptr(), message.len()) };
        if result != 0 {
            let reason = unsafe { std::ffi::CStr::from_ptr(message.as_ptr()) }.to_string_lossy();
            return Err(CoreError::InvalidJpeg(format!("JPEG 精确解码失败：{reason}")));
        }
        // The bridge owns its allocation; release it with the same CRT that allocated it.
        struct Pixels(*mut u8);
        impl Drop for Pixels { fn drop(&mut self) { unsafe { chromora_jpeg_free(self.0) }; } }
        let owned = Pixels(pixels);
        let length = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(3))
            .filter(|n| *n > 0 && *n <= 512 * 1024 * 1024)
            .ok_or_else(|| CoreError::InvalidJpeg("JPEG 解码器返回了无效尺寸".into()))?;
        if owned.0.is_null() { return Err(CoreError::InvalidJpeg("JPEG 解码器未返回像素".into())); }
        let data = unsafe { std::slice::from_raw_parts(owned.0, length) }.to_vec();
        RgbImage::from_raw(width, height, data).ok_or_else(|| CoreError::InvalidJpeg("JPEG 像素尺寸不一致".into()))
    }
    #[cfg(not(windows))]
    { let _ = path; Err(CoreError::InvalidJpeg("此精确 JPEG 解码版本仅支持 Windows".into())) }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncation_instead_of_accepting_repaired_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("truncated.jpg");
        let mut encoded = Vec::new();
        jpeg_encoder::Encoder::new(&mut encoded, 96)
            .encode(&[30, 60, 90].repeat(17 * 13), 17, 13, jpeg_encoder::ColorType::Rgb).unwrap();
        std::fs::write(&path, &encoded).unwrap();
        assert_eq!(decode(&path).unwrap().dimensions(), (17, 13));
        encoded.truncate(encoded.len() - 2);
        std::fs::write(&path, &encoded).unwrap();
        assert!(matches!(decode(&path), Err(CoreError::InvalidJpeg(_))));
    }
}
