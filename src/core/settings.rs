use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{CoreError, CoreResult};
use super::{geometry::GeometrySettings, masks::LocalMask};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkinRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

macro_rules! adjustment_settings {
    ($($field:ident => ($key:literal, $low:expr, $high:expr, $default:expr)),+ $(,)?) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default, rename_all = "camelCase")]
        pub struct AdjustmentSettings {
            $(pub $field: f32,)+
            pub curves: BTreeMap<String, Vec<[f32; 2]>>,
            pub skin_region: Option<SkinRegion>,
            pub geometry: GeometrySettings,
            pub masks: Vec<LocalMask>,
        }

        impl Default for AdjustmentSettings {
            fn default() -> Self {
                Self { $($field: $default,)+ curves: BTreeMap::new(), skin_region: None, geometry:GeometrySettings::default(), masks:Vec::new() }
            }
        }

        impl AdjustmentSettings {
            pub const CONTROLS: &'static [(&'static str, f32, f32)] = &[
                $(($key, $low, $high),)+
            ];

            pub fn value(&self, key: &str) -> Option<f32> {
                match key { $($key => Some(self.$field),)+ _ => None }
            }

            pub fn set_value(&mut self, key: &str, value: f32) -> CoreResult<()> {
                match key {
                    $($key => {
                        if !value.is_finite() || !($low..=$high).contains(&value) {
                            return Err(CoreError::InvalidProject(format!("{key} 数值超出范围")));
                        }
                        self.$field = value;
                    },)+
                    _ => return Err(CoreError::InvalidProject(format!("未知调节项：{key}"))),
                }
                Ok(())
            }
        }
    }
}

adjustment_settings! {
    exposure => ("exposure", -3.0, 3.0, 0.0),
    temperature => ("temperature", -100.0, 100.0, 0.0),
    tint => ("tint", -100.0, 100.0, 0.0),
    highlights => ("highlights", -100.0, 100.0, 0.0),
    shadows => ("shadows", -100.0, 100.0, 0.0),
    whites => ("whites", -100.0, 100.0, 0.0),
    blacks => ("blacks", -100.0, 100.0, 0.0),
    clarity => ("clarity", -100.0, 100.0, 0.0),
    contrast => ("contrast", -100.0, 100.0, 0.0),
    saturation => ("saturation", -100.0, 100.0, 0.0),
    vibrance => ("vibrance", -100.0, 100.0, 0.0),
    dehaze => ("dehaze", -100.0, 100.0, 0.0),
    sharpening => ("sharpening", 0.0, 100.0, 0.0),
    denoise => ("denoise", 0.0, 100.0, 0.0),
    color_balance => ("colorBalance", -100.0, 100.0, 0.0),
    blemishes => ("blemishes", 0.0, 100.0, 0.0),
    smoothing => ("smoothing", 0.0, 100.0, 0.0),
    skin_tone => ("skinTone", 0.0, 100.0, 0.0),
    texture => ("texture", 0.0, 100.0, 0.0),
    lut_strength => ("lutStrength", 0.0, 100.0, 100.0),
    post_exposure => ("postExposure", -3.0, 3.0, 0.0),
    post_temperature => ("postTemperature", -100.0, 100.0, 0.0),
    post_contrast => ("postContrast", -100.0, 100.0, 0.0),
    red_hue => ("redHue", -100.0, 100.0, 0.0),
    red_sat => ("redSat", -100.0, 100.0, 0.0),
    red_lum => ("redLum", -100.0, 100.0, 0.0),
    orange_hue => ("orangeHue", -100.0, 100.0, 0.0),
    orange_sat => ("orangeSat", -100.0, 100.0, 0.0),
    orange_lum => ("orangeLum", -100.0, 100.0, 0.0),
    yellow_hue => ("yellowHue", -100.0, 100.0, 0.0),
    yellow_sat => ("yellowSat", -100.0, 100.0, 0.0),
    yellow_lum => ("yellowLum", -100.0, 100.0, 0.0),
    green_hue => ("greenHue", -100.0, 100.0, 0.0),
    green_sat => ("greenSat", -100.0, 100.0, 0.0),
    green_lum => ("greenLum", -100.0, 100.0, 0.0),
    aqua_hue => ("aquaHue", -100.0, 100.0, 0.0),
    aqua_sat => ("aquaSat", -100.0, 100.0, 0.0),
    aqua_lum => ("aquaLum", -100.0, 100.0, 0.0),
    blue_hue => ("blueHue", -100.0, 100.0, 0.0),
    blue_sat => ("blueSat", -100.0, 100.0, 0.0),
    blue_lum => ("blueLum", -100.0, 100.0, 0.0),
    purple_hue => ("purpleHue", -100.0, 100.0, 0.0),
    purple_sat => ("purpleSat", -100.0, 100.0, 0.0),
    purple_lum => ("purpleLum", -100.0, 100.0, 0.0),
    magenta_hue => ("magentaHue", -100.0, 100.0, 0.0),
    magenta_sat => ("magentaSat", -100.0, 100.0, 0.0),
    magenta_lum => ("magentaLum", -100.0, 100.0, 0.0),
    shadow_hue => ("shadowHue", 0.0, 360.0, 0.0),
    shadow_sat => ("shadowSat", 0.0, 100.0, 0.0),
    midtone_hue => ("midtoneHue", 0.0, 360.0, 0.0),
    midtone_sat => ("midtoneSat", 0.0, 100.0, 0.0),
    highlight_hue => ("highlightHue", 0.0, 360.0, 0.0),
    highlight_sat => ("highlightSat", 0.0, 100.0, 0.0),
}

