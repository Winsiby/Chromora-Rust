mod document;
mod error;
mod export;
mod image;
mod metadata;
mod project;
mod workspace;
mod settings;
mod adjustments;
mod lut;
mod io;
mod jpeg;
mod task;
mod diagnostics;
pub use task::RenderCancellation;
pub use diagnostics::PreviewDiagnostics;
mod raw;
mod preset;
mod geometry;
mod masks;
mod style_preview;
mod colorspace;

pub use document::{ColorPipeline, PhotoDocument, SCHEMA_VERSION, WorkingImage};
pub use error::{CoreError, CoreResult};
pub use export::{DEFAULT_JPEG_QUALITY, export_jpeg};
pub use export::{export_png, export_tiff, export_psd, open_in_patchy, patchy_path};
pub use export::{ExportOptions, ExportKind, export_with_options, protect_export_destinations};
pub use colorspace::ExportColorSpace;
pub use raw::{libraw_path, is_raw_path};
pub use preset::{load_preset, save_preset};
pub use preset::{PresetEntry, discover_presets, copy_color_settings};
pub use image::{
    EXPOSURE_STEP_EV, MAX_EXPOSURE_EV, MIN_EXPOSURE_EV, import_image, open_image, open_image_with_pipeline, render_preview,
    set_exposure,
};
pub use metadata::{
    CaptureFileFormat, CaptureInfo, ColorConversionState, IccProfileSummary, PhotoInfo, WhiteBalance, WhiteBalanceMode,
    WorkingColorSpace, WorkingImageMetadata, WorkingSampleFormat,
};
pub use project::{load_project, save_project};
pub use project::{WorkspaceDocument, load_workspace, save_workspace};
pub use workspace::{ProjectEditState, ProjectHistory, LoadedWorkspace, MAX_WORKSPACE_PHOTOS, load_workspace_with_history, load_workspace_with_progress, save_workspace_with_history};
pub use settings::{AdjustmentSettings, SkinRegion};
pub use lut::{Lut3d, load_cube};
pub use image::{render_rgb_f32, render_preview_at, set_settings, set_lut, hydrate_document};
pub use image::{prepare_preview, prepare_thumbnail, render_original, thumbnail_document};
pub use style_preview::{
    STYLE_PREVIEW_LONG_EDGE, render_lut_style_preview, render_preset_style_preview, style_icon_shows_photo,
    style_preview_key, style_preview_settings,
};
pub use image::{render_rgb_f32_cancellable, render_preview_at_cancellable, render_original_cancellable, prepare_preview_cancellable};
pub use geometry::{GeometrySettings, NormalizedRect, output_dimensions, source_to_output, output_to_source, apply_geometry};
pub use masks::{LocalMask, LocalAdjustments, MaskShape};

/// Lowercase hex for a byte slice, used for the SHA-256 fields stored in projects and LUTs.
pub fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}
