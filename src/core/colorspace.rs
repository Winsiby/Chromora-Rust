//! Output color spaces for finished files.
//!
//! The working buffer stays float sRGB: every adjustment, mask and style runs
//! in the same space as before. A delivery may then be converted to a wider
//! output space and tagged with the matching ICC profile. Conversion is a
//! matrix/transfer step after the full edit pipeline, so renders are unchanged
//! and only the file numbers and profile differ.

/// Color space written into JPEG, PNG and TIFF deliveries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportColorSpace {
    /// The working space itself; no conversion, so nothing can clip.
    #[default]
    Srgb,
    /// Display P3 primaries with the sRGB transfer curve (D65 white).
    DisplayP3,
    /// Adobe RGB (1998) primaries with a 563/256 gamma (D65 white).
    AdobeRgb,
    /// ProPhoto RGB primaries with a 1.8 gamma (D50 white, Bradford-adapted).
    ProPhotoRgb,
}

impl ExportColorSpace {
    pub const ALL: [Self; 4] = [Self::Srgb, Self::DisplayP3, Self::AdobeRgb, Self::ProPhotoRgb];

    pub fn label(self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::DisplayP3 => "Display P3",
            Self::AdobeRgb => "Adobe RGB",
            Self::ProPhotoRgb => "ProPhoto RGB",
        }
    }
}

type Mat3 = [[f64; 3]; 3];

/// sRGB primaries and D65 white, as IEC 61966-2-1.
const SRGB_TO_XYZ_D65: Mat3 = [
    [0.412_390_8, 0.357_584_34, 0.180_480_79],
    [0.212_639_01, 0.715_168_68, 0.072_192_32],
    [0.019_330_82, 0.119_194_78, 0.950_532_15],
];
/// Inverse of the Display P3 matrix (DCI-P3 primaries, D65 white).
const XYZ_D65_TO_P3: Mat3 = [
    [2.493_496_91, -0.931_383_62, -0.402_710_78],
    [-0.829_488_97, 1.762_664_06, 0.023_624_69],
    [0.035_845_83, -0.076_172_39, 0.956_884_52],
];
/// Inverse of the Adobe RGB (1998) matrix (D65 white).
const XYZ_D65_TO_ADOBE: Mat3 = [
    [2.041_587_9, -0.565_006_97, -0.344_731_35],
    [-0.969_243_64, 1.875_967_5, 0.041_555_06],
    [0.013_444_28, -0.118_362_39, 1.015_174_99],
];
/// Bradford chromatic adaptation from D65 to D50, as ProPhoto RGB expects.
const BRADFORD_D65_TO_D50: Mat3 = [
    [1.047_886, 0.022_918_77, -0.050_216_1],
    [0.029_581_78, 0.990_483_52, -0.017_078_71],
    [-0.009_251_88, 0.015_072_61, 0.751_678_13],
];
/// Inverse of the ProPhoto RGB matrix (D50 white).
const XYZ_D50_TO_PROPHOTO: Mat3 = [
    [1.345_946_11, -0.255_602_33, -0.051_107_91],
    [-0.544_604_61, 1.508_175_49, 0.020_526_46],
    [0.0, 0.0, 1.212_268_15],
];

fn mul(matrix: &Mat3, value: [f64; 3]) -> [f64; 3] {
    [
        matrix[0][0] * value[0] + matrix[0][1] * value[1] + matrix[0][2] * value[2],
        matrix[1][0] * value[0] + matrix[1][1] * value[1] + matrix[1][2] * value[2],
        matrix[2][0] * value[0] + matrix[2][1] * value[1] + matrix[2][2] * value[2],
    ]
}

fn srgb_decode(value: f64) -> f64 {
    if value <= 0.040_45 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
}
fn srgb_encode(value: f64) -> f64 {
    if value <= 0.003_130_8 { 12.92 * value } else { 1.055 * value.powf(1.0 / 2.4) - 0.055 }
}

/// Adobe RGB (1998) gamma: 563/256.
fn adobe_decode(value: f64) -> f64 { value.max(0.0).powf(563.0 / 256.0) }
fn adobe_encode(value: f64) -> f64 { value.max(0.0).powf(256.0 / 563.0) }

/// ProPhoto RGB gamma: 1.8.
fn pro_photo_decode(value: f64) -> f64 { value.max(0.0).powf(1.8) }
fn pro_photo_encode(value: f64) -> f64 { value.max(0.0).powf(1.0 / 1.8) }

fn clamp_to_unit(value: f64) -> f64 { value.clamp(0.0, 1.0) }

fn linearize(space: ExportColorSpace, rgb: [f64; 3]) -> [f64; 3] {
    rgb.map(|v| match space {
        ExportColorSpace::Srgb | ExportColorSpace::DisplayP3 => srgb_decode(v),
        ExportColorSpace::AdobeRgb => adobe_decode(v),
        ExportColorSpace::ProPhotoRgb => pro_photo_decode(v),
    })
}

