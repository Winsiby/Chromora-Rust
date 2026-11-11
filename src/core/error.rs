use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("操作已取消")]
    Cancelled,
    #[error("不支持的图像格式：{0}")]
    UnsupportedFormat(PathBuf),
    #[error("不支持的图像采样格式：{0:?}，当前支持 8/16 位整数图像")]
    UnsupportedBitDepth(image::ColorType),
    #[error("暂不支持透明/半透明像素：{0}")]
    UnsupportedTransparency(PathBuf),
    #[error("图像文件无效或无法解码：{0}")]
    Decode(#[from] image::ImageError),
    #[error("文件操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("工程文件无效：{0}")]
    Project(#[from] serde_json::Error),
    #[error("工程文件无效：{0}")]
    InvalidProject(String),
    #[error("工程版本 {0} 不受支持")]
    UnsupportedProjectVersion(u32),
    #[error("工程引用的原片缺失或已改变：{0}")]
    MissingOrChangedSource(PathBuf),
    #[error("工程不能保存到原片或原片副本：{0}")]
    ProtectedOriginalDestination(PathBuf),
    #[error("曝光值 {0} 无效；范围为 -3..=+3 EV，步进为 0.05 EV")]
    InvalidExposure(f32),
    #[error("JPEG 参数无效：{0}")]
    InvalidJpeg(String),
    #[error("无法将图片编码为 JPEG：{0}")]
    Encode(#[from] jpeg_encoder::EncodingError),
    #[error("ICC 色彩配置文件转换失败：{0}")]
    Icc(#[from] moxcms::CmsError),
    #[error("无法按正确色彩打开 {path}：{stage}失败（{message}）。请修复原文件的 ICC 配置后重新导入；未按 sRGB 静默降级。")]
    InvalidIcc { path: PathBuf, stage: &'static str, message: String },
    #[error("图像尺寸过大，JPEG 单边不能超过 65535 像素")]
    JpegDimensionsTooLarge,
    #[error("TIFF 编解码失败：{0}")]
    Tiff(#[from] tiff::TiffError),
    #[error("导出参数无效：{0}")]
    InvalidExport(String),
    #[error("RAW 显影失败：{0}")]
    Raw(String),
    #[error("外部精修工具启动失败：{0}")]
    ExternalTool(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
