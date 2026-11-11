use gpui::{div, px, FontWeight, IntoElement, ParentElement, Styled, prelude::*};
use chromora::core::{CaptureFileFormat, ColorConversionState, PhotoInfo, WhiteBalanceMode};
use super::theme;
pub(super) fn photo_info_panel(
    info: Option<&PhotoInfo>,
    dimensions: Option<(u32, u32)>,
    has_image: bool,
) -> impl IntoElement {
    let mut panel = div()
        .id("photo-info-scroll")
        .flex()
        .flex_col()
        .flex_shrink_0()
        .gap(px(16.0));

    if has_image {
        let (capture_rows, file_rows) = photo_info_rows(info, dimensions);
        panel = panel
            .child(info_group("拍摄", capture_rows))
            .child(info_group("文件", file_rows))
            .child(
                div()
                    .text_xs()
                    .text_color(theme::muted())
                    .child("相机风格仅显示原始记录，不代表预览效果。"),
            );
    } else {
        panel = panel.child(
            div()
                .mt(px(10.0))
                .text_xs()
                .text_color(theme::muted())
                .child("打开照片后，这里会显示文件中实际记录的拍摄信息。"),
        );
    }

    panel
}

fn info_group(title: &'static str, rows: Vec<(&'static str, String)>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .py(px(7.0))
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(title),
        )
        .children(rows.into_iter().map(|(label, value)| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .py(px(6.0))
                .border_b_1()
                .border_color(theme::edge())
                .min_w_0()
                .child(
                    div()
                        .w(px(92.0))
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(theme::muted())
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(if value == "无数据" {
                            theme::muted()
                        } else {
                            theme::text()
                        })
                        .text_right()
                        .child(value),
                )
        }))
}


