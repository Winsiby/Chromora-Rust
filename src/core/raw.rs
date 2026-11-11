use std::path::{Path, PathBuf};

use image::{ColorType, ImageFormat, Rgb32FImage};

use super::{CaptureFileFormat, CoreError, CoreResult};
use super::io::DecodedImage;
use super::metadata::parse_capture_info;

pub fn is_raw_path(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|extension| {
        matches!(extension.to_ascii_lowercase().as_str(),
            "3fr" | "arw" | "cr2" | "cr3" | "crw" | "dcr" | "dng" | "erf" |
            "fff" | "iiq" | "k25" | "kdc" | "mef" | "mos" | "mrw" | "nef" |
            "nrw" | "orf" | "pef" | "raf" | "raw" | "rw2" | "rwl" | "sr2" | "srf" | "srw" | "x3f")
    })
}

#[repr(C)]
struct RawMetadata {
    make: [std::ffi::c_char; 64], model: [std::ffi::c_char; 64],
    lens: [std::ffi::c_char; 128], captured_at: [std::ffi::c_char; 20],
    iso: f64, shutter: f64, aperture: f64, focal_length: f64, equivalent_focal_length: f64,
    orientation: u32,
}
impl Default for RawMetadata {
    fn default() -> Self { Self { make:[0;64],model:[0;64],lens:[0;128],captured_at:[0;20],
        iso:0.0,shutter:0.0,aperture:0.0,focal_length:0.0,equivalent_focal_length:0.0,orientation:0 } }
}
fn metadata_text(bytes:&[std::ffi::c_char])->Option<String> {
    let bytes:Vec<u8>=bytes.iter().take_while(|&&v|v!=0).map(|&v|v as u8).collect();
    let text=String::from_utf8_lossy(&bytes).trim().to_owned();
    if text.is_empty(){None}else{Some(text)}
}
fn positive(value:f64)->Option<f64>{(value.is_finite()&&value>0.0).then_some(value)}
fn fill_raw_metadata(info:&mut super::PhotoInfo,metadata:&RawMetadata) {
    if info.make.is_none(){info.make=metadata_text(&metadata.make);}
    if info.model.is_none(){info.model=metadata_text(&metadata.model);}
    if info.lens_model.is_none(){info.lens_model=metadata_text(&metadata.lens);}
    if info.iso.is_none(){info.iso=positive(metadata.iso).filter(|v|*v>=1.0&&*v<=u32::MAX as f64).map(|v|v.round()as u32);}
    if info.exposure_time_seconds.is_none(){info.exposure_time_seconds=positive(metadata.shutter);}
    if info.aperture_f_number.is_none(){info.aperture_f_number=positive(metadata.aperture);}
    if info.focal_length_mm.is_none(){info.focal_length_mm=positive(metadata.focal_length);}
    if info.equivalent_focal_length_35mm.is_none(){info.equivalent_focal_length_35mm=positive(metadata.equivalent_focal_length);}
    if info.captured_at_exif.is_none(){
        info.captured_at_exif=metadata_text(&metadata.captured_at).filter(|text|{
            text.len()==19&&text.bytes().enumerate().all(|(i,v)|match i{4|7|13|16=>v==b':',10=>v==b' ',_=>v.is_ascii_digit()})
        });
    }
    if info.orientation.is_none(){info.orientation=u8::try_from(metadata.orientation).ok().filter(|v|(1..=8).contains(v));}
}

fn raw_error(message: impl Into<String>) -> CoreError {
    CoreError::Raw(message.into())
}

pub fn libraw_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(directory) = std::env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf)) {
        candidates.push(directory.join("libraw.dll"));
        candidates.push(directory.join("native/libraw/bin/libraw.dll"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("native/libraw/bin/libraw.dll"));
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(windows)]
pub(crate) fn decode_raw(path: &Path) -> CoreResult<DecodedImage> {
    use std::{ffi::CStr, os::windows::ffi::OsStrExt};
    unsafe extern "C" {
        fn chromora_decode_raw(dll: *const u16, source: *const u16, pixels: *mut *mut u16,
            width: *mut u32, height: *mut u32, metadata: *mut RawMetadata,
            error: *mut std::ffi::c_char, error_capacity: usize) -> i32;
        fn chromora_free_raw(pixels: *mut u16);
    }
    let dll = libraw_path().ok_or_else(|| raw_error("缺少随附的 LibRaw 0.22.3，请恢复 libraw.dll。"))?;
    let dll: Vec<u16> = dll.canonicalize()?.as_os_str().encode_wide().chain(Some(0)).collect();
    let source: Vec<u16> = path.canonicalize()?.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut width, mut height) = (0, 0);
    let mut pointer = std::ptr::null_mut();
    let mut message = [0i8; 512];
    let mut metadata=RawMetadata::default();
    // The C++ boundary validates dimensions/layout, catches library return codes,
    // and owns/frees LibRaw's internal allocations before returning.
    let status = unsafe { chromora_decode_raw(dll.as_ptr(), source.as_ptr(), &mut pointer, &mut width, &mut height, &mut metadata, message.as_mut_ptr(), message.len()) };
    if status != 0 {
        return Err(raw_error(unsafe { CStr::from_ptr(message.as_ptr()) }.to_string_lossy()));
    }
    if pointer.is_null() || width == 0 || height == 0 {
        return Err(raw_error("解码器没有返回有效像素。"));
    }
    struct Pixels(*mut u16);
    impl Drop for Pixels { fn drop(&mut self) { unsafe { chromora_free_raw(self.0) } } }
    let pixels_owner = Pixels(pointer);
    let length = (width as usize).checked_mul(height as usize).and_then(|v| v.checked_mul(3))
        .ok_or_else(|| raw_error("图像尺寸溢出。"))?;
    let samples = unsafe { std::slice::from_raw_parts(pixels_owner.0, length) };
    let pixels = Rgb32FImage::from_raw(width, height, samples.iter().map(|v| f32::from(*v) / 65535.0).collect())
        .ok_or_else(|| raw_error("无效 RGB16 数据长度。"))?;
    let mut capture_info = parse_capture_info(path, ImageFormat::Tiff, (width, height), (width, height), ColorType::Rgb16, None);
    capture_info.file_format = CaptureFileFormat::Raw;
    capture_info.color_conversion = super::ColorConversionState::RawDevelopedSrgb;
    // RGB16 is the developed buffer depth, not evidence of sensor bit depth.
    capture_info.bit_depth_per_sample = None;
    fill_raw_metadata(&mut capture_info,&metadata);
    // LibRaw already applies RAW orientation; do not rotate a second time.
    Ok(DecodedImage { pixels, capture_info })
}