fn encode(space: ExportColorSpace, linear: [f64; 3]) -> [f64; 3] {
    linear.map(|v| clamp_to_unit(match space {
        ExportColorSpace::Srgb | ExportColorSpace::DisplayP3 => srgb_encode(v),
        ExportColorSpace::AdobeRgb => adobe_encode(v),
        ExportColorSpace::ProPhotoRgb => pro_photo_encode(v),
    }))
}

/// Encodes an sRGB working value into the target space. Out-of-gamut colors clip.
pub fn from_srgb(space: ExportColorSpace, rgb: [f32; 3]) -> [f32; 3] {
    if space == ExportColorSpace::Srgb { return rgb; }
    let linear = linearize(ExportColorSpace::Srgb, rgb.map(f64::from));
    let xyz = mul(&SRGB_TO_XYZ_D65, linear);
    let target = match space {
        ExportColorSpace::DisplayP3 => mul(&XYZ_D65_TO_P3, xyz),
        ExportColorSpace::AdobeRgb => mul(&XYZ_D65_TO_ADOBE, xyz),
        ExportColorSpace::ProPhotoRgb => mul(&XYZ_D50_TO_PROPHOTO, mul(&BRADFORD_D65_TO_D50, xyz)),
        ExportColorSpace::Srgb => unreachable!("sRGB returns early"),
    };
    encode(space, target).map(|v| v as f32)
}

/// Inverse of from_srgb, used to verify the conversion matrix round trip.
#[cfg(test)]
pub fn to_srgb(space: ExportColorSpace, rgb: [f32; 3]) -> [f32; 3] {
    if space == ExportColorSpace::Srgb { return rgb; }
    let linear = linearize(space, rgb.map(f64::from));
    let srgb_linear = match space {
        ExportColorSpace::DisplayP3 => mul(&inverse(&XYZ_D65_TO_P3), linear),
        ExportColorSpace::AdobeRgb => mul(&inverse(&XYZ_D65_TO_ADOBE), linear),
        ExportColorSpace::ProPhotoRgb => {
            let d50 = mul(&inverse(&XYZ_D50_TO_PROPHOTO), linear);
            mul(&inverse(&BRADFORD_D65_TO_D50), d50)
        }
        ExportColorSpace::Srgb => unreachable!("sRGB returns early"),
    };
    encode(ExportColorSpace::Srgb, mul(&inverse(&SRGB_TO_XYZ_D65), srgb_linear)).map(|v| v as f32)
}

/// 3x3 inverse by cofactors; keeps the tunable matrices above single-sourced.
#[cfg(test)]
fn inverse(m: &Mat3) -> Mat3 {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let r = 1.0 / det;
    [
        [(m[1][1] * m[2][2] - m[1][2] * m[2][1]) * r, (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * r, (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * r],
        [(m[1][2] * m[2][0] - m[1][0] * m[2][2]) * r, (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * r, (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * r],
        [(m[1][0] * m[2][1] - m[1][1] * m[2][0]) * r, (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * r, (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * r],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(left: [f32; 3], right: [f32; 3], tolerance: f32) -> bool {
        left.iter().zip(right).all(|(a, b)| (a - b).abs() <= tolerance)
    }

    #[test]
    fn display_p3_keeps_the_srgb_transfer_curve_so_grays_are_unchanged() {
        for value in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let gray = [value; 3];
            assert!(close(from_srgb(ExportColorSpace::DisplayP3, gray), gray, 1e-6));
        }
    }

    #[test]
    fn srgb_red_maps_to_the_published_display_p3_numbers() {
        let p3 = from_srgb(ExportColorSpace::DisplayP3, [1.0, 0.0, 0.0]);
        assert!(close(p3, [0.917_49, 0.200_29, 0.138_56], 2e-4), "{p3:?}");
    }

    #[test]
    fn adobe_rgb_and_prophoto_keep_grays_neutral_but_reshape_the_curve() {
        let gray = [0.5f32; 3];
        for space in [ExportColorSpace::AdobeRgb, ExportColorSpace::ProPhotoRgb] {
            let converted = from_srgb(space, gray);
            assert!((converted[0] - converted[1]).abs() < 1e-5 && (converted[1] - converted[2]).abs() < 1e-5,
                "{space:?} must stay neutral: {converted:?}");
            assert!((converted[0] - gray[0]).abs() > 1e-3, "{space:?} keeps its own transfer curve");
        }
    }

    #[test]
    fn in_gamut_colors_round_trip_through_every_output_space() {
        for space in ExportColorSpace::ALL {
            for color in [[0.2f32, 0.4, 0.6], [0.9, 0.35, 0.1], [0.05, 0.8, 0.3]] {
                let there = from_srgb(space, color);
                let back = to_srgb(space, there);
                assert!(close(back, color, 2e-4), "{space:?} {color:?} -> {there:?} -> {back:?}");
            }
        }
    }
}
