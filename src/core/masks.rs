use serde::{Deserialize, Serialize};
use super::{CoreError, CoreResult};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LocalAdjustments {
    pub exposure: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub temperature: f32,
    pub saturation: f32,
}

impl LocalAdjustments {
    pub const CONTROLS: &'static [(&'static str,f32,f32)] = &[
        ("exposure",-3.0,3.0),("highlights",-100.0,100.0),("shadows",-100.0,100.0),
        ("temperature",-100.0,100.0),("saturation",-100.0,100.0),
    ];
    pub fn value(&self,key:&str)->Option<f32>{
        Some(match key {"exposure"=>self.exposure,"highlights"=>self.highlights,"shadows"=>self.shadows,
            "temperature"=>self.temperature,"saturation"=>self.saturation,_=>return None})
    }
    pub fn set_value(&mut self,key:&str,value:f32)->CoreResult<()> {
        let &(_,low,high)=Self::CONTROLS.iter().find(|(name,_,_)|*name==key)
            .ok_or_else(||CoreError::InvalidProject("未知局部调节项".into()))?;
        if !value.is_finite() || !(low..=high).contains(&value) {
            return Err(CoreError::InvalidProject("局部调节数值超出范围".into()));
        }
        match key {"exposure"=>self.exposure=value,"highlights"=>self.highlights=value,"shadows"=>self.shadows=value,
            "temperature"=>self.temperature=value,"saturation"=>self.saturation=value,_=>unreachable!()}
        Ok(())
    }
    pub fn validate(&self)->CoreResult<()> {
        for &(key,low,high) in Self::CONTROLS {
            let value=self.value(key).expect("known local adjustment");
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err(CoreError::InvalidProject(format!("局部 {key} 数值超出范围")));
            }
        }
        Ok(())
    }
    pub fn is_neutral(&self)->bool { *self==Self::default() }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MaskShape {
    Linear { start:[f32;2], end:[f32;2] },
    Radial { center:[f32;2], radius:[f32;2], feather:f32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMask {
    pub id:String,
    pub name:String,
    pub enabled:bool,
    pub invert:bool,
    pub shape:MaskShape,
    #[serde(default)] pub adjustments:LocalAdjustments,
}

impl Default for LocalMask {
    fn default()->Self {
        Self {id:uuid::Uuid::new_v4().to_string(),name:"蒙版".into(),enabled:true,invert:false,
            shape:MaskShape::Radial{center:[0.5,0.5],radius:[0.25,0.3],feather:0.5},adjustments:LocalAdjustments::default()}
    }
}

fn point_valid(point:[f32;2])->bool {point.iter().all(|v|v.is_finite() && (0.0..=1.0).contains(v))}
fn smoothstep(value:f32)->f32 {let v=value.clamp(0.0,1.0);v*v*(3.0-2.0*v)}

impl LocalMask {
    pub fn validate(&self)->CoreResult<()> {
        if self.id.is_empty() || self.id.len()>128 || self.name.trim().is_empty() || self.name.chars().count()>80 {
            return Err(CoreError::InvalidProject("蒙版标识或名称无效".into()));
        }
        let valid=match self.shape {
            MaskShape::Linear{start,end}=>point_valid(start)&&point_valid(end)
                && (end[0]-start[0]).powi(2)+(end[1]-start[1]).powi(2)>0.00000001,
            MaskShape::Radial{center,radius,feather}=>point_valid(center)
                && radius.iter().all(|r|r.is_finite() && (0.0001..=2.0).contains(r))
                && feather.is_finite() && (0.0..=1.0).contains(&feather),
        };
        if !valid {return Err(CoreError::InvalidProject("蒙版位置、尺寸或羽化参数无效".into()));}
        self.adjustments.validate()
    }
    /// Coverage is defined in the EXIF-oriented full source, before geometry.
    pub fn coverage_at(&self,point:[f32;2])->f32 {
        self.coverage_at_with_size(point,1,1)
    }
    pub fn coverage_at_with_size(&self,point:[f32;2],width:u32,height:u32)->f32 {
        if !self.enabled {return 0.0;}
        let coverage=match self.shape {
            MaskShape::Linear{start,end}=>{
                let aspect=width.max(1) as f32/height.max(1) as f32;
                let dx=(end[0]-start[0])*aspect;let dy=end[1]-start[1];
                let length=dx*dx+dy*dy;
                if length<=0.00000001 {0.0}else{1.0-smoothstep(((point[0]-start[0])*aspect*dx+(point[1]-start[1])*dy)/length)}
            },
            MaskShape::Radial{center,radius,feather}=>{
                let dx=(point[0]-center[0])/radius[0].max(0.0001);
                let dy=(point[1]-center[1])/radius[1].max(0.0001);
                let distance=(dx*dx+dy*dy).sqrt();
                if feather<=0.0 {if distance<=1.0 {1.0}else{0.0}}
                else {1.0-smoothstep((distance-(1.0-feather))/feather)}
            },
        };
        if self.invert {1.0-coverage}else{coverage}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linear_transition_has_expected_sides_and_inversion() {
        let mut mask=LocalMask{shape:MaskShape::Linear{start:[0.5,0.2],end:[0.5,0.8]},..Default::default()};
        assert_eq!(mask.coverage_at([0.1,0.1]),1.0);
        assert!((mask.coverage_at([0.8,0.5])-0.5).abs()<0.00001);
        assert_eq!(mask.coverage_at([0.8,0.9]),0.0);
        mask.invert=true;assert_eq!(mask.coverage_at([0.1,0.1]),0.0);
        mask.enabled=false;assert_eq!(mask.coverage_at([0.8,0.9]),0.0);
    }
    #[test]
    fn radial_feather_is_elliptical_and_continuous() {
        let mask=LocalMask::default();
        assert_eq!(mask.coverage_at([0.5,0.5]),1.0);
        assert_eq!(mask.coverage_at([0.9,0.5]),0.0);
        assert!((mask.coverage_at([0.6875,0.5])-0.5).abs()<0.00001);
        assert!((mask.coverage_at([0.5,0.725])-0.5).abs()<0.00001);
    }
    #[test]
    fn malformed_masks_are_rejected_and_json_round_trips() {
        let mask=LocalMask::default();mask.validate().unwrap();
        let value=serde_json::to_vec(&mask).unwrap();assert_eq!(serde_json::from_slice::<LocalMask>(&value).unwrap(),mask);
        let invalid=LocalMask{shape:MaskShape::Linear{start:[0.3,0.3],end:[0.3,0.3]},..mask.clone()};assert!(invalid.validate().is_err());
        let invalid=LocalMask{shape:MaskShape::Radial{center:[0.5,0.5],radius:[f32::NAN,0.3],feather:0.5},..mask};assert!(invalid.validate().is_err());
    }
    #[test]
    fn diagonal_gradient_is_perpendicular_in_photo_pixels() {
        let mask=LocalMask{shape:MaskShape::Linear{start:[0.2,0.2],end:[0.8,0.8]},..Default::default()};
        assert!((mask.coverage_at_with_size([0.53,0.38],200,100)-0.5).abs()<0.00001);
    }
}
