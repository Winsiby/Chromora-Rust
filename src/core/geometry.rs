use image::{Rgb, Rgb32FImage};
use serde::{Deserialize, Serialize};
use super::{CoreError, CoreResult};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormalizedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for NormalizedRect {
    fn default() -> Self { Self { x: 0.0, y: 0.0, width: 1.0, height: 1.0 } }
}

impl NormalizedRect {
    pub fn validate(&self) -> CoreResult<()> {
        if [self.x,self.y,self.width,self.height].iter().any(|v| !v.is_finite())
            || self.x < 0.0 || self.y < 0.0 || self.width <= 0.0 || self.height <= 0.0
            || self.x + self.width > 1.000001 || self.y + self.height > 1.000001 {
            return Err(CoreError::InvalidProject("裁剪区域必须是照片内的有效矩形".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GeometrySettings {
    pub crop: Option<NormalizedRect>,
    pub quarter_turns: u8,
    pub straighten_degrees: f32,
}

impl GeometrySettings {
    pub fn validate(&self) -> CoreResult<()> {
        if self.quarter_turns > 3 || !self.straighten_degrees.is_finite()
            || !(-15.0..=15.0).contains(&self.straighten_degrees) {
            return Err(CoreError::InvalidProject("旋转或拉直参数无效".into()));
        }
        if let Some(rect) = self.crop { rect.validate()?; }
        Ok(())
    }
    pub fn without_crop(mut self) -> Self { self.crop = None; self }
}

fn rotated_dimensions(width: u32, height: u32, quarter: u8) -> (u32,u32) {
    if quarter % 2 == 1 { (height,width) } else { (width,height) }
}

fn crop_bounds(width: u32, height: u32, crop: Option<NormalizedRect>) -> (u32,u32,u32,u32) {
    if width == 0 || height == 0 { return (0,0,0,0); }
    let Some(r) = crop else { return (0,0,width,height); };
    let x = (r.x * width as f32).floor().clamp(0.0,(width-1) as f32) as u32;
    let y = (r.y * height as f32).floor().clamp(0.0,(height-1) as f32) as u32;
    let right = ((r.x+r.width)*width as f32).ceil().clamp((x+1) as f32,width as f32) as u32;
    let bottom = ((r.y+r.height)*height as f32).ceil().clamp((y+1) as f32,height as f32) as u32;
    (x,y,right-x,bottom-y)
}

pub fn output_dimensions(width: u32, height: u32, geometry: &GeometrySettings) -> (u32,u32) {
    let (w,h) = rotated_dimensions(width,height,geometry.quarter_turns);
    let (_,_,cw,ch) = crop_bounds(w,h,geometry.crop);
    (cw,ch)
}

fn straighten_parameters(width: u32, height: u32, angle: f32) -> (f32,f32,f32) {
    let (sin,cos) = angle.to_radians().sin_cos();
    let aspect = width.max(1) as f32 / height.max(1) as f32;
    // Zoom enough that every output corner lies inside the source rectangle.
    let scale = (cos.abs()+sin.abs()/aspect).max(cos.abs()+sin.abs()*aspect);
    (sin,cos,scale)
}

pub fn source_to_output(mut point: [f32;2], width: u32, height: u32, geometry: &GeometrySettings) -> [f32;2] {
    point = match geometry.quarter_turns % 4 {
        1 => [1.0-point[1],point[0]],
        2 => [1.0-point[0],1.0-point[1]],
        3 => [point[1],1.0-point[0]],
        _ => point,
    };
    let (w,h) = rotated_dimensions(width.max(1),height.max(1),geometry.quarter_turns);
    let (sin,cos,scale) = straighten_parameters(w,h,geometry.straighten_degrees);
    let x = (point[0]-0.5)*w as f32;
    let y = (point[1]-0.5)*h as f32;
    let fx = 0.5 + scale*(cos*x-sin*y)/w as f32;
    let fy = 0.5 + scale*(sin*x+cos*y)/h as f32;
    let (cx,cy,cw,ch) = crop_bounds(w,h,geometry.crop);
    [(fx*w as f32-cx as f32)/cw as f32,(fy*h as f32-cy as f32)/ch as f32]
}

pub fn output_to_source(point: [f32;2], width: u32, height: u32, geometry: &GeometrySettings) -> [f32;2] {
    let (w,h) = rotated_dimensions(width.max(1),height.max(1),geometry.quarter_turns);
    let (cx,cy,cw,ch) = crop_bounds(w,h,geometry.crop);
    let (sin,cos,scale) = straighten_parameters(w,h,geometry.straighten_degrees);
    let x = (cx as f32+point[0]*cw as f32-w as f32*0.5)/scale;
    let y = (cy as f32+point[1]*ch as f32-h as f32*0.5)/scale;
    let point = [0.5+(cos*x+sin*y)/w as f32,0.5+(-sin*x+cos*y)/h as f32];
    match geometry.quarter_turns % 4 {
        1 => [point[1],1.0-point[0]],
        2 => [1.0-point[0],1.0-point[1]],
        3 => [1.0-point[1],point[0]],
        _ => point,
    }
}

fn bilinear(image: &Rgb32FImage, x: f32, y: f32) -> Rgb<f32> {
    let x = x.clamp(0.0,(image.width()-1) as f32);
    let y = y.clamp(0.0,(image.height()-1) as f32);
    let x0=x.floor() as u32; let y0=y.floor() as u32;
    let x1=(x0+1).min(image.width()-1); let y1=(y0+1).min(image.height()-1);
    let tx=x-x0 as f32; let ty=y-y0 as f32;
    let a=image.get_pixel(x0,y0); let b=image.get_pixel(x1,y0);
    let c=image.get_pixel(x0,y1); let d=image.get_pixel(x1,y1);
    Rgb(std::array::from_fn(|i| (a[i]*(1.0-tx)+b[i]*tx)*(1.0-ty)+(c[i]*(1.0-tx)+d[i]*tx)*ty))
}

pub fn apply_geometry(image: &mut Rgb32FImage, geometry: &GeometrySettings) -> CoreResult<()> {
    geometry.validate()?;
    if image.width()==0 || image.height()==0 {
        return Err(CoreError::InvalidProject("构图操作需要有效照片".into()));
    }
    match geometry.quarter_turns {
        1 => *image=image::imageops::rotate90(image),
        2 => *image=image::imageops::rotate180(image),
        3 => *image=image::imageops::rotate270(image),
        _ => {},
    }
    if geometry.straighten_degrees != 0.0 {
        let (w,h)=image.dimensions();
        let (sin,cos,scale)=straighten_parameters(w,h,geometry.straighten_degrees);
        let rotated=Rgb32FImage::from_fn(w,h,|x,y| {
            let dx=(x as f32+0.5-w as f32*0.5)/scale;
            let dy=(y as f32+0.5-h as f32*0.5)/scale;
            bilinear(image,w as f32*0.5+cos*dx+sin*dy-0.5,h as f32*0.5-sin*dx+cos*dy-0.5)
        });
        *image=rotated;
    }
    if geometry.crop.is_some() {
        let (x,y,w,h)=crop_bounds(image.width(),image.height(),geometry.crop);
        if (x,y,w,h)!=(0,0,image.width(),image.height()) {
            *image=image::imageops::crop_imm(image,x,y,w,h).to_image();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_geometry_preserves_samples_and_allocation() {
        let mut image=Rgb32FImage::from_fn(9,7,|x,y|Rgb([x as f32/13.0,y as f32/11.0,0.123456]));
        let expected=image.clone();let ptr=image.as_raw().as_ptr();
        apply_geometry(&mut image,&GeometrySettings::default()).unwrap();
        assert_eq!(image,expected);assert_eq!(image.as_raw().as_ptr(),ptr);
    }
    #[test]
    fn quarter_turn_and_crop_have_exact_pixels_and_dimensions() {
        let mut image=Rgb32FImage::from_fn(4,2,|x,y|Rgb([(y*4+x) as f32,0.0,0.0]));
        let geometry=GeometrySettings{quarter_turns:1,crop:Some(NormalizedRect{x:0.0,y:0.25,width:1.0,height:0.5}),..Default::default()};
        assert_eq!(output_dimensions(4,2,&geometry),(2,2));
        apply_geometry(&mut image,&geometry).unwrap();
        assert_eq!(image.dimensions(),(2,2));
        assert_eq!(image.pixels().map(|p|p[0]).collect::<Vec<_>>(),vec![5.0,1.0,6.0,2.0]);
    }
    #[test]
    fn source_and_output_coordinates_round_trip_after_every_transform() {
        for quarter_turns in 0..4 { for angle in [-15.0,0.0,7.3,15.0] {
            let geometry=GeometrySettings{quarter_turns,straighten_degrees:angle,crop:Some(NormalizedRect{x:0.12,y:0.17,width:0.73,height:0.61})};
            for p in [[0.0,0.0],[0.2,0.7],[0.5,0.5],[1.0,1.0]] {
                let restored=output_to_source(source_to_output(p,101,63,&geometry),101,63,&geometry);
                assert!((restored[0]-p[0]).abs()<0.00001 && (restored[1]-p[1]).abs()<0.00001);
            }
        }}
    }
    #[test]
    fn straighten_fills_frame_and_retains_float_precision() {
        let mut image=Rgb32FImage::from_pixel(13,7,Rgb([0.123456,0.345678,0.789123]));
        apply_geometry(&mut image,&GeometrySettings{straighten_degrees:15.0,..Default::default()}).unwrap();
        for p in image.pixels(){for(actual,expected)in p.0.into_iter().zip([0.123456,0.345678,0.789123]){assert!((actual-expected).abs()<0.000001);}}
    }
    #[test]
    fn invalid_geometry_is_rejected() {
        for g in [GeometrySettings{quarter_turns:4,..Default::default()},GeometrySettings{straighten_degrees:f32::NAN,..Default::default()},
            GeometrySettings{crop:Some(NormalizedRect{x:0.8,y:0.0,width:0.4,height:1.0}),..Default::default()}] {assert!(g.validate().is_err());}
    }
}
