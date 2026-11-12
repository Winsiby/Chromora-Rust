use std::{cell::{Cell,RefCell}, collections::BTreeMap, rc::Rc, sync::{Arc,OnceLock}};
use gpui::{App, AnyElement, Bounds, Context, ExternalPaths, FocusHandle, FontWeight, Image, ImageFormat,
    ImageSource, IntoElement, MouseButton, ObjectFit, ParentElement, Pixels, Point, Render,
    ScrollHandle, SharedString, Styled, Window, canvas, div, img, point, prelude::*, px};
use chromora::core::SkinRegion;
use crate::app::{ActionHandler,AppAction,AppSnapshot,ExportFormat,ZoomMode,InputTarget};
use super::{theme,photo_info::photo_info_panel};
#[path="tools.rs"] mod tools;
#[path="diagnostics.rs"] mod diagnostics;
#[path="interaction.rs"] mod interaction;
use tools::ToolState;
use diagnostics::PixelSample;
use interaction::{ChromeSpace, ZoomFrame, APPROXIMATE_STYLE_NOTE, LUT_STYLE_LABEL, PARAMETER_STYLE_LABEL};

const INSET:f32=16.;
const HSL_BANDS:[(&str,&str);8]=[("red","红"),("orange","橙"),("yellow","黄"),("green","绿"),("aqua","青"),("blue","蓝"),("purple","紫"),("magenta","洋红")];
fn branding_icon()->Arc<Image>{static ICON:OnceLock<Arc<Image>>=OnceLock::new();ICON.get_or_init(||Arc::new(Image::from_bytes(ImageFormat::Png,include_bytes!("../../assets/branding/chromora-app-icon.png").to_vec()))).clone()}
#[derive(Clone,Copy,PartialEq)]enum DragMode{None,Split,Region,Tool}
pub struct Workbench {
    snapshot:AppSnapshot,on_action:ActionHandler,focus:FocusHandle,
    curve_channel:String,curve_point:usize,curve_dragging:bool,hsl_band:usize,grade_band:usize,
    editing_region:bool,region_start:Option<[f32;2]>,region_draft:Option<SkinRegion>,drag_mode:DragMode,
    slider_drags:RefCell<BTreeMap<String,Rc<Cell<bool>>>>,
    tools:ToolState,
    sampled_pixel:Option<PixelSample>,
    show_rail:bool, show_inspector:bool, show_filmstrip:bool,
    preview_scroll:ScrollHandle, filmstrip_scroll:ScrollHandle,
    pending_zoom:Option<PendingZoom>,
    swatch_cache:RefCell<BTreeMap<String,Vec<[u8;3]>>>,
}
struct PendingZoom { old_frame:(f32,f32), old_offset:(f32,f32), viewport:(f32,f32) }
impl Workbench {
    pub fn new(snapshot:AppSnapshot,on_action:ActionHandler,cx:&mut Context<Self>)->Self{
        Self{snapshot,on_action,focus:cx.focus_handle(),curve_channel:"rgb".into(),curve_point:0,curve_dragging:false,
            hsl_band:0,grade_band:0,editing_region:false,region_start:None,region_draft:None,drag_mode:DragMode::None,
            slider_drags:RefCell::new(BTreeMap::new()),tools:ToolState::default(),sampled_pixel:None,
            show_rail:true,show_inspector:true,show_filmstrip:true,
            preview_scroll:ScrollHandle::new(),filmstrip_scroll:ScrollHandle::new(),pending_zoom:None,
            swatch_cache:RefCell::new(BTreeMap::new())}
    }
    pub fn focus_handle(&self)->FocusHandle { self.focus.clone() }
    pub fn set_snapshot(&mut self,snapshot:AppSnapshot){
        let photo_changed=self.snapshot.current_index!=snapshot.current_index;
        if self.snapshot.preview.as_ref().map(|image|image.id)!=snapshot.preview.as_ref().map(|image|image.id)
            || self.snapshot.peeking!=snapshot.peeking || self.snapshot.compare!=snapshot.compare || self.snapshot.split!=snapshot.split {
            self.sampled_pixel=None;
        }
        if self.snapshot.panel!=snapshot.panel||self.snapshot.selected_mask!=snapshot.selected_mask{self.tools.input=None;}
        if self.snapshot.current_index!=snapshot.current_index || self.snapshot.photos.len()!=snapshot.photos.len()
            || self.snapshot.panel!=snapshot.panel {self.reset_interaction();}
        self.snapshot=snapshot;
        if photo_changed && self.snapshot.current_index<self.snapshot.photos.len() {
            self.filmstrip_scroll.scroll_to_item(self.snapshot.current_index);
        }
    }
    pub fn reset_interaction(&mut self){
        self.editing_region=false;self.region_start=None;self.region_draft=None;
        self.sampled_pixel=None;
        self.drag_mode=DragMode::None;self.curve_dragging=false;self.curve_point=0;
        self.snapshot.peeking=false;
        self.tools.input=None;self.tools.input_spec=None;self.tools.crop=None;self.tools.crop_anchor=None;self.tools.crop_before=None;
        self.tools.mask_draw=None;self.tools.mask_start=None;self.tools.mask_draft=None;self.tools.mask_handle=None;
        for dragging in self.slider_drags.borrow().values(){dragging.set(false);}
    }
    fn dispatch(&self,action:AppAction,window:&mut Window,cx:&mut Context<Self>){
        let handler=self.on_action.clone();window.defer(cx,move|window,app|handler(action,window,app));
    }
    fn enabled(&self)->bool{self.snapshot.image_size.is_some()&&!self.snapshot.busy}
    fn top_bar(&self)->impl IntoElement + use<> {
        let busy=self.snapshot.busy;let available=self.snapshot.image_size.is_some();
        div().h(px(theme::TOP_BAR_HEIGHT)).w_full().flex_shrink_0().flex().items_center().justify_between()
            .px(px(18.)).gap(px(12.)).bg(theme::shell()).border_b_1().border_color(theme::edge())
            .child(div().flex().flex_shrink_0().items_center().gap(px(10.))
                .child(div().w(px(34.)).h(px(34.)).rounded(px(8.)).overflow_hidden().bg(theme::well())
                    .child(img(ImageSource::Image(branding_icon())).size_full().object_fit(ObjectFit::Contain)))
                .child(div().flex().flex_col().gap(px(1.)).mr(px(16.))
                    .child(div().font_family("Segoe UI").text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child("Chromora"))
                    .child(div().text_size(px(10.)).text_color(theme::muted()).child("照片工作台")))
                .child(action_button("open-images","导入照片",!busy,false,AppAction::OpenImage,self.on_action.clone()))
                .child(action_button("open-folder","文件夹",!busy,false,AppAction::OpenFolder,self.on_action.clone()))
                .child(action_button("open-project","打开工程",!busy,false,AppAction::OpenProject,self.on_action.clone())))
            .child(div().flex().flex_1().min_w_0().items_center().justify_end().gap(px(10.))
                .child(div().max_w(px(200.)).min_w_0().flex().flex_col().gap(px(3.)).text_size(px(11.)).text_color(theme::muted())
                    .child(div().truncate().text_color(theme::text()).child(self.snapshot.project_name.clone()))
                    .child(div().truncate().child(self.snapshot.image_name.clone().unwrap_or_else(||"照片工作台".into()))))
                .when(self.snapshot.dirty,|d|d.child(div().flex().flex_shrink_0().items_center().gap(px(5.))
                    .child(div().w(px(5.)).h(px(5.)).rounded(px(3.)).bg(theme::accent()))
                    .child(div().text_size(px(10.)).text_color(theme::muted()).child("未保存"))))
                .child(action_button("save-project","保存工程",available&&!busy,false,AppAction::SaveProject,self.on_action.clone()))
                .child(action_button("export-current",format!("导出 {}",self.snapshot.export_format.label()),available&&!busy,true,AppAction::Export(self.snapshot.export_format),self.on_action.clone())))
    }