impl AdjustmentSettings {
    pub fn validate(&self) -> CoreResult<()> {
        self.geometry.validate()?;
        if self.masks.len()>32 {return Err(CoreError::InvalidProject("单张照片最多支持 32 个局部蒙版".into()));}
        let mut identifiers=std::collections::HashSet::new();
        for mask in &self.masks {
            mask.validate()?;
            if !identifiers.insert(&mask.id) {return Err(CoreError::InvalidProject("蒙版标识重复".into()));}
        }
        for &(key, low, high) in Self::CONTROLS {
            let value = self.value(key).expect("known adjustment");
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err(CoreError::InvalidProject(format!("{key} 数值超出范围")));
            }
        }
        for (channel, points) in &self.curves {
            if !matches!(channel.as_str(), "rgb" | "red" | "green" | "blue")
                || !(2..=12).contains(&points.len())
                || points.first().is_none_or(|p| p[0] != 0.0)
                || points.last().is_none_or(|p| p[0] != 1.0)
                || points.iter().flatten().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                || points.windows(2).any(|p| p[0][0] >= p[1][0])
            {
                return Err(CoreError::InvalidProject(format!("{channel} 曲线格式无效")));
            }
        }
        if let Some(r) = self.skin_region {
            if [r.x, r.y, r.width, r.height].iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
                return Err(CoreError::InvalidProject("皮肤选区格式无效".into()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_json_defaults_and_validation() {
        let settings: AdjustmentSettings = serde_json::from_str(r#"{"exposure":0.5,"redHue":20,"skinRegion":{"x":0.1,"y":0.2,"width":0.4,"height":0.5}}"#).unwrap();
        assert_eq!(settings.red_hue, 20.0);
        assert_eq!(settings.lut_strength, 100.0);
        settings.validate().unwrap();
        let mut invalid = settings;
        invalid.curves.insert("rgb".into(), vec![[0.0, 0.0], [0.0, 1.0]]);
        assert!(invalid.validate().is_err());
        assert!(invalid.set_value("temperature", f32::NAN).is_err());
    }

    #[test]
    fn edit_tools_default_for_old_settings_and_reject_duplicate_masks() {
        let mut settings:AdjustmentSettings=serde_json::from_str("{}").unwrap();
        assert_eq!(settings.geometry,GeometrySettings::default());assert!(settings.masks.is_empty());
        let mask=LocalMask::default();settings.masks=vec![mask.clone(),mask];
        assert!(settings.validate().is_err());
        settings.masks.pop();settings.geometry.quarter_turns=1;
        settings.validate().unwrap();
        let bytes=serde_json::to_vec(&settings).unwrap();assert_eq!(serde_json::from_slice::<AdjustmentSettings>(&bytes).unwrap(),settings);
    }
}
