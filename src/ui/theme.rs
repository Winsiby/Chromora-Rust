use gpui::{Rgba, rgb};

// Neutral surroundings keep the photograph, rather than the interface, dominant.
pub const SHELL_HEX: u32 = 0x151619;
pub const CANVAS_HEX: u32 = 0x111214;
pub const PANEL_HEX: u32 = 0x1e2023;
pub const WELL_HEX: u32 = 0x17181b;
pub const EDGE_HEX: u32 = 0x34363b;
pub const TEXT_HEX: u32 = 0xedece9;
pub const MUTED_HEX: u32 = 0xa3a6ac;

// Semantic colors are used sparingly, never as decorative accents.
pub const PAPER_HEX: u32 = 0xcbb38e;
pub const ERROR_HEX: u32 = 0xe6a29b;

pub fn shell() -> Rgba {
    rgb(SHELL_HEX)
}
pub fn canvas() -> Rgba {
    rgb(CANVAS_HEX)
}
pub fn panel() -> Rgba {
    rgb(PANEL_HEX)
}
pub fn well() -> Rgba {
    rgb(WELL_HEX)
}
pub fn edge() -> Rgba {
    rgb(EDGE_HEX)
}
pub fn text() -> Rgba {
    rgb(TEXT_HEX)
}
pub fn muted() -> Rgba {
    rgb(MUTED_HEX)
}
pub fn paper() -> Rgba {
    rgb(PAPER_HEX)
}
pub fn accent()->Rgba{paper()}
pub fn accent_hover()->Rgba{rgb(0xdcc8aa)}
pub fn elevated()->Rgba{rgb(0x282a2d)}
pub fn faint()->Rgba{rgb(0x73777e)}
pub fn hover()->Rgba{rgb(0x303236)}
pub fn error() -> Rgba {
    rgb(ERROR_HEX)
}

pub const TOOL_RAIL_WIDTH: f32 = 76.0;
pub const INSPECTOR_WIDTH: f32 = 332.0;
pub const TOP_BAR_HEIGHT: f32 = 62.0;
pub const FILMSTRIP_HEIGHT: f32 = 112.0;
pub const CANVAS_HEADER_HEIGHT:f32=48.;
pub const PREVIEW_PADDING:f32=24.;