    fn tool_rail(&self,cx:&mut Context<Self>)->AnyElement {
        if !self.show_rail {
            return self.restore_strip("restore-rail","工具",true,cx);
        }
        let mut rail=div().w(px(theme::TOOL_RAIL_WIDTH)).h_full().flex_shrink_0().flex().flex_col().items_center().gap(px(5.)).py(px(15.))
            .bg(theme::shell()).border_r_1().border_color(theme::edge());
        for (id,label) in [("landscape","调色"),("portrait","人像"),("styles","风格"),("crop","构图"),("masks","局部"),("project","项目"),("info","信息")] {
            let selected=self.snapshot.panel==id;
            let click=self.on_action.clone();let keyboard=self.on_action.clone();
            rail=rail.child(div().id(SharedString::from(format!("tab-{id}"))).w(px(60.)).h(px(58.)).flex().flex_col().items_center().justify_center().gap(px(4.))
                .rounded(px(7.)).tab_stop(true).cursor_pointer().text_size(px(12.))
                .text_color(if selected{theme::text()}else{theme::muted()})
                .when(selected,|d|d.bg(theme::elevated()).border_l_2().border_color(theme::accent()))
                .hover(|d|d.bg(theme::elevated()).text_color(theme::text())).focus(|d|d.bg(theme::hover()))
                .child(tool_icon(id,selected)).child(label)
                .on_click(move|_,w,cx|click(AppAction::SetPanel(id),w,cx))
                .on_key_down(move|event,w,cx|{if !event.is_held&&matches!(event.keystroke.key.as_str(),"enter"|"return"|"space"|" "){cx.stop_propagation();keyboard(AppAction::SetPanel(id),w,cx);}}));
        }
        rail.child(div().flex_1()).child(self.chrome_button("collapse-rail","收起",cx.listener(|this,_,window,cx|{
            window.focus(&this.focus);this.show_rail=false;cx.notify();
        }))).into_any_element()
    }

    fn canvas_header(&self)->impl IntoElement + use<> {
        let enabled=self.snapshot.preview.is_some();
        let (fit,actual,double)=interaction::zoom_button_flags(&self.snapshot.zoom);
        div().h(px(theme::CANVAS_HEADER_HEIGHT)).flex_shrink_0().w_full().flex().items_center().justify_between().px(px(16.))
            .gap(px(8.)).bg(theme::shell()).border_b_1().border_color(theme::edge())
            .child(div().flex().items_center().gap(px(3.)).min_w_0()
                .child(action_button("zoom-fit","适合",enabled,fit,AppAction::SetZoom(ZoomMode::Fit),self.on_action.clone()))
                .child(action_button("zoom-100","100%",enabled,actual,AppAction::SetZoom(ZoomMode::Actual),self.on_action.clone()))
                .child(action_button("zoom-200","200%",enabled,double,AppAction::SetZoom(ZoomMode::Percent(200.)),self.on_action.clone()))
                .when_some(interaction::zoom_readout(&self.snapshot.zoom),|row,label|row.child(div().flex_shrink_0().px(px(4.)).text_size(px(11.)).text_color(theme::muted()).child(label))))
            .child(div().flex().items_center().gap(px(6.))
                .child(action_button("undo","撤销",self.snapshot.can_undo&&!self.snapshot.busy&&!self.snapshot.crop_editing,false,AppAction::Undo,self.on_action.clone()))
                .child(action_button("redo","重做",self.snapshot.can_redo&&!self.snapshot.busy&&!self.snapshot.crop_editing,false,AppAction::Redo,self.on_action.clone()))
                .child(action_button("compare","分屏对比",enabled,self.snapshot.compare,AppAction::ToggleCompare,self.on_action.clone()))
                .child(action_button("reset-photo","重置",self.enabled()&&!self.snapshot.crop_editing,false,AppAction::Reset,self.on_action.clone())))
    }