#[cfg(not(windows))]
pub(crate) fn decode_raw(_path: &Path) -> CoreResult<DecodedImage> {
    Err(raw_error("当前打包的 LibRaw 后端仅支持 Windows。"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_extensions_do_not_capture_tiff_or_jpeg() {
        assert!(is_raw_path(Path::new("中文.NEF")));
        assert!(is_raw_path(Path::new("camera.RAF")));
        assert!(!is_raw_path(Path::new("edit.tif")));
        assert!(!is_raw_path(Path::new("edit.jpg")));
    }

    #[test]
    fn corrupt_raw_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("损坏.ARW");
        std::fs::write(&path, b"not a raw image").unwrap();
        assert!(decode_raw(&path).is_err());
    }

    fn set_text<const N:usize>(target:&mut [std::ffi::c_char;N],text:&str) {
        for (slot,&byte) in target.iter_mut().zip(text.as_bytes()){*slot=byte as std::ffi::c_char;}
    }

    #[test]
    fn native_metadata_fills_gaps_without_replacing_recorded_exif() {
        assert_eq!(std::mem::size_of::<RawMetadata>(),328);
        let mut info=parse_capture_info(Path::new("absent-fixture.raf"),ImageFormat::Tiff,(3,2),(3,2),ColorType::Rgb16,None);
        info.make=Some("Recorded maker".into());info.iso=Some(200);info.bit_depth_per_sample=None;
        let mut metadata=RawMetadata{iso:400.0,shutter:0.01,aperture:2.8,focal_length:35.0,equivalent_focal_length:53.0,orientation:6,..Default::default()};
        set_text(&mut metadata.make,"Fujifilm");set_text(&mut metadata.model,"X-T2");
        set_text(&mut metadata.lens,"XF35mmF1.4 R");set_text(&mut metadata.captured_at,"2017:01:14 17:43:41");
        fill_raw_metadata(&mut info,&metadata);
        assert_eq!(info.make.as_deref(),Some("Recorded maker"));assert_eq!(info.model.as_deref(),Some("X-T2"));
        assert_eq!(info.iso,Some(200));assert_eq!(info.lens_model.as_deref(),Some("XF35mmF1.4 R"));
        assert_eq!(info.exposure_time_seconds,Some(0.01));assert_eq!(info.aperture_f_number,Some(2.8));
        assert_eq!(info.focal_length_mm,Some(35.0));assert_eq!(info.equivalent_focal_length_35mm,Some(53.0));
        assert_eq!(info.captured_at_exif.as_deref(),Some("2017:01:14 17:43:41"));assert_eq!(info.orientation,Some(6));
        assert!(info.captured_at_offset_exif.is_none()&&info.bit_depth_per_sample.is_none()&&info.vendor_style_name.is_none()&&info.white_balance.is_none());
    }

    #[test]
    fn invalid_or_absent_native_metadata_stays_unknown() {
        let mut info=parse_capture_info(Path::new("absent-fixture.cr3"),ImageFormat::Tiff,(3,2),(3,2),ColorType::Rgb16,None);
        let mut metadata=RawMetadata{iso:f64::NAN,shutter:-1.0,aperture:f64::INFINITY,orientation:9,..Default::default()};
        set_text(&mut metadata.captured_at,"invalid");fill_raw_metadata(&mut info,&metadata);
        assert!(info.make.is_none()&&info.model.is_none()&&info.iso.is_none()&&info.exposure_time_seconds.is_none());
        assert!(info.aperture_f_number.is_none()&&info.captured_at_exif.is_none()&&info.orientation.is_none());
        metadata.iso=0.1;fill_raw_metadata(&mut info,&metadata);assert!(info.iso.is_none());
    }
}