fn photo_info_rows(
    info: Option<&PhotoInfo>,
    fallback_dimensions: Option<(u32, u32)>,
) -> (Vec<(&'static str, String)>, Vec<(&'static str, String)>) {
    let missing = || "无数据".to_owned();
    let Some(info) = info else {
        let dimensions = fallback_dimensions
            .map(|(width, height)| format!("{width} × {height} px"))
            .unwrap_or_else(missing);
        return (
            vec![
                ("拍摄时间", missing()),
                ("相机", missing()),
                ("镜头", missing()),
                ("焦距 / 等效", missing()),
                ("光圈", missing()),
                ("快门", missing()),
                ("ISO", missing()),
                ("曝光补偿", missing()),
                ("白平衡", missing()),
                ("相机风格", missing()),
            ],
            vec![
                ("格式", missing()),
                ("文件大小", missing()),
                ("尺寸", dimensions),
                ("方向", missing()),
                ("位深", missing()),
                ("色彩配置", missing()),
            ],
        );
    };

    let captured_at = match (&info.captured_at_exif, &info.captured_at_offset_exif) {
        (Some(value), Some(offset)) => format!("{value} {offset}"),
        (Some(value), None) => value.clone(),
        (None, _) => missing(),
    };
    let camera = join_known([info.make.as_deref(), info.model.as_deref()], " ");
    let focal = match (info.focal_length_mm, info.equivalent_focal_length_35mm) {
        (Some(actual), Some(equivalent)) => {
            format!("{} mm / {} mm eq", number(actual), number(equivalent))
        }
        (Some(actual), None) => format!("{} mm", number(actual)),
        (None, Some(equivalent)) => format!("无数据 / {} mm eq", number(equivalent)),
        (None, None) => missing(),
    };
    let white_balance = info
        .white_balance
        .as_ref()
        .map(|balance| {
            let mut parts = Vec::new();
            if let Some(mode) = balance.mode {
                parts.push(match mode {
                    WhiteBalanceMode::Auto => "自动".to_owned(),
                    WhiteBalanceMode::Manual => "手动".to_owned(),
                });
            }
            if let Some(temperature) = balance.temperature_kelvin {
                parts.push(format!("色温 {temperature} K"));
            }
            if let Some(tint) = balance.tint {
                parts.push(format!("色调 {tint:+.1}"));
            }
            parts.join(" / ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(missing);
    let dimensions = format!(
        "{} × {} px",
        info.source_dimensions.0, info.source_dimensions.1
    );
    let format = match info.file_format {
        CaptureFileFormat::Jpeg => "JPG",
        CaptureFileFormat::Png => "PNG",
        CaptureFileFormat::Tiff => "TIFF",
        CaptureFileFormat::Raw => "RAW",
    };
    let profile = match (
        &info.icc_profile.description,
        info.icc_profile.embedded,
        info.icc_profile.bytes,
    ) {
        (Some(description), _, _) => description.clone(),
        (None, true, Some(bytes)) => format!("嵌入 ICC / {}", format_bytes(bytes as u64)),
        (None, true, None) => "嵌入 ICC".to_owned(),
        (None, false, _) => "未嵌入 ICC".to_owned(),
    };

    let capture_rows = vec![
        ("拍摄时间", captured_at),
        ("相机", camera),
        ("镜头", info.lens_model.clone().unwrap_or_else(missing)),
        ("焦距 / 等效", focal),
        (
            "光圈",
            info.aperture_f_number
                .map(|value| format!("f/{}", number(value)))
                .unwrap_or_else(missing),
        ),
        (
            "快门",
            info.exposure_time_seconds
                .filter(|seconds| *seconds > 0.0)
                .map(format_shutter)
                .unwrap_or_else(missing),
        ),
        (
            "ISO",
            info.iso
                .map(|value| value.to_string())
                .unwrap_or_else(missing),
        ),
        (
            "曝光补偿",
            info.exposure_compensation_ev
                .map(|value| format!("{value:+.1} EV"))
                .unwrap_or_else(missing),
        ),
        ("白平衡", white_balance),
        (
            "相机风格",
            info.vendor_style_name.clone().unwrap_or_else(missing),
        ),
    ];

    let file_rows = vec![
        ("格式", format.to_owned()),
        (
            "文件大小",
            info.file_size_bytes
                .map(format_bytes)
                .unwrap_or_else(missing),
        ),
        ("尺寸", dimensions),
        (
            "方向",
            info.orientation
                .map(format_orientation)
                .unwrap_or_else(missing),
        ),
        (
            "位深",
            info.bit_depth_per_sample
                .map(|bits| format!("{bits} bit / channel"))
                .unwrap_or_else(missing),
        ),
        ("色彩配置", profile),
        ("色彩处理", match &info.color_conversion {
            ColorConversionState::AssumedSrgb => "无 ICC，按 sRGB 解释".to_owned(),
            ColorConversionState::ConvertedToSrgb => "ICC 已转换为 sRGB".to_owned(),
            ColorConversionState::RawDevelopedSrgb => "RAW 显影为 sRGB".to_owned(),
            ColorConversionState::LegacyIccFallback{..} => "旧版兼容：ICC 未转换".to_owned(),
        }),
    ];
    (capture_rows, file_rows)
}

fn join_known<const N: usize>(parts: [Option<&str>; N], separator: &str) -> String {
    let joined = parts
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(separator);
    if joined.is_empty() {
        "无数据".to_owned()
    } else {
        joined
    }
}

fn number(value: f64) -> String {
    if (value - value.round()).abs() < 0.05 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

fn format_shutter(seconds: f64) -> String {
    if seconds < 1.0 {
        let denominator = (1.0 / seconds).round().max(1.0) as u32;
        format!("1/{denominator} s")
    } else {
        format!("{seconds:.1} s")
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.0} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn format_orientation(orientation: u8) -> String {
    let meaning = match orientation {
        1 => "正常",
        2 => "左右镜像",
        3 => "旋转 180°",
        4 => "上下镜像",
        5 => "转置镜像",
        6 => "顺时针 90°",
        7 => "反向转置",
        8 => "逆时针 90°",
        _ => "未知方向",
    };
    format!("{meaning} (EXIF {orientation})")
}