    fn canvas_area(&self,window:&Window,cx:&mut Context<Self>,pending:Option<PendingZoom>)->impl IntoElement + use<> {
        let mut area=div().id("photo-area").relative().flex().flex_col().flex_1().min_w_0().min_h_0().bg(theme::canvas());
        let mut viewport=div().id("preview-pan").flex().flex_1().min_w_0().min_h_0().overflow_scroll().track_scroll(&self.preview_scroll).p(px(theme::PREVIEW_PADDING));
        if let (Some(preview),Some((width,height)))=(self.snapshot.preview.clone(),self.snapshot.image_size) {
            let chrome=self.chrome_space();
            let (w,h)=display_size(width,height,self.snapshot.zoom.clone(),window.scale_factor(),f32::from(window.viewport_size().width),f32::from(window.viewport_size().height),chrome);
            let content_viewport=self.content_viewport(window);
            if let Some(pending)=pending {
                let new_frame=interaction::zoom_frame_size((w,h),content_viewport);
                let offset=interaction::zoom_scroll_offset(ZoomFrame{old_frame:pending.old_frame,new_frame,viewport:pending.viewport,old_offset:pending.old_offset});
                self.preview_scroll.set_offset(point(px(offset.0),px(offset.1)));
            }
            let frame_size=interaction::zoom_frame_size((w,h),content_viewport);
            let bounds=Rc::new(Cell::new(Bounds::<Pixels>::default()));let measure=bounds.clone();
            let mut photo=div().id("photo-stack").relative().overflow_hidden().flex_shrink_0().w(px(w)).h(px(h))
                .child(img(ImageSource::Render(preview)).absolute().size_full().object_fit(ObjectFit::Fill));
            if let Some(original)=self.snapshot.original_preview.clone(){
                if self.snapshot.peeking {
                    photo=photo.child(img(ImageSource::Render(original)).absolute().size_full().object_fit(ObjectFit::Fill));
                }else if self.snapshot.compare{
                    let split=w*self.snapshot.split/100.;
                    photo=photo.child(div().absolute().left(px(0.)).top(px(0.)).w(px(split)).h(px(h)).overflow_hidden()
                        .child(img(ImageSource::Render(original)).w(px(w)).h(px(h)).flex_shrink_0().object_fit(ObjectFit::Fill)))
                        .child(div().absolute().left(px(split-1.)).top(px(0.)).w(px(2.)).h(px(h)).bg(theme::paper()))
                        .child(div().absolute().left(px(12.)).top(px(12.)).text_size(px(11.)).text_color(theme::text()).bg(theme::well()).rounded(px(4.)).px(px(8.)).py(px(4.)).child("原图"));
                }
            }
            let mut overlay=self.snapshot.clone();if self.editing_region{overlay.settings.skin_region=self.region_draft;}
            let geometry=self.visible_geometry();let crop=self.tools.crop;let mask=self.tools.mask_draft.clone();
            photo=photo.child(canvas(move|b,_,_|{measure.set(b);},move|b,_,window,_|tools::paint_overlay(&overlay,geometry,crop,mask.clone(),b,window)).absolute().size_full());
            let down_bounds=bounds.clone();let move_bounds=bounds.clone();
            photo=photo.on_mouse_down(MouseButton::Left,cx.listener(move|this,event:&gpui::MouseDownEvent,window,cx|{
                window.focus(&this.focus);
                let p=normalized_point(event.position,down_bounds.get());
                if !this.enabled(){return;}
                if this.tool_down(p,window,cx){this.drag_mode=DragMode::Tool;}
                else if this.editing_region {let p=this.source_point(p);this.drag_mode=DragMode::Region;this.region_start=Some(p);this.region_draft=Some(region_from_points(p,p));cx.notify();}
                else if this.snapshot.compare {this.drag_mode=DragMode::Split;this.dispatch(AppAction::SetSplit(p[0]*100.),window,cx);}
            })).on_mouse_move(cx.listener(move|this,event:&gpui::MouseMoveEvent,window,cx|{
                let p=normalized_point(event.position,move_bounds.get());
                if !event.dragging(){if this.sample_preview(p){cx.notify();}return;}
                match this.drag_mode {DragMode::Region=>{if let Some(start)=this.region_start{this.region_draft=Some(region_from_points(start,this.source_point(p)));cx.notify();}},
                    DragMode::Tool=>this.tool_move(p,cx),DragMode::Split=>this.dispatch(AppAction::SetSplit(p[0]*100.),window,cx),DragMode::None=>{}}
            })).on_mouse_up(MouseButton::Left,cx.listener(|this,_,w,cx|{if this.drag_mode==DragMode::Tool{this.tool_up(w,cx);}this.drag_mode=DragMode::None;cx.notify();}))
                .on_mouse_up_out(MouseButton::Left,cx.listener(|this,_,w,cx|{if this.drag_mode==DragMode::Tool{this.tool_up(w,cx);}this.drag_mode=DragMode::None;cx.notify();}));
            let frame=div().id("preview-zoom-surface").flex().flex_shrink_0().items_center().justify_center()
                .min_w_full().min_h_full().w(px(frame_size.0)).h(px(frame_size.1))
                .child(photo)
                .on_scroll_wheel(cx.listener(|this,event:&gpui::ScrollWheelEvent,window,cx|{
                    this.zoom_with_wheel(event,window,cx);
                    cx.stop_propagation();
                }));
            viewport=viewport.child(frame);
        }else{
            viewport=viewport.items_center().justify_center().child(div().flex().flex_col().items_center().gap(px(18.)).max_w(px(470.))
                .child(div().w(px(64.)).h(px(64.)).rounded(px(16.)).bg(theme::panel()).flex().items_center().justify_center()
                    .child(img(ImageSource::Image(branding_icon())).w(px(42.)).h(px(42.)).object_fit(ObjectFit::Contain)))
                .child(div().flex().flex_col().items_center().gap(px(8.))
                    .child(div().text_size(px(25.)).font_weight(FontWeight::SEMIBOLD).child("打开照片，开始调色"))
                    .child(div().text_size(px(12.)).text_color(theme::muted()).child("导入照片或打开工程，开始调整影调与颜色。")))
                .child(div().flex().items_center().gap(px(9.))
                    .child(action_button("empty-open","导入照片",!self.snapshot.busy,true,AppAction::OpenImage,self.on_action.clone()))
                    .child(action_button("empty-project","打开工程",!self.snapshot.busy,false,AppAction::OpenProject,self.on_action.clone())))
                .child(div().mt(px(4.)).text_size(px(11.)).text_color(theme::muted()).child("JPG、PNG、TIFF 与相机 RAW，也可拖入画布")));
        }
        area=area.child(viewport).child(self.task_status()).child(div().h(px(25.)).flex_shrink_0().flex().items_center().justify_between().gap(px(10.)).px(px(16.)).bg(theme::shell())
            .child(div().min_w_0().truncate().text_size(px(10.)).text_color(theme::muted()).child(self.sample_text()))
            .child(div().flex_shrink_0().text_size(px(10.)).text_color(theme::muted()).child("空格查看原图")));
        if let Some(error)=&self.snapshot.error{area=area.child(div().px(px(14.)).py(px(7.)).bg(theme::well()).text_xs().text_color(theme::error()).child(error.clone()));}
        area
    }
    fn filmstrip(&self,cx:&mut Context<Self>)->AnyElement {
        if !self.show_filmstrip {
            return self.restore_bar("restore-filmstrip","显示照片条",cx);
        }
        div().h(px(theme::FILMSTRIP_HEIGHT)).flex_shrink_0().w_full().flex().items_center().gap(px(12.)).px(px(16.)).bg(theme::shell()).border_t_1().border_color(theme::edge())
            .child(div().w(px(42.)).flex_shrink_0().flex().flex_col().items_center().gap(px(4.))
                .child(div().text_size(px(11.)).text_color(theme::muted()).child("照片"))
                .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(format!("{:02}",self.snapshot.photos.len())))
                .child(self.chrome_button("collapse-filmstrip","收起",cx.listener(|this,_,window,cx|{
                    window.focus(&this.focus);this.show_filmstrip=false;cx.notify();
                }))))
            .child(div().id("filmstrip-scroll").flex().flex_1().min_w_0().gap(px(10.)).overflow_x_scroll().track_scroll(&self.filmstrip_scroll)
                .children(self.snapshot.photos.iter().enumerate().map(|(index,photo)|{
                    let action=self.on_action.clone();let selected=index==self.snapshot.current_index;
                    div().id(SharedString::from(format!("photo-{index}"))).flex_shrink_0().w(px(110.)).h(px(86.)).p(px(4.)).flex().flex_col().gap(px(5.))
                        .border_1().border_color(if selected{theme::accent()}else{theme::edge()}).rounded(px(5.)).bg(if selected{theme::elevated()}else{theme::well()})
                        .cursor_pointer().hover(|s|s.bg(theme::elevated()))
                        .child(img(ImageSource::Render(photo.preview.clone())).w_full().h(px(56.)).object_fit(ObjectFit::Contain))
                        .child(div().text_size(px(10.)).truncate().text_color(if selected{theme::text()}else{theme::muted()}).child(format!("{}{}",if photo.adjusted{"● "}else{""},photo.name)))
                        .on_click(move|_,w,cx|action(AppAction::SelectPhoto(index),w,cx))
                }))).into_any_element()
    }

    fn inspector(&self,cx:&mut Context<Self>)->AnyElement {
        if !self.show_inspector {
            return self.restore_strip("restore-inspector","面板",false,cx);
        }
        let (title,description)=match self.snapshot.panel {
            "landscape"=>("调色","影调、曲线与颜色"),"portrait"=>("人像","局部肤色与细节修饰"),
            "styles"=>("风格","滤镜、3D LUT 与成片微调"),"crop"=>("构图","裁剪、旋转与拉直"),
            "masks"=>("局部调整","让调整落在需要的位置"),"project"=>("工程与输出","保存进度，导出成片"),_=> ("照片信息","查看文件与拍摄记录")};
        let heading=div().flex_shrink_0().px(px(INSET)).py(px(15.)).border_b_1().border_color(theme::edge())
            .child(div().flex().items_center().justify_between().gap(px(8.))
                .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(title))
                .child(self.chrome_button("collapse-inspector","收起",cx.listener(|this,_,window,cx|{
                    window.focus(&this.focus);this.show_inspector=false;cx.notify();
                }))))
            .child(div().mt(px(4.)).text_size(px(11.)).text_color(theme::muted()).child(description));
        let mut body=div().id("inspector-scroll").flex().flex_col().gap(px(10.)).p(px(INSET)).min_h_0().flex_1().overflow_y_scroll();
        match self.snapshot.panel {
            "landscape"=>{
                body=body.child(self.diagnostics_panel()).child(self.color_copy_buttons());
                body=body.child(self.group_header("base","基础调色",cx));
                if self.group_open("base"){
                body=body.child(div().text_xs().text_color(theme::muted()).child("点击数值输入，双击名称重置该项。"));
                for (key,label,min,max,step) in [
                    ("exposure","曝光",-3.,3.,0.05),("temperature","色温偏移",-100.,100.,1.),("tint","绿／洋红色调",-100.,100.,1.),
                    ("contrast","对比度",-100.,100.,1.),("highlights","高光",-100.,100.,1.),("shadows","阴影",-100.,100.,1.),
                    ("whites","白色色阶",-100.,100.,1.),("blacks","黑色色阶",-100.,100.,1.),("saturation","饱和度",-100.,100.,1.),
                    ("vibrance","自然饱和度",-100.,100.,1.),("clarity","清晰度",-100.,100.,1.)] {body=body.child(self.control(key,label,min,max,step,cx));}
                    body=body.child(self.reset_group_button("base",cx));
                }
                body=body.child(self.group_header("curves","曲线",cx));if self.group_open("curves"){body=body.child(self.curve_editor(cx)).child(self.reset_group_button("curves",cx));}
                body=body.child(self.group_header("hsl","八色 HSL",cx));if self.group_open("hsl"){body=body.child(self.hsl_editor(cx)).child(self.reset_group_button("hsl",cx));}
                body=body.child(self.group_header("grade","色彩分级",cx));if self.group_open("grade"){body=body.child(self.grade_editor(cx)).child(self.reset_group_button("grade",cx));}
                body=body.child(self.group_header("detail","细节",cx));if self.group_open("detail"){
                    for (k,l,min,max) in [("dehaze","去雾",-100.,100.),("sharpening","锐化",0.,100.),("denoise","亮度降噪",0.,100.)]{body=body.child(self.control(k,l,min,max,1.,cx));}
                    body=body.child(self.reset_group_button("detail",cx));
                }
            },
            "portrait"=>{
                body=body.child(section_title("皮肤区域"))
                    .child(div().text_xs().text_color(theme::muted()).child("先拖出皮肤区域，再调整。区域可使用方向键移动，Shift + 方向键调整大小。"));
                let choose=div().id("choose-region").px(px(10.)).py(px(8.)).border_1().border_color(theme::paper()).cursor_pointer()
                    .child(if self.editing_region{"取消框选"}else{"框选皮肤区域"})
                    .on_click(cx.listener(|this,_,window,cx|{if this.snapshot.busy{return;}window.focus(&this.focus);this.editing_region=!this.editing_region;
                        this.region_draft=this.snapshot.settings.skin_region;this.region_start=None;this.drag_mode=DragMode::None;cx.notify();}));
                body=body.child(choose);
                if self.editing_region {
                    body=body.child(div().id("confirm-region").px(px(10.)).py(px(8.)).bg(theme::well()).border_1().border_color(theme::edge()).cursor_pointer().child("确认区域（Enter）")
                        .on_click(cx.listener(|this,_,w,cx|this.commit_region(w,cx))));
                }
                body=body.child(action_button("clear-region","清除区域",self.enabled(),false,AppAction::SetSkinRegion(None),self.on_action.clone()));
                for(k,l)in[("blemishes","弱化瑕疵"),("smoothing","平滑皮肤"),("skinTone","均匀肤色"),("texture","保留纹理")]{
                    let value=self.snapshot.settings.value(k).unwrap_or(0.);
                    body=body.child(self.numeric_control(k,l,value,0.,100.,1.,0.,self.enabled()&&self.snapshot.settings.skin_region.is_some(),InputTarget::Global(k.into()),cx));
                }body=body.child(self.preset_buttons(true));
            },
            "styles"=>{
                body=body.child(self.preset_buttons(false)).child(section_title("sRGB 3D LUT"))
                    .child(div().text_xs().text_color(theme::muted()).child("选择滤镜后，可到“调色”继续微调。"))
                    .child(div().text_xs().text_color(theme::muted()).child("基础调色 → 应用风格 → 成片微调"))
                    .child(action_button("import-lut","导入 .cube",!self.snapshot.busy,false,AppAction::ImportLut,self.on_action.clone()))
                    .child(action_button("no-lut","停用 LUT",self.enabled(),self.snapshot.selected_lut.is_none(),AppAction::SelectLut(None),self.on_action.clone()))
                    .child(self.control("lutStrength","LUT 强度",0.,100.,1.,cx));
                for (index,path) in self.snapshot.luts.iter().enumerate(){
                    let selected=self.snapshot.selected_lut.as_ref()==Some(path);let favorite=self.snapshot.favorites.contains(path);
                    let name=path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                    body=body.child(div().flex().items_center().gap(px(5.)).min_w_0()
                        .child(div().flex_1().min_w_0().child(self.style_row(format!("lut-{index}"),name,LUT_STYLE_LABEL,self.swatch_colors("lut",path),self.enabled(),selected,AppAction::SelectLut(Some(path.clone())))))
                        .child(action_button(format!("fav-{index}"),if favorite{"★"}else{"☆"},true,favorite,AppAction::ToggleFavorite(path.clone()),self.on_action.clone())));
                }
                body=body.child(section_title("成片微调 · 风格之后"));
                for (k,l,min,max,step) in [("postExposure","成片曝光",-3.,3.,0.05),("postTemperature","成片色温",-100.,100.,1.),("postContrast","成片对比度",-100.,100.,1.)]{body=body.child(self.control(k,l,min,max,step,cx));}
                body=body.child(self.reset_group_button("post",cx));
            },
            "crop"=>{body=body.child(self.crop_editor(cx));},
            "masks"=>{body=body.child(self.mask_editor(cx));},
            "project"=>{
                body=body.child(section_title("工程与交付"))
                    .child(div().text_xs().text_color(theme::muted()).child(format!("数据目录：{}",self.snapshot.data_root)))
                    .child(div().text_xs().text_color(theme::muted()).child(match self.snapshot.color_pipeline {
                        Some(chromora::core::ColorPipeline::LegacySrgb)=>"当前照片：旧版色彩兼容。旧工程首次保存将另存新版。",
                        Some(chromora::core::ColorPipeline::HqSrgb)=>"当前照片：0.6 高精度兼容。旧工程首次保存将另存新版。",
                        Some(chromora::core::ColorPipeline::AccurateSrgb)=>"当前照片：0.7 精确 JPEG 解码与高精度转换。工作与导出色域为 sRGB。",
                        Some(chromora::core::ColorPipeline::FloatSrgb)=>"当前照片：0.8 浮点细节处理。工作与导出色域为 sRGB。",
                        None=>"新导入照片使用高精度色彩转换。",
                    }))
                    .child(div().text_xs().text_color(theme::muted()).child("工程保存所有照片的参数和风格引用。使用文件名为工程命名。"))
                    .child(action_button("project-save-as","工程另存为…",self.enabled(),false,AppAction::SaveProjectAs,self.on_action.clone()));
                body=body.child(div().text_xs().text_color(theme::muted()).child(self.snapshot.project_path.as_ref().map(|p|p.to_string_lossy().into_owned()).unwrap_or_else(||"尚未保存工程".into())))
                    .child(section_title("导出设置"));
                for formats in [[ExportFormat::Jpeg,ExportFormat::Png,ExportFormat::Tiff].as_slice(),[ExportFormat::Psd,ExportFormat::Patchy].as_slice()]{
                    body=body.child(div().flex().gap(px(6.)).children(formats.iter().map(|&format|action_button(format!("format-{format:?}"),format.label(),!self.snapshot.busy,self.snapshot.export_format==format,AppAction::SetExportFormat(format),self.on_action.clone()))));
                }
                if self.snapshot.export_format==ExportFormat::Jpeg{
                    body=body.child(self.numeric_control("export-quality","JPG 质量",self.snapshot.export_options.jpeg_quality as f32,1.,100.,1.,96.,!self.snapshot.busy,InputTarget::ExportQuality,cx));
                }
                if matches!(self.snapshot.export_format,ExportFormat::Jpeg|ExportFormat::Png|ExportFormat::Tiff){
                    let edge=self.snapshot.export_options.max_long_edge;
                    body=body.child(div().flex().gap(px(6.))
                        .child(action_button("size-original","原尺寸",!self.snapshot.busy,edge.is_none(),AppAction::SetExportLongEdge(None),self.on_action.clone()))
                        .child(action_button("size-limited","限制长边",!self.snapshot.busy,edge.is_some(),AppAction::SetExportLongEdge(Some(edge.unwrap_or(2048))),self.on_action.clone())));
                    if let Some(edge)=edge{body=body.child(self.numeric_control("export-edge","长边像素",edge as f32,1.,30000.,1.,2048.,!self.snapshot.busy,InputTarget::ExportLongEdge,cx));}
                    body=body.child(div().text_xs().text_color(theme::muted()).child("构图后等比缩小，不放大小图。"));
                }else{body=body.child(div().text_xs().text_color(theme::muted()).child("PSD 与 Patchy 保留原尺寸。"));}
                body=body.child(action_button("export-selected","导出当前照片…",self.enabled(),true,AppAction::Export(self.snapshot.export_format),self.on_action.clone()))
                    .child(action_button("export-all",format!("导出全部 {} 张…",self.snapshot.photos.len()),self.enabled()&&self.snapshot.export_format!=ExportFormat::Patchy,false,AppAction::ExportAll,self.on_action.clone()));
                body=body.child(div().text_xs().text_color(theme::muted()).child("PNG 为 16 位无损输出。PSD 包含 Original 和 Edited 两个像素图层；调色参数及撤销历史保存在工程。Patchy 通过 16 位 TIFF 继续精修。"));
            },
            _=>{body=body.child(photo_info_panel(self.snapshot.photo_info.as_ref(),self.snapshot.image_size,self.snapshot.image_size.is_some()));}
        }
        div().w(px(theme::INSPECTOR_WIDTH)).h_full().flex_shrink_0().flex().flex_col().min_h_0().bg(theme::panel()).border_l_1().border_color(theme::edge()).child(heading).child(body).into_any_element()
    }
    fn control(&self,key:&str,label:&str,min:f32,max:f32,step:f32,cx:&mut Context<Self>)->gpui::AnyElement {
        let default=if key=="lutStrength"{100.}else{0.};
        self.numeric_control(key,label,self.snapshot.settings.value(key).unwrap_or(default),min,max,step,default,self.enabled(),InputTarget::Global(key.into()),cx)
    }
    fn preset_buttons(&self,portrait:bool)->impl IntoElement + use<> {
        let mut body=div().flex().flex_col().gap(px(8.)).pb(px(12.)).border_b_1().border_color(theme::edge())
            .child(div().flex().items_center().gap(px(6.))
            .child(div().flex_1().text_size(px(11.)).text_color(theme::muted()).child(if portrait{"个人预设"}else{"滤镜预设"}))
            .when(portrait,|row|row.child(action_button("browse-presets",if self.snapshot.presets_open{"收起"}else{"浏览"},!self.snapshot.busy,self.snapshot.presets_open,AppAction::TogglePresets,self.on_action.clone())))
            .child(action_button(if portrait{"open-portrait-preset"}else{"open-color-preset"},"打开…",self.enabled(),false,AppAction::OpenPreset(portrait),self.on_action.clone()))
            .child(action_button(if portrait{"save-portrait-preset"}else{"save-color-preset"},"保存…",self.enabled(),false,AppAction::SavePreset(portrait),self.on_action.clone())));
        if !portrait {
            body=body.child(div().text_xs().text_color(theme::muted()).child(APPROXIMATE_STYLE_NOTE));
        }
        if !portrait||self.snapshot.presets_open{
            let mut entries=self.snapshot.presets.iter().filter(|p|p.portrait==portrait).collect::<Vec<_>>();
            entries.sort_by_key(|entry|self.snapshot.recent_presets.iter().position(|p|p==&entry.path).unwrap_or(usize::MAX));
            let mut list=div().id(if portrait{"portrait-preset-list"}else{"preset-list"}).max_h(px(if portrait{180.}else{320.})).overflow_y_scroll().flex().flex_col().gap(px(5.));
            if entries.is_empty(){list=list.child(div().text_xs().text_color(theme::muted()).child("此分组暂无预设，可用打开或保存添加。"));}
            for entry in entries{
                let recent=self.snapshot.recent_presets.contains(&entry.path);
                let name=format!("{}{}",if recent{"最近 · "}else{""},entry.name);
                if portrait {
                    list=list.child(action_button(SharedString::from(format!("preset-{}",entry.path.display())),name,self.enabled(),false,AppAction::ApplyPreset(entry.path.clone(),portrait),self.on_action.clone()));
                } else {
                    list=list.child(self.style_row(format!("preset-{}",entry.path.display()),name,PARAMETER_STYLE_LABEL,self.swatch_colors("preset",&entry.path),self.enabled(),false,AppAction::ApplyPreset(entry.path.clone(),portrait)));
                }
            }
            body=body.child(list).child(action_button("refresh-presets","刷新预设目录",!self.snapshot.busy,false,AppAction::RefreshPresets,self.on_action.clone()));
        }
        if let Some(name)=&self.snapshot.last_preset{body=body.child(div().text_xs().text_color(theme::muted()).child(format!("最近应用：{name}")));}
        body
    }
    fn color_copy_buttons(&self)->impl IntoElement + use<> {
        div().flex().flex_col().gap(px(8.)).pb(px(12.)).border_b_1().border_color(theme::edge())
            .child(div().flex().gap(px(6.))
            .child(action_button("copy-color","复制调色",self.enabled(),false,AppAction::CopyColor,self.on_action.clone()))
            .child(action_button("paste-color","粘贴调色",self.enabled()&&self.snapshot.can_paste_color,false,AppAction::PasteColor,self.on_action.clone())))
            .child(div().text_size(px(10.)).text_color(theme::faint()).child("粘贴保留曝光、白平衡、风格与局部区域。"))
    }

    fn hsl_editor(&self,cx:&mut Context<Self>)->impl IntoElement + use<> {
        let mut choices=div().flex().flex_wrap().gap(px(5.));
        for(index,(_,label))in HSL_BANDS.iter().enumerate(){choices=choices.child(div().id(SharedString::from(format!("hsl-{index}"))).px(px(7.)).py(px(5.)).rounded(px(3.)).border_1().border_color(if index==self.hsl_band{theme::paper()}else{theme::edge()}).cursor_pointer().child(*label)
            .on_click(cx.listener(move|this,_,w,cx|{if !this.commit_pending_input(w,cx){return;}w.focus(&this.focus);this.hsl_band=index;cx.notify();})));}
        let band=HSL_BANDS[self.hsl_band].0;
        div().flex().flex_col().gap(px(10.)).child(choices)
            .child(self.control(&format!("{band}Hue"),"色相",-100.,100.,1.,cx))
            .child(self.control(&format!("{band}Sat"),"饱和度",-100.,100.,1.,cx))
            .child(self.control(&format!("{band}Lum"),"明亮度",-100.,100.,1.,cx))
    }
    fn grade_editor(&self,cx:&mut Context<Self>)->impl IntoElement + use<> {
        let bands=[("shadow","阴影"),("midtone","中间调"),("highlight","高光")];
        let mut choices=div().flex().gap(px(5.));
        for(index,(_,label))in bands.iter().enumerate(){choices=choices.child(div().id(SharedString::from(format!("grade-{index}"))).px(px(7.)).py(px(5.)).border_1().border_color(if index==self.grade_band{theme::paper()}else{theme::edge()}).cursor_pointer().child(*label)
            .on_click(cx.listener(move|this,_,w,cx|{if !this.commit_pending_input(w,cx){return;}w.focus(&this.focus);this.grade_band=index;cx.notify();})));}
        let band=bands[self.grade_band].0;
        div().flex().flex_col().gap(px(10.)).child(choices)
            .child(self.control(&format!("{band}Hue"),"色相",0.,360.,1.,cx))
            .child(self.control(&format!("{band}Sat"),"饱和度",0.,100.,1.,cx))
            .child(self.control("colorBalance","分级平衡",-100.,100.,1.,cx))
    }
    fn curve_points(&self)->Vec<[f32;2]>{self.snapshot.settings.curves.get(&self.curve_channel).cloned().unwrap_or_else(||vec![[0.,0.],[1.,1.]])}
    fn curve_editor(&self,cx:&mut Context<Self>)->impl IntoElement + use<> {
        let mut channels=div().flex().gap(px(5.));
        for (channel,label)in[("rgb","RGB"),("red","红"),("green","绿"),("blue","蓝")]{let name=channel.to_owned();channels=channels.child(div().id(SharedString::from(format!("curve-{channel}"))).px(px(8.)).py(px(5.)).border_1().border_color(if self.curve_channel==channel{theme::paper()}else{theme::edge()}).cursor_pointer().child(label)
            .on_click(cx.listener(move|this,_,_,cx|{this.curve_channel=name.clone();this.curve_point=0;cx.notify();})));}
        let bounds=Rc::new(Cell::new(Bounds::<Pixels>::default()));let measure=bounds.clone();let down=bounds.clone();let movement=bounds.clone();
        let points=self.curve_points();let selected=self.curve_point.min(points.len()-1);let painted=points.clone();
        let graph=div().id("curve-graph").w_full().h(px(220.)).bg(theme::well()).border_1().border_color(theme::edge()).cursor_crosshair()
            .child(canvas(move|b,_,_|{measure.set(b);},move|b,_,window,_|{
                let map=|p:[f32;2]|point(b.origin.x+px(p[0]*f32::from(b.size.width)),b.origin.y+px((1.-p[1])*f32::from(b.size.height)));
                for n in 0..=4 {let t=n as f32/4.;paint_line(window,&[map([t,0.]),map([t,1.])],theme::edge());paint_line(window,&[map([0.,t]),map([1.,t])],theme::edge());}
                paint_line(window,&painted.iter().copied().map(map).collect::<Vec<_>>(),theme::paper());
                for (i,p)in painted.iter().enumerate(){let at=map(*p);window.paint_quad(gpui::fill(Bounds::new(point(at.x-px(4.),at.y-px(4.)),gpui::size(px(8.),px(8.))),if i==selected{theme::paper()}else{theme::text()}));}
            }).size_full())
            .on_mouse_down(MouseButton::Left,cx.listener(move|this,event:&gpui::MouseDownEvent,w,cx|{
                if !this.enabled(){return;}w.focus(&this.focus);this.dispatch(AppAction::BeginEdit,w,cx);this.curve_dragging=true;let p=normalized_point(event.position,down.get());let p=[p[0],1.-p[1]];
                let mut points=this.curve_points();let nearest=points.iter().enumerate().min_by(|(_,a),(_,b)|distance(**a,p).total_cmp(&distance(**b,p))).map(|(i,_)|i).unwrap_or(0);
                if distance(points[nearest],p)<0.0016 {this.curve_point=nearest;}
                else if points.len()<12 {let insert=points.iter().position(|q|q[0]>p[0]).unwrap_or(points.len());
                    if insert>0&&insert<points.len()&&p[0]-points[insert-1][0]>=0.01&&points[insert][0]-p[0]>=0.01 {points.insert(insert,p);this.curve_point=insert;this.dispatch(AppAction::SetCurve(this.curve_channel.clone(),points),w,cx);}}
                cx.notify();
            })).on_mouse_move(cx.listener(move|this,event:&gpui::MouseMoveEvent,w,cx|{
                if !event.dragging()||!this.curve_dragging||!this.enabled(){return;}
                let p=normalized_point(event.position,movement.get());let mut points=this.curve_points();let i=this.curve_point.min(points.len()-1);
                move_curve_point(&mut points,i,[p[0],1.-p[1]]);this.dispatch(AppAction::SetCurve(this.curve_channel.clone(),points),w,cx);
            })).on_mouse_up(MouseButton::Left,cx.listener(|this,_,w,cx|{if this.curve_dragging{this.dispatch(AppAction::EndEdit,w,cx);}this.curve_dragging=false;}))
            .on_mouse_up_out(MouseButton::Left,cx.listener(|this,_,w,cx|{if this.curve_dragging{this.dispatch(AppAction::EndEdit,w,cx);}this.curve_dragging=false;}));
        div().flex().flex_col().gap(px(10.)).child(channels).child(graph)
            .child(div().text_xs().text_color(theme::muted()).child("点击添加控制点，拖动调整。每通道最多 12 点。"))
            .child(div().flex().gap(px(5.))
                .child(div().id("curve-remove").px(px(9.)).py(px(5.)).border_1().border_color(theme::edge()).cursor_pointer().child("删除点")
                    .on_click(cx.listener(|this,_,w,cx|{let mut points=this.curve_points();let i=this.curve_point;if i>0&&i+1<points.len(){points.remove(i);this.curve_point=i-1;this.dispatch(AppAction::SetCurve(this.curve_channel.clone(),points),w,cx);}})))
                .child(action_button("curve-reset","重置通道",self.enabled(),false,AppAction::SetCurve(self.curve_channel.clone(),vec![[0.,0.],[1.,1.]]),self.on_action.clone())))
    }
    fn chrome_space(&self)->ChromeSpace {
        ChromeSpace{
            rail:if self.show_rail{theme::TOOL_RAIL_WIDTH}else{theme::COLLAPSED_STRIP},
            inspector:if self.show_inspector{theme::INSPECTOR_WIDTH}else{theme::COLLAPSED_STRIP},
            filmstrip:if self.show_filmstrip{theme::FILMSTRIP_HEIGHT}else{theme::COLLAPSED_STRIP},
        }
    }
    fn content_viewport(&self,window:&Window)->(f32,f32) {
        let measured=self.preview_scroll.bounds().size;
        let measured_w=f32::from(measured.width)-theme::PREVIEW_PADDING*2.;
        let measured_h=f32::from(measured.height)-theme::PREVIEW_PADDING*2.;
        if measured_w>8.&&measured_h>8. {return (measured_w,measured_h);}
        let chrome=self.chrome_space();
        let viewport=window.viewport_size();
        let w=(f32::from(viewport.width)-chrome.rail-chrome.inspector-theme::PREVIEW_PADDING*2.).max(1.);
        let h=(f32::from(viewport.height)-theme::TOP_BAR_HEIGHT-theme::CANVAS_HEADER_HEIGHT-chrome.filmstrip-53.-theme::PREVIEW_PADDING*2.).max(1.);
        (w,h)
    }
    fn zoom_with_wheel(&mut self,event:&gpui::ScrollWheelEvent,window:&mut Window,cx:&mut Context<Self>) {
        let Some((width,height))=self.snapshot.image_size else {return;};
        if self.snapshot.preview.is_none(){return;}
        let lines=match event.delta {
            gpui::ScrollDelta::Lines(delta)=>delta.y,
            gpui::ScrollDelta::Pixels(delta)=>f32::from(delta.y)/f32::from(window.line_height()).max(1.),
        };
        if !lines.is_finite()||lines==0.{return;}
        let chrome=self.chrome_space();
        let viewport=window.viewport_size();
        let fitted=display_size(width,height,ZoomMode::Fit,window.scale_factor(),f32::from(viewport.width),f32::from(viewport.height),chrome).0;
        let current=interaction::effective_zoom_percent(width,&self.snapshot.zoom,window.scale_factor(),fitted);
        let next=interaction::wheel_zoom_percent(current,lines);
        if (next-current).abs()<0.25 {return;}
        let (photo_w,photo_h)=display_size(width,height,self.snapshot.zoom.clone(),window.scale_factor(),f32::from(viewport.width),f32::from(viewport.height),chrome);
        let content=self.content_viewport(window);
        let offset=self.preview_scroll.offset();
        self.pending_zoom=Some(PendingZoom{
            old_frame:interaction::zoom_frame_size((photo_w,photo_h),content),
            old_offset:(f32::from(offset.x),f32::from(offset.y)),
            viewport:content,
        });
        let mode=if (next-100.).abs()<0.51 {ZoomMode::Actual}else{ZoomMode::Percent(next)};
        self.dispatch(AppAction::SetZoom(mode),window,cx);
    }
    fn swatch_colors(&self,kind:&str,path:&std::path::Path)->Vec<[u8;3]> {
        let modified=std::fs::metadata(path).and_then(|meta|meta.modified()).ok();
        let key=format!("{kind}:{}:{modified:?}",path.display());
        if let Some(colors)=self.swatch_cache.borrow().get(&key) {return colors.clone();}
        let colors=if kind=="lut"{interaction::lut_swatch(path)}else{interaction::preset_swatch(path)};
        let mut cache=self.swatch_cache.borrow_mut();
        if cache.len()>64 {cache.clear();}
        cache.insert(key,colors.clone());
        colors
    }
    fn style_row(&self,id:impl Into<SharedString>,name:String,kind:&'static str,colors:Vec<[u8;3]>,enabled:bool,selected:bool,action:AppAction)->AnyElement {
        let handler=self.on_action.clone();let key_handler=handler.clone();let key_action=action.clone();
        let mut row=div().id(id.into()).w_full().min_h(px(52.)).flex().items_center().gap(px(8.)).px(px(6.)).py(px(5.))
            .rounded(px(5.)).border_1()
            .border_color(if selected&&enabled{theme::accent()}else{theme::edge()})
            .bg(if selected{theme::elevated()}else{theme::panel()})
            .child(swatch(colors))
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(3.))
                .child(div().min_w_0().truncate().text_size(px(12.)).text_color(if enabled{theme::text()}else{theme::faint()}).child(name))
                .child(div().flex().items_center()
                    .child(div().px(px(5.)).py(px(1.)).rounded(px(3.)).border_1().border_color(theme::edge()).text_size(px(10.)).text_color(theme::muted()).child(kind))));
        if enabled {
            row=row.tab_stop(true).cursor_pointer()
                .hover(|style|style.bg(theme::hover()).border_color(theme::faint()))
                .focus(|style|style.border_color(theme::accent()))
                .on_click(move|_,window,cx|handler(action.clone(),window,cx))
                .on_key_down(move|event,window,cx|{
                    if !event.is_held&&matches!(event.keystroke.key.as_str(),"enter"|"return"|"space"|" "){
                        cx.stop_propagation();key_handler(key_action.clone(),window,cx);
                    }
                });
        }
        row.into_any_element()
    }
    fn chrome_button(&self,id:&'static str,label:&'static str,listener:impl Fn(&gpui::ClickEvent,&mut Window,&mut App)+'static)->AnyElement {
        div().id(id).h(px(26.)).px(px(7.)).flex_shrink_0().flex().items_center().justify_center()
            .rounded(px(4.)).border_1().border_color(theme::edge()).bg(theme::panel())
            .text_size(px(11.)).text_color(theme::muted()).child(label)
            .tab_stop(true).cursor_pointer()
            .hover(|style|style.bg(theme::elevated()).text_color(theme::text()).border_color(theme::faint()))
            .focus(|style|style.border_color(theme::accent()))
            .on_click(listener)
            .into_any_element()
    }
    fn restore_strip(&self,id:&'static str,label:&'static str,rail:bool,cx:&mut Context<Self>)->AnyElement {
        div().id(id).w(px(theme::COLLAPSED_STRIP)).h_full().flex_shrink_0().flex().flex_col().items_center().justify_center()
            .bg(theme::shell()).when(rail,|strip|strip.border_r_1()).when(!rail,|strip|strip.border_l_1()).border_color(theme::edge())
            .child(self.chrome_button(if rail{"expand-rail"}else{"expand-inspector"},label,cx.listener(move|this,_,window,cx|{
                window.focus(&this.focus);
                if rail{this.show_rail=true;}else{this.show_inspector=true;}
                cx.notify();
            })))
            .into_any_element()
    }
    fn restore_bar(&self,id:&'static str,label:&'static str,cx:&mut Context<Self>)->AnyElement {
        div().id(id).h(px(theme::COLLAPSED_STRIP)).w_full().flex_shrink_0().flex().items_center().justify_center()
            .bg(theme::shell()).border_t_1().border_color(theme::edge())
            .child(self.chrome_button("expand-filmstrip",label,cx.listener(|this,_,window,cx|{
                window.focus(&this.focus);this.show_filmstrip=true;cx.notify();
            })))
            .into_any_element()
    }
    fn commit_region(&mut self,w:&mut Window,cx:&mut Context<Self>){
        if let Some(region)=self.region_draft {if region.width>=0.02&&region.height>=0.02{
            self.editing_region=false;self.dispatch(AppAction::SetSkinRegion(Some(region)),w,cx);cx.notify();}}
    }
}
impl Render for Workbench {
    fn render(&mut self,window:&mut Window,cx:&mut Context<Self>)->impl IntoElement {
        let pending=self.pending_zoom.take();
        let body=self.canvas_area(window,cx,pending);let inspector=self.inspector(cx);
        div().id("chromora-workbench").track_focus(&self.focus).size_full().flex().flex_col().font_family("Microsoft YaHei UI").text_size(px(13.)).text_color(theme::text()).bg(theme::shell())
            .child(self.top_bar()).child(div().flex().flex_1().min_h_0().min_w_0().child(self.tool_rail(cx))
                .child(div().flex().flex_1().min_h_0().min_w_0().flex_col().child(self.canvas_header()).child(body).child(self.filmstrip(cx))).child(inspector))
            .on_drop(cx.listener(|this,paths:&ExternalPaths,w,cx|{this.dispatch(AppAction::ImportPaths(paths.paths().to_vec()),w,cx);}))
            .on_key_down(cx.listener(|this,event:&gpui::KeyDownEvent,w,cx|{
                let key=event.keystroke.key.to_ascii_lowercase();
                if this.tools.input.is_some(){return;}
                if this.snapshot.crop_editing{
                    if key=="escape"{this.finish_crop(false,w,cx);cx.stop_propagation();return;}
                    if key=="enter"{this.finish_crop(true,w,cx);cx.stop_propagation();return;}
                }
                if key=="escape"&&(this.tools.mask_draw.is_some()||this.tools.mask_draft.is_some()){
                    if this.tools.mask_draft.is_some(){this.dispatch(AppAction::CancelEdit,w,cx);}
                    this.tools.mask_draw=None;this.tools.mask_draft=None;this.tools.mask_start=None;this.tools.mask_handle=None;this.drag_mode=DragMode::None;cx.notify();cx.stop_propagation();return;
                }
                if event.keystroke.modifiers.control&&!event.is_held&&!this.snapshot.busy&&!this.snapshot.crop_editing{
                    if key=="z"{let redo=event.keystroke.modifiers.shift;if (redo&&this.snapshot.can_redo)||(!redo&&this.snapshot.can_undo){this.dispatch(if redo{AppAction::Redo}else{AppAction::Undo},w,cx);}cx.stop_propagation();return;}
                    if key=="y"{if this.snapshot.can_redo{this.dispatch(AppAction::Redo,w,cx);}cx.stop_propagation();return;}
                }
                if this.editing_region {
                    if key=="escape"{this.editing_region=false;this.region_draft=None;cx.notify();return;}
                    if key=="enter"{this.commit_region(w,cx);return;}
                    let mut r=this.region_draft.unwrap_or(SkinRegion{x:0.28,y:0.24,width:0.44,height:0.52});
                    let step=0.01;let resize=event.keystroke.modifiers.shift;
                    match key.as_str(){"left"=>if resize{r.width=(r.width-step).max(0.02)}else{r.x=(r.x-step).max(0.)},
                        "right"=>if resize{r.width=(r.width+step).min(1.-r.x)}else{r.x=(r.x+step).min(1.-r.width)},
                        "up"=>if resize{r.height=(r.height-step).max(0.02)}else{r.y=(r.y-step).max(0.)},
                        "down"=>if resize{r.height=(r.height+step).min(1.-r.y)}else{r.y=(r.y+step).min(1.-r.height)},_=>return}
                    this.region_draft=Some(r);cx.notify();return;
                }
                // Skin-region arrows return above. Plain left/right steps the open set.
                let plain=!event.keystroke.modifiers.control&&!event.keystroke.modifiers.alt&&!event.keystroke.modifiers.shift&&!event.keystroke.modifiers.platform&&!event.keystroke.modifiers.function;
                let editing_mask=this.tools.mask_draw.is_some()||this.tools.mask_draft.is_some();
                if plain&&!event.is_held&&!this.snapshot.crop_editing&&!editing_mask&&!this.snapshot.busy {
                    let delta=match key.as_str(){"left"=>Some(-1),"right"=>Some(1),_=>None};
                    if let Some(delta)=delta {
                        if let Some(next)=interaction::step_photo_index(this.snapshot.current_index,this.snapshot.photos.len(),delta) {
                            this.filmstrip_scroll.scroll_to_item(next);
                            this.dispatch(AppAction::SelectPhoto(next),w,cx);
                        }
                        cx.stop_propagation();
                        return;
                    }
                }
                if matches!(key.as_str(),"space"|" ")&&!event.keystroke.modifiers.control&&!event.keystroke.modifiers.alt&&!this.snapshot.busy {
                    this.dispatch(AppAction::PeekOriginal(true),w,cx);cx.stop_propagation();
                }
            })).on_key_up(cx.listener(|this,event:&gpui::KeyUpEvent,w,cx|{if matches!(event.keystroke.key.as_str(),"space"|" "){this.dispatch(AppAction::PeekOriginal(false),w,cx);}}))
    }
}
fn action_button(id:impl Into<SharedString>,label:impl Into<SharedString>,enabled:bool,selected:bool,action:AppAction,handler:ActionHandler)->impl IntoElement {
    let primary=selected&&matches!(&action,AppAction::Export(_)|AppAction::OpenImage);
    let mut button=div().id(id.into()).h(px(32.)).px(px(11.)).flex_shrink_0().flex().items_center().justify_center().rounded(px(5.)).border_1()
        .border_color(if primary&&enabled{theme::accent()}else if selected&&enabled{theme::faint()}else{theme::edge()})
        .bg(if primary&&enabled{theme::accent()}else if selected{theme::elevated()}else{theme::panel()})
        .text_size(px(11.)).font_weight(if primary{FontWeight::SEMIBOLD}else{FontWeight::NORMAL})
        .text_color(if !enabled{theme::faint()}else if primary{theme::canvas()}else if selected{theme::accent()}else{theme::text()}).child(div().min_w_0().truncate().child(label.into()));
    if enabled {let click=handler.clone();let key_action=action.clone();button=button.tab_stop(true).cursor_pointer()
        .hover(move|s|s.bg(if primary{theme::accent_hover()}else{theme::hover()}).border_color(if primary{theme::accent_hover()}else{theme::faint()}))
        .focus(|s|s.border_color(theme::accent())).on_click(move|_,w,cx|click(action.clone(),w,cx))
        .on_key_down(move|event,w,cx|{if !event.is_held&&matches!(event.keystroke.key.as_str(),"enter"|"return"|"space"|" "){cx.stop_propagation();handler(key_action.clone(),w,cx);}});}
    button
}
fn section_title(label:&'static str)->gpui::Div{div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::text()).child(label)}
fn tool_icon(kind:&'static str,selected:bool)->impl IntoElement {
    canvas(move|_,_,_|(),move|bounds,_,window,_|{
        let color=if selected{theme::accent()}else{theme::muted()};
        let line=|window:&mut Window,points:&[[f32;2]]|{
            paint_line(window,&points.iter().map(|p|point(bounds.origin.x+px(p[0]),bounds.origin.y+px(p[1]))).collect::<Vec<_>>(),color);
        };
        let circle=|window:&mut Window,x:f32,y:f32,r:f32|{
            let points=(0..=24).map(|i|{let t=i as f32/24.*std::f32::consts::TAU;[x+r*t.cos(),y+r*t.sin()]}).collect::<Vec<_>>();line(window,&points);
        };
        match kind {
            "landscape"=>{for(y,x)in[(5.,8.),(12.,16.),(19.,10.)]{line(window,&[[3.,y],[21.,y]]);line(window,&[[x,y-3.],[x,y+3.]]);}},
            "portrait"=>{circle(window,12.,7.,4.);line(window,&[[4.,22.],[5.,18.],[8.,15.],[16.,15.],[19.,18.],[20.,22.]]);},
            "styles"=>{line(window,&[[12.,3.],[21.,10.],[12.,17.],[3.,10.],[12.,3.]]);line(window,&[[3.,15.],[12.,22.],[21.,15.]]);},
            "crop"=>{line(window,&[[7.,2.],[7.,18.],[23.,18.]]);line(window,&[[2.,7.],[18.,7.],[18.,23.]]);},
            "masks"=>{circle(window,12.,12.,9.);line(window,&[[6.,17.],[17.,6.]]);line(window,&[[10.,20.],[20.,10.]]);},
            "project"=>{line(window,&[[3.,7.],[10.,7.],[12.,4.],[21.,4.],[21.,21.],[3.,21.],[3.,7.]]);line(window,&[[7.,12.],[17.,12.]]);line(window,&[[7.,16.],[14.,16.]]);},
            _=>{circle(window,12.,12.,9.);line(window,&[[12.,11.],[12.,18.]]);circle(window,12.,7.,0.7);}
        }
    }).w(px(24.)).h(px(24.))
}
fn slider_drag_flag(states:&RefCell<BTreeMap<String,Rc<Cell<bool>>>>,key:&str)->Rc<Cell<bool>>{
    states.borrow_mut().entry(key.to_owned()).or_insert_with(||Rc::new(Cell::new(false))).clone()
}
fn paint_line(window:&mut Window,points:&[Point<Pixels>],color:gpui::Rgba){let mut path=gpui::PathBuilder::stroke(px(1.5));for(i,p)in points.iter().enumerate(){if i==0{path.move_to(*p)}else{path.line_to(*p)}}if let Ok(path)=path.build(){window.paint_path(path,color);}}
fn normalized_point(p:Point<Pixels>,bounds:Bounds<Pixels>)->[f32;2]{[((f32::from(p.x)-f32::from(bounds.origin.x))/f32::from(bounds.size.width).max(1.)).clamp(0.,1.),((f32::from(p.y)-f32::from(bounds.origin.y))/f32::from(bounds.size.height).max(1.)).clamp(0.,1.)]}
fn region_from_points(a:[f32;2],b:[f32;2])->SkinRegion{SkinRegion{x:a[0].min(b[0]),y:a[1].min(b[1]),width:(a[0]-b[0]).abs(),height:(a[1]-b[1]).abs()}}
fn distance(a:[f32;2],b:[f32;2])->f32{(a[0]-b[0]).powi(2)+(a[1]-b[1]).powi(2)}
fn move_curve_point(points:&mut [[f32;2]],index:usize,position:[f32;2]){
    if index>0&&index+1<points.len(){
        let lower=points[index-1][0]+0.01;let upper=points[index+1][0]-0.01;
        // Imported curves can have less horizontal space than a new UI point needs.
        if lower<=upper {points[index][0]=position[0].clamp(lower,upper);}
    }
    points[index][1]=position[1].clamp(0.,1.);
}
fn preview_logical_size(width:u32,height:u32,zoom:ZoomMode,dpi:f32)->Option<(f32,f32)>{let factor=match zoom{ZoomMode::Fit=>return None,ZoomMode::Actual=>1.,ZoomMode::Percent(p)=>p/100.};Some((width as f32*factor/dpi.max(1.),height as f32*factor/dpi.max(1.)))}
fn display_size(width:u32,height:u32,zoom:ZoomMode,dpi:f32,viewport_width:f32,viewport_height:f32,chrome:ChromeSpace)->(f32,f32){
    if let Some(size)=preview_logical_size(width,height,zoom,dpi){return size;}
    let available_w=(viewport_width-chrome.rail-chrome.inspector-theme::PREVIEW_PADDING*2.).max(1.);
    let available_h=(viewport_height-theme::TOP_BAR_HEIGHT-chrome.filmstrip-theme::CANVAS_HEADER_HEIGHT-96.).max(1.);
    let scale=(available_w/width.max(1) as f32).min(available_h/height.max(1) as f32);
    (width as f32*scale,height as f32*scale)
}
fn swatch(colors:Vec<[u8;3]>)->gpui::Div {
    div().w(px(58.)).h(px(40.)).flex_shrink_0().rounded(px(4.)).overflow_hidden().border_1().border_color(theme::edge()).bg(theme::well())
        .child(canvas(move|_,_,_|{},move|bounds,_,window,_|{
            if colors.is_empty(){return;}
            let count=colors.len() as f32;
            let width=f32::from(bounds.size.width);
            for (index,rgb) in colors.iter().enumerate() {
                let x=bounds.origin.x+px(width*index as f32/count);
                let color=gpui::rgb((u32::from(rgb[0])<<16)|(u32::from(rgb[1])<<8)|u32::from(rgb[2]));
                window.paint_quad(gpui::fill(Bounds::new(point(x,bounds.origin.y),gpui::size(px(width/count+0.5),bounds.size.height)),color));
            }
        }).size_full())
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn fixed_zoom_preserves_device_pixel_size_across_windows_dpi(){for dpi in [1.,1.25,1.5,2.]{let(w,h)=preview_logical_size(1600,1000,ZoomMode::Actual,dpi).unwrap();assert!((w*dpi-1600.).abs()<0.01);assert!((h*dpi-1000.).abs()<0.01);}}
    #[test]fn fit_grows_when_chrome_collapses(){
        let full=ChromeSpace{rail:theme::TOOL_RAIL_WIDTH,inspector:theme::INSPECTOR_WIDTH,filmstrip:theme::FILMSTRIP_HEIGHT};
        let slim=ChromeSpace{rail:theme::COLLAPSED_STRIP,inspector:theme::COLLAPSED_STRIP,filmstrip:theme::COLLAPSED_STRIP};
        let open=display_size(4000,3000,ZoomMode::Fit,1.,1024.,650.,full);
        let collapsed=display_size(4000,3000,ZoomMode::Fit,1.,1024.,650.,slim);
        assert!(collapsed.0>open.0&&collapsed.1>open.1);
    }
    #[test]fn drag_region_works_in_every_direction(){let r=region_from_points([0.8,0.7],[0.2,0.1]);assert!((r.x-0.2).abs()<1e-6);assert!((r.y-0.1).abs()<1e-6);assert!((r.width-0.6).abs()<1e-6);assert!((r.height-0.6).abs()<1e-6);}
    #[test]fn imported_close_curve_points_keep_x_and_allow_y_edits(){
        let mut points=[[0.,0.],[0.499,0.3],[0.5,0.5],[0.501,0.7],[1.,1.]];
        move_curve_point(&mut points,2,[0.8,0.9]);
        assert_eq!(points[2],[0.5,0.9]);
        let mut spaced=[[0.,0.],[0.5,0.5],[1.,1.]];
        move_curve_point(&mut spaced,1,[0.8,1.2]);assert_eq!(spaced[1],[0.8,1.]);
    }
    #[test]fn slider_drag_flags_survive_renders_without_activating_other_tracks(){
        let states=RefCell::new(BTreeMap::new());
        let first=slider_drag_flag(&states,"exposure");first.set(true);
        assert!(slider_drag_flag(&states,"exposure").get());
        assert!(!slider_drag_flag(&states,"contrast").get());
        first.set(false);assert!(!slider_drag_flag(&states,"exposure").get());
    }
}

