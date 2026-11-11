use gpui::{App, FontWeight, RenderImage, Window, canvas, div, point, px, rgb, prelude::*};
use super::{AppAction, Workbench, action_button, paint_line, theme};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PixelSample {
    pub x:u32, pub y:u32, pub width:u32, pub height:u32, pub rgb:[u8;3], pub original:bool,
}

fn sample(image:&RenderImage, position:[f32;2], original:bool) -> Option<PixelSample> {
    if image.frame_count()==0 || position.iter().any(|v|!v.is_finite()) {return None;}
    let size=image.size(0);
    let (width,height)=(u32::try_from(size.width.0).ok()?,u32::try_from(size.height.0).ok()?);
    if width==0 || height==0 {return None;}
    let x=((position[0].clamp(0.,1.)*width as f32) as u32).min(width-1);
    let y=((position[1].clamp(0.,1.)*height as f32) as u32).min(height-1);
    let offset=(y as usize*width as usize+x as usize)*4;
    let bgra=image.as_bytes(0)?.get(offset..offset+4)?;
    Some(PixelSample{x,y,width,height,rgb:[bgra[2],bgra[1],bgra[0]],original})
}

impl Workbench {
    pub(super) fn sample_preview(&mut self, position:[f32;2]) -> bool {
        let original=self.snapshot.peeking || (self.snapshot.compare && position[0]*100.<self.snapshot.split);
        let image=if original {self.snapshot.original_preview.as_ref()}else{self.snapshot.preview.as_ref()};
        let next=image.and_then(|image|sample(image,position,original));
        if self.sampled_pixel==next {return false;}
        self.sampled_pixel=next;
        true
    }
    pub(super) fn sample_text(&self) -> String {
        match &self.sampled_pixel {
            Some(p)=>format!("{}预览 RGB8  ·  ({}, {}) / {}×{}  ·  R {}   G {}   B {}",if p.original{"原图"}else{"调整后"},p.x,p.y,p.width,p.height,p.rgb[0],p.rgb[1],p.rgb[2]),
            None=>"指向照片查看预览 RGB8 数值".into(),
        }
    }
    pub(super) fn task_status(&self) -> gpui::Div {
        let indicator=if self.snapshot.error.is_some(){theme::error()}
            else if self.snapshot.busy||self.snapshot.rendering{theme::accent()}else{theme::faint()};
        let mut row=div().flex_shrink_0().flex().items_center().gap(px(9.)).px(px(16.)).py(px(6.))
            .border_t_1().border_color(theme::edge()).bg(theme::shell()).text_size(px(11.)).text_color(theme::muted())
            .child(div().w(px(5.)).h(px(5.)).flex_shrink_0().rounded(px(3.)).bg(indicator));
        let mut text=self.snapshot.status.clone();
        if let Some(progress)=&self.snapshot.progress {
            if !progress.current_file.is_empty(){text=format!("{}  {}",text,progress.current_file);}
        }
        row=row.child(div().flex_1().min_w_0().truncate().child(text));
        if let Some(progress)=&self.snapshot.progress {
            row=row.child(div().flex_shrink_0().px(px(7.)).py(px(3.)).rounded(px(3.)).bg(theme::well())
                .text_color(theme::text()).child(format!("{} / {}",progress.completed,progress.total)))
                .child(div().flex_shrink_0().text_color(if progress.failed>0{theme::error()}else{theme::faint()})
                    .child(format!("失败 {}",progress.failed)));
        }
        if self.snapshot.can_cancel_operation {
            row=row.child(action_button("cancel-operation","取消任务",true,false,AppAction::CancelOperation,self.on_action.clone()));
        }
        row
    }
    pub(super) fn diagnostics_panel(&self) -> gpui::Div {
        let channel_colors=[rgb(0xd8938b),rgb(0x92b59b),rgb(0x91b2d4)];
        let mut legend=div().flex().items_center().gap(px(7.)).text_size(px(10.));
        for (label,color) in ["R","G","B"].into_iter().zip(channel_colors) {
            legend=legend.child(div().flex().items_center().gap(px(3.))
                .child(div().w(px(3.)).h(px(8.)).rounded(px(1.)).bg(color))
                .child(div().text_color(theme::muted()).child(label)));
        }
        let mut panel=div().flex().flex_col().gap(px(7.)).pb(px(14.)).border_b_1().border_color(theme::edge());
        panel=panel.child(div().flex().items_center().justify_between()
            .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::text()).child("RGB 直方图"))
            .child(legend));
        let Some(stats)=self.snapshot.diagnostics.clone() else {
            return panel.child(div().h(px(68.)).flex().items_center().justify_center().rounded(px(4.)).bg(theme::well())
                .text_size(px(11.)).text_color(theme::muted()).child("打开照片后查看色调分布"));
        };
        let histogram=stats.clone();
        let graph=canvas(move|_,_,_|(),move|bounds,_,window:&mut Window,_:&mut App|{
            let origin=bounds.origin;
            let width=f32::from(bounds.size.width);
            let height=f32::from(bounds.size.height);
            for fraction in [0.25,0.5,0.75] {
                paint_line(window,&[point(origin.x+px(width*fraction),origin.y),point(origin.x+px(width*fraction),origin.y+bounds.size.height)],rgb(0x34363a));
                paint_line(window,&[point(origin.x,origin.y+px(height*fraction)),point(origin.x+bounds.size.width,origin.y+px(height*fraction))],rgb(0x34363a));
            }
            let maximum=histogram.histograms.iter().flatten().copied().max().unwrap_or(1).max(1) as f32;
            for (channel,color) in channel_colors.into_iter().enumerate() {
                let points=(0..256).map(|bin|point(origin.x+px(bin as f32/255.*width),
                    origin.y+bounds.size.height-px(histogram.histograms[channel][bin] as f32/maximum*(height-2.)))).collect::<Vec<_>>();
                paint_line(window,&points,color);
            }
        }).w_full().h(px(56.));
        panel=panel.child(div().flex().flex_col().gap(px(3.)).px(px(6.)).pt(px(5.)).pb(px(3.)).rounded(px(4.)).bg(theme::well())
            .child(graph)
            .child(div().flex().justify_between().text_size(px(10.)).text_color(theme::faint()).child("0").child("128").child("255")))
            .child(div().flex().items_center().justify_between().text_size(px(11.))
                .child(div().text_color(theme::muted()).child(format!("通道为 0  {:.2}%",stats.shadow_percent())))
                .child(div().text_color(if stats.highlight_pixels>0{theme::error()}else{theme::muted()}).child(format!("通道为 255  {:.2}%",stats.highlight_percent()))))
            .child(div().text_size(px(10.)).text_color(theme::muted())
                .child(format!("RGB8 {}×{} · 任一通道触边像素占比；非 RAW 饱和",stats.width,stats.height)));
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Frame, RgbaImage};
    #[test]
    fn pixel_sampler_reads_bgra_and_clamps_right_bottom_edge() {
        let image=RenderImage::new(vec![Frame::new(RgbaImage::from_raw(2,1,vec![30,20,10,255,90,80,70,255]).unwrap())]);
        let first=sample(&image,[0.,0.],false).unwrap();
        assert_eq!(first.rgb,[10,20,30]);assert_eq!((first.x,first.y),(0,0));
        let last=sample(&image,[1.,1.],true).unwrap();
        assert_eq!(last.rgb,[70,80,90]);assert_eq!(last.x,1);assert!(last.original);
        assert!(sample(&image,[f32::NAN,0.],false).is_none());
    }
}
