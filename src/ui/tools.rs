use super::*;
use gpui::{AnyElement, Entity};
use chromora::core::{GeometrySettings, LocalAdjustments, LocalMask, MaskShape, NormalizedRect, output_to_source, source_to_output, output_dimensions};
use crate::ui::inline_input::InlineInput;
use crate::app::InputTarget;

#[derive(Default)]
pub(super) struct ToolState {
    pub groups:BTreeMap<&'static str,bool>,
    pub input:Option<(String,Entity<InlineInput>)>,
    pub input_spec:Option<(InputTarget,Option<(f32,f32,f32)>)>,
    pub input_generation:u64,
    pub committed_inputs:Vec<AppAction>,
    pub crop:Option<NormalizedRect>,
    pub ratio:Option<f32>,
    pub crop_anchor:Option<[f32;2]>,
    pub crop_before:Option<NormalizedRect>,
    pub mask_draw:Option<bool>,
    pub mask_start:Option<[f32;2]>,
    pub mask_draft:Option<(usize,LocalMask)>,
    pub mask_handle:Option<usize>,
}

impl Workbench {
    pub(super) fn visible_geometry(&self)->GeometrySettings {
        let mut g=self.snapshot.settings.geometry;
        if self.snapshot.crop_editing {g.crop=None;} g
    }
    pub(super) fn source_point(&self,p:[f32;2])->[f32;2]{
        let (w,h)=self.snapshot.source_size.unwrap_or((1,1));
        output_to_source(p,w,h,&self.visible_geometry()).map(|v|v.clamp(0.,1.))
    }
    pub(super) fn group_open(&self,key:&'static str)->bool{self.tools.groups.get(key).copied().unwrap_or(key=="base")}
    pub(super) fn group_header(&self,key:&'static str,label:&'static str,cx:&mut Context<Self>)->AnyElement{
        let open=self.group_open(key);
        div().id(SharedString::from(format!("group-{key}"))).w_full().flex().items_center().justify_between().min_h(px(34.)).px(px(2.)).py(px(5.))
            .border_b_1().border_color(theme::edge()).rounded(px(4.)).cursor_pointer().hover(|style|style.bg(theme::elevated()))
            .child(div().flex().items_center().gap(px(8.))
                .child(div().w(px(2.)).h(px(12.)).rounded(px(1.)).bg(if open{theme::accent()}else{theme::edge()}))
                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::text()).child(label)))
            .child(div().flex().items_center().gap(px(5.)).text_size(px(11.)).text_color(theme::faint())
                .child(if open{"收起"}else{"展开"}).child(if open{"−"}else{"+"}))
            .on_click(cx.listener(move|this,_,w,cx|{if !this.commit_pending_input(w,cx){return;}w.focus(&this.focus);this.tools.groups.insert(key,!open);cx.notify();})).into_any_element()
    }
    pub(super) fn reset_group_button(&self,key:&'static str,cx:&mut Context<Self>)->AnyElement{
        let enabled=self.enabled();
        div().id(SharedString::from(format!("reset-group-{key}"))).h(px(24.)).px(px(8.)).flex().items_center().justify_center()
            .rounded(px(4.)).border_1().border_color(theme::edge()).text_size(px(11.)).text_color(if enabled{theme::muted()}else{theme::faint()})
            .when(enabled,|button|button.cursor_pointer().hover(|style|style.bg(theme::elevated()).text_color(theme::text()))).child("重置本组")
            .on_click(cx.listener(move|this,_,w,cx|{
                if !this.enabled() || !this.commit_pending_input(w,cx){return;}
                this.dispatch(AppAction::BeginEdit,w,cx);
                let keys:Vec<&str>=match key {
                    "base"=>vec!["exposure","temperature","tint","contrast","highlights","shadows","whites","blacks","saturation","vibrance","clarity"],
                    "detail"=>vec!["dehaze","sharpening","denoise"],
                    "post"=>vec!["postExposure","postTemperature","postContrast"],
                    "grade"=>vec!["shadowHue","shadowSat","midtoneHue","midtoneSat","highlightHue","highlightSat","colorBalance"],
                    _=>vec![],
                };
                for k in keys{this.dispatch(AppAction::SetValue(k.into(),0.),w,cx);}
                if key=="hsl"{for (band,_)in HSL_BANDS {for suffix in ["Hue","Sat","Lum"]{this.dispatch(AppAction::SetValue(format!("{band}{suffix}"),0.),w,cx);}}}
                if key=="curves"{for channel in ["rgb","red","green","blue"]{this.dispatch(AppAction::SetCurve(channel.into(),vec![[0.,0.],[1.,1.]]),w,cx);}}
                this.dispatch(AppAction::EndEdit,w,cx);
            })).into_any_element()
    }
    fn action_for_value(&self,target:&InputTarget,value:f32)->AppAction{
        if matches!(target,InputTarget::Name(_)){unreachable!("names use text input");}
        AppAction::SetControl(target.clone(),value)
    }
    /// Validate and detach the draft before its action is applied by AppRoot.
    /// The caller can then apply this action before save/export/navigation, in that order.
    pub fn take_pending_input(&mut self,w:&mut Window,cx:&mut Context<Self>)->Result<Option<AppAction>,()> {
        let Some((_,input))=self.tools.input.clone() else {self.tools.input_spec=None;return Ok(None);};
        let text=input.read(cx).text().to_owned();
        let action=(||->Result<AppAction,String>{
            let (target,range)=self.tools.input_spec.as_ref().ok_or("输入目标已失效")?;
            if let InputTarget::Name(index)=target {
                let name=parse_mask_name(&text)?;
                if self.snapshot.settings.masks.get(*index).is_none(){return Err("蒙版已不存在".into());}
                Ok(AppAction::RenameMask(*index,name))
            }else{
                let (min,max,step)=range.ok_or("输入范围无效")?;
                Ok(self.action_for_value(target,parse_numeric_input(&text,min,max,step)?))
            }
        })();
        match action {
            Ok(action)=>{self.tools.input=None;self.tools.input_spec=None;w.focus(&self.focus);cx.notify();Ok(Some(action))},
            Err(error)=>{input.update(cx,|this,cx|this.show_error(error,cx));w.focus(&input.read(cx).focus);Err(())},
        }
    }
    pub fn commit_pending_input(&mut self,w:&mut Window,cx:&mut Context<Self>)->bool {
        match self.take_pending_input(w,cx) {
            Ok(Some(action))=>{self.tools.committed_inputs.push(action);self.dispatch(AppAction::FlushInputs,w,cx);true},Ok(None)=>true,Err(())=>false,
        }
    }
    pub fn take_committed_inputs(&mut self)->Vec<AppAction> {std::mem::take(&mut self.tools.committed_inputs)}
    fn start_input(&mut self,id:String,target:InputTarget,value:String,range:Option<(f32,f32,f32)>,w:&mut Window,cx:&mut Context<Self>){
        if !self.commit_pending_input(w,cx){return;}
        let weak=cx.weak_entity();let cancel_weak=weak.clone();
        self.tools.input_generation=self.tools.input_generation.wrapping_add(1);
        let generation=self.tools.input_generation;
        let submit=Rc::new(move|_text:String,w:&mut Window,app:&mut App|->Result<(),String>{
            weak.update(app,|this,cx|{
                if this.tools.input_generation!=generation || this.tools.input.is_none(){return Ok(());}
                if this.commit_pending_input(w,cx){Ok(())}else{Err(String::new())}
            }).unwrap_or_else(|_|Err("编辑窗口已关闭".into()))
        });
        let cancel=Rc::new(move|w:&mut Window,app:&mut App|{let _=cancel_weak.update(app,|this,cx|{
            if this.tools.input_generation==generation {this.tools.input=None;this.tools.input_spec=None;w.focus(&this.focus);cx.notify();}
        });});
        let input=cx.new(|cx|InlineInput::new(value,submit,cancel,w,cx));
        w.focus(&input.read(cx).focus);self.tools.input=Some((id,input));self.tools.input_spec=Some((target,range));cx.notify();
    }
    pub(super) fn numeric_control(&self,id:&str,label:&str,value:f32,min:f32,max:f32,step:f32,default:f32,enabled:bool,target:InputTarget,cx:&mut Context<Self>)->AnyElement{
        let id=id.to_owned();let value=value.clamp(min,max);let fraction=(value-min)/(max-min);
        let origin=if min<0. && max>0.{-min/(max-min)}else{0.};
        let drag=slider_drag_flag(&self.slider_drags,&id);if !enabled{drag.set(false);}
        let bounds=Rc::new(Cell::new(Bounds::<Pixels>::default()));let measure=bounds.clone();let down=bounds.clone();let movement=bounds.clone();
        let target_down=target.clone();let target_move=target.clone();let drag_down=drag.clone();let drag_move=drag.clone();let drag_up=drag.clone();
        let nested=self.snapshot.crop_editing;
        let mut track=div().id(SharedString::from(format!("slider-{id}"))).relative().h(px(18.)).w_full()
            .child(canvas(move|b,_,_|{measure.set(b);},move|b,_,window:&mut Window,_|{
                let width=f32::from(b.size.width).max(0.);
                let left=b.origin.x;let center=b.origin.y+b.size.height/2.;
                let at=left+px(fraction*width);let anchor=left+px(origin*width);
                let thumb=left+px((fraction*width-4.).clamp(0.,(width-8.).max(0.)));
                window.paint_quad(gpui::fill(Bounds::new(point(left,center-px(1.5)),gpui::size(px(width),px(3.))),theme::edge()).corner_radii(px(1.5)));
                window.paint_quad(gpui::fill(Bounds::new(point(anchor.min(at),center-px(1.5)),gpui::size((at-anchor).abs(),px(3.))),
                    if enabled{gpui::Rgba{a:0.7,..theme::accent()}}else{gpui::Rgba{a:0.4,..theme::faint()}}).corner_radii(px(1.5)));
                if min<0. && max>0.{window.paint_quad(gpui::fill(Bounds::new(point(anchor-px(0.5),center-px(4.)),gpui::size(px(1.),px(8.))),theme::faint()));}
                window.paint_quad(gpui::fill(Bounds::new(point(thumb,center-px(4.)),gpui::size(px(8.),px(8.))),
                    if enabled{theme::text()}else{theme::faint()}).corner_radii(px(4.)));
            }).absolute().size_full());
        if enabled{track=track.cursor_ew_resize()
            .on_mouse_down(MouseButton::Left,cx.listener(move|this,e:&gpui::MouseDownEvent,w,cx|{if !this.commit_pending_input(w,cx){return;}drag_down.set(true);if !nested{this.dispatch(AppAction::BeginEdit,w,cx);}let v=slider_value(e.position,down.get(),min,max,step);this.dispatch(this.action_for_value(&target_down,v),w,cx);}))
            .on_mouse_move(cx.listener(move|this,e:&gpui::MouseMoveEvent,w,cx|{if drag_move.get()&&e.dragging(){let v=slider_value(e.position,movement.get(),min,max,step);this.dispatch(this.action_for_value(&target_move,v),w,cx);}}))
            .on_mouse_up(MouseButton::Left,cx.listener(move|this,_,w,cx|{if drag_up.replace(false)&&!nested{this.dispatch(AppAction::EndEdit,w,cx);}}))
            .on_mouse_up_out(MouseButton::Left,cx.listener(move|this,_,w,cx|{if drag.replace(false)&&!nested{this.dispatch(AppAction::EndEdit,w,cx);}}));}
        let label_target=target.clone();let reset_target=target.clone();let minus_target=target.clone();let plus_target=target.clone();let input_target=target;
        let input_id=id.clone();let display=if step<1.{format!("{value:.2}")}else{format!("{value:.0}")};let input_text=display.clone();
        let number=if let Some((key,input))=&self.tools.input{if *key==id{div().w(px(64.)).child(input.clone()).into_any_element()}else{number_label(&id,display,enabled,cx.listener(move|this,_,w,cx|{this.start_input(input_id.clone(),input_target.clone(),input_text.clone(),Some((min,max,step)),w,cx);})).into_any_element()}}
            else{number_label(&id,display,enabled,cx.listener(move|this,_,w,cx|{this.start_input(input_id.clone(),input_target.clone(),input_text.clone(),Some((min,max,step)),w,cx);})).into_any_element()};
        div().flex().flex_col().gap(px(1.))
            .child(div().flex().items_center().justify_between().min_h(px(26.)).gap(px(6.))
                .child(div().id(SharedString::from(format!("label-{id}"))).flex_1().min_w_0().truncate().text_size(px(12.)).text_color(if enabled{theme::text()}else{theme::faint()}).child(label.to_owned())
                    .on_mouse_down(MouseButton::Left,cx.listener(move|this,e:&gpui::MouseDownEvent,w,cx|{if enabled&&e.click_count==2{this.dispatch(this.action_for_value(&label_target,default),w,cx);}})))
                .child(div().flex().flex_shrink_0().items_center().gap(px(2.))
                    .child(div().id(SharedString::from(format!("minus-{id}"))).w(px(20.)).h(px(24.)).flex().items_center().justify_center().rounded(px(4.))
                        .text_size(px(12.)).text_color(if enabled{theme::muted()}else{theme::faint()}).when(enabled,|button|button.cursor_pointer().hover(|style|style.bg(theme::elevated())))
                        .child("−").on_click(cx.listener(move|this,_,w,cx|{if enabled{this.dispatch(AppAction::StepControl(minus_target.clone(),-step),w,cx);}})))
                    .child(number)
                    .child(div().id(SharedString::from(format!("plus-{id}"))).w(px(20.)).h(px(24.)).flex().items_center().justify_center().rounded(px(4.))
                        .text_size(px(12.)).text_color(if enabled{theme::muted()}else{theme::faint()}).when(enabled,|button|button.cursor_pointer().hover(|style|style.bg(theme::elevated())))
                        .child("+").on_click(cx.listener(move|this,_,w,cx|{if enabled{this.dispatch(AppAction::StepControl(plus_target.clone(),step),w,cx);}})))
                    .child(div().id(SharedString::from(format!("reset-{id}"))).w(px(22.)).h(px(24.)).flex().items_center().justify_center().rounded(px(4.))
                        .text_size(px(13.)).text_color(theme::faint()).when(enabled,|button|button.cursor_pointer().hover(|style|style.bg(theme::elevated()).text_color(theme::text())))
                        .child("↺").on_click(cx.listener(move|this,_,w,cx|{if enabled{this.dispatch(this.action_for_value(&reset_target,default),w,cx);}})))))
            .child(track).into_any_element()
    }
    pub(super) fn crop_editor(&self,cx:&mut Context<Self>)->AnyElement{
        let active=self.snapshot.crop_editing;let mut body=div().flex().flex_col().gap(px(12.)).child(section_title("构图"));
        if !active{return body.child(div().text_sm().text_color(theme::muted()).child("裁剪、旋转和拉直均可在工程中继续修改。"))
            .child(div().id("begin-crop").px(px(10.)).py(px(9.)).border_1().border_color(theme::paper()).cursor_pointer().child("编辑构图")
                .on_click(cx.listener(|this,_,w,cx|{if !this.enabled()||!this.commit_pending_input(w,cx){return;}this.tools.crop=Some(this.snapshot.settings.geometry.crop.unwrap_or_default());this.dispatch(AppAction::BeginEdit,w,cx);this.dispatch(AppAction::SetCropEditing(true),w,cx);cx.notify();})))
            .child(action_button("reset-geometry","恢复原构图",self.enabled(),false,AppAction::ResetGeometry,self.on_action.clone())).into_any_element();}
        body=body.child(div().text_xs().text_color(theme::muted()).child("拖出裁剪框，框内拖动可移动。Enter确认，Esc取消。"));
        let mut ratios=div().flex().flex_wrap().gap(px(5.));
        let (sw,sh)=self.snapshot.source_size.unwrap_or((1,1));let (gw,gh)=output_dimensions(sw,sh,&self.visible_geometry());
        for (id,label,ratio)in[("free","自由",None),("original","原比例",Some(gw as f32/gh.max(1)as f32)),("square","1:1",Some(1.)),("portrait","4:5",Some(0.8)),("photo","3:2",Some(1.5)),("wide","16:9",Some(16./9.))]{
            ratios=ratios.child(div().id(SharedString::from(format!("crop-ratio-{id}"))).px(px(7.)).py(px(5.)).border_1().border_color(if this_ratio(self.tools.ratio,ratio){theme::paper()}else{theme::edge()}).cursor_pointer().child(label)
                .on_click(cx.listener(move|this,_,w,cx|{if !this.commit_pending_input(w,cx){return;}this.tools.ratio=ratio;this.tools.crop=Some(centered_ratio(ratio,gw,gh));cx.notify();})));
        }
        body=body.child(ratios).child(div().flex().gap(px(7.))
            .child(div().id("rotate-left").px(px(9.)).py(px(7.)).border_1().border_color(theme::edge()).cursor_pointer().child("↶ 90°").on_click(cx.listener(|this,_,w,cx|this.rotate_crop(3,w,cx))))
            .child(div().id("rotate-right").px(px(9.)).py(px(7.)).border_1().border_color(theme::edge()).cursor_pointer().child("↷ 90°").on_click(cx.listener(|this,_,w,cx|this.rotate_crop(1,w,cx)))));
        body=body.child(self.numeric_control("straighten","拉直角度",self.snapshot.settings.geometry.straighten_degrees,-15.,15.,0.1,0.,true,InputTarget::Straighten,cx));
        body.child(div().flex().gap(px(7.))
            .child(div().id("confirm-crop").px(px(10.)).py(px(8.)).bg(theme::well()).border_1().border_color(theme::paper()).cursor_pointer().child("确认构图").on_click(cx.listener(|this,_,w,cx|this.finish_crop(true,w,cx))))
            .child(div().id("cancel-crop").px(px(10.)).py(px(8.)).border_1().border_color(theme::edge()).cursor_pointer().child("取消").on_click(cx.listener(|this,_,w,cx|this.finish_crop(false,w,cx))))).into_any_element()
    }
    fn rotate_crop(&mut self,turns:u8,w:&mut Window,cx:&mut Context<Self>){if !self.commit_pending_input(w,cx){return;}let mut g=self.snapshot.settings.geometry;g.quarter_turns=(g.quarter_turns+turns)%4;let(sw,sh)=self.snapshot.source_size.unwrap_or((1,1));let(gw,gh)=output_dimensions(sw,sh,&g.without_crop());self.tools.crop=Some(centered_ratio(self.tools.ratio,gw,gh));self.dispatch(AppAction::RotateGeometry(turns),w,cx);cx.notify();}
    pub(super) fn finish_crop(&mut self,apply:bool,w:&mut Window,cx:&mut Context<Self>){
        if apply{if !self.commit_pending_input(w,cx){return;}self.dispatch(AppAction::SetCrop(self.tools.crop),w,cx);self.dispatch(AppAction::EndEdit,w,cx);}else{self.tools.input=None;self.tools.input_spec=None;self.tools.input_generation=self.tools.input_generation.wrapping_add(1);self.dispatch(AppAction::CancelEdit,w,cx);}
        self.dispatch(AppAction::SetCropEditing(false),w,cx);self.tools.crop=None;self.tools.crop_anchor=None;self.tools.crop_before=None;cx.notify();
    }
    pub(super) fn mask_editor(&self,cx:&mut Context<Self>)->AnyElement{
        let enabled=self.enabled();let mut body=div().flex().flex_col().gap(px(11.)).child(section_title("局部蒙版"))
            .child(div().text_xs().text_color(theme::muted()).child("先选形状，再在照片上拖出范围。选中后可拖动控制点。"));
        let mut buttons=div().flex().gap(px(6.));
        for(radial,label)in[(false,"线性渐变"),(true,"径向椭圆")]{buttons=buttons.child(div().id(if radial{"new-radial-mask"}else{"new-linear-mask"}).px(px(8.)).py(px(7.)).border_1().border_color(if self.tools.mask_draw==Some(radial){theme::paper()}else{theme::edge()}).cursor_pointer().child(label)
            .on_click(cx.listener(move|this,_,w,cx|{if enabled{this.tools.mask_draw=Some(radial);this.dispatch(AppAction::SetMaskOverlay(true),w,cx);cx.notify();}})));}
        body=body.child(buttons).child(action_button("mask-overlay","显示覆盖范围",enabled,self.snapshot.mask_overlay,AppAction::SetMaskOverlay(!self.snapshot.mask_overlay),self.on_action.clone()));
        for(index,mask)in self.snapshot.settings.masks.iter().enumerate(){let selected=self.snapshot.selected_mask==Some(index);body=body.child(action_button(format!("mask-item-{index}"),format!("{} {}",if mask.enabled{"●"}else{"○"},mask.name),enabled,selected,AppAction::SelectMask(Some(index)),self.on_action.clone()));}
        if let Some(index)=self.snapshot.selected_mask.filter(|i|*i<self.snapshot.settings.masks.len()){
            let mask=&self.snapshot.settings.masks[index];let mut commands=div().flex().flex_wrap().gap(px(5.));
            for(kind,label)in[(0,if mask.enabled{"停用"}else{"启用"}),(1,if mask.invert{"取消反选"}else{"反选"}),(2,"删除")]{commands=commands.child(div().id(SharedString::from(format!("mask-command-{kind}"))).px(px(8.)).py(px(5.)).border_1().border_color(theme::edge()).cursor_pointer().child(label)
                .on_click(cx.listener(move|this,_,w,cx|{this.dispatch(AppAction::MaskCommand(index,kind),w,cx);}))); 
            }
            body=body.child(commands);
            let name_id=format!("mask-name-{index}");
            if let Some((id,input))=&self.tools.input{if *id==name_id{body=body.child(input.clone());}else{body=body.child(self.rename_mask(index,cx));}}else{body=body.child(self.rename_mask(index,cx));}
            let a=&mask.adjustments;
            for(key,label,value,min,max,step)in[("exposure","局部曝光",a.exposure,-3.,3.,0.05),("highlights","局部高光",a.highlights,-100.,100.,1.),("shadows","局部阴影",a.shadows,-100.,100.,1.),("temperature","局部色温",a.temperature,-100.,100.,1.),("saturation","局部饱和度",a.saturation,-100.,100.,1.)]{body=body.child(self.numeric_control(&format!("mask-{index}-{key}"),label,value,min,max,step,0.,enabled,InputTarget::Local(index,key.into()),cx));}
            if let MaskShape::Radial{feather,..}=mask.shape{body=body.child(self.numeric_control("radial-feather","边缘羽化",feather*100.,0.,100.,1.,50.,enabled,InputTarget::Feather(index),cx));}
            else{body=body.child(div().text_xs().text_color(theme::muted()).child("拖动渐变两端可调整方向与过渡宽度。"));}
        }
        body.into_any_element()
    }
    fn rename_mask(&self,index:usize,cx:&mut Context<Self>)->AnyElement{div().id("rename-mask").text_sm().text_color(theme::paper()).cursor_pointer().child("重命名蒙版…")
        .on_click(cx.listener(move|this,_,w,cx|{if let Some(mask)=this.snapshot.settings.masks.get(index){this.start_input(format!("mask-name-{index}"),InputTarget::Name(index),mask.name.clone(),None,w,cx);}})).into_any_element()}
    pub(super) fn tool_down(&mut self,p:[f32;2],w:&mut Window,cx:&mut Context<Self>)->bool{
        if !self.commit_pending_input(w,cx){return true;}
        if self.snapshot.crop_editing{
            let p=p.map(|v|v.clamp(0.0001,0.9999));
            self.tools.crop_anchor=Some(p);let crop=self.tools.crop.unwrap_or_default();
            self.tools.crop_before=if (crop.width<0.999||crop.height<0.999)&&inside(p,crop){Some(crop)}else{None};
            if self.tools.crop_before.is_none(){self.tools.crop=Some(NormalizedRect{x:p[0],y:p[1],width:0.0001,height:0.0001});}cx.notify();return true;
        }
        if self.snapshot.panel!="masks"{return false;}
        let source=self.source_point(p);
        if let Some(radial)=self.tools.mask_draw{
            if self.snapshot.settings.masks.len()>=32{return true;}
            self.dispatch(AppAction::BeginEdit,w,cx);self.tools.mask_start=Some(source);
            let shape=if radial{MaskShape::Radial{center:source,radius:[0.01,0.01],feather:0.5}}else{MaskShape::Linear{start:source,end:[(source[0]+0.05).min(1.),source[1]]}};
            let index=self.snapshot.settings.masks.len();let mask=LocalMask{id:uuid::Uuid::new_v4().to_string(),name:format!("{} {}",if radial{"径向"}else{"线性"},index+1),enabled:true,invert:false,shape,adjustments:LocalAdjustments::default()};
            self.tools.mask_draft=Some((index,mask));cx.notify();return true;
        }
        if let Some(index)=self.snapshot.selected_mask{if let Some(mask)=self.snapshot.settings.masks.get(index){
            let(sw,sh)=self.snapshot.source_size.unwrap_or((1,1));let geometry=self.visible_geometry();
            let nearest=mask_handles(mask).iter().enumerate().map(|(i,q)|(i,distance(source_to_output(*q,sw,sh,&geometry),p))).min_by(|a,b|a.1.total_cmp(&b.1));
            if let Some((handle,d))=nearest.filter(|(_,d)|*d<0.003){let _=d;self.dispatch(AppAction::BeginEdit,w,cx);self.tools.mask_draft=Some((index,mask.clone()));self.tools.mask_handle=Some(handle);self.tools.mask_start=Some(source);cx.notify();return true;}
        }}false
    }
    pub(super) fn tool_move(&mut self,p:[f32;2],cx:&mut Context<Self>){
        if let Some(start)=self.tools.crop_anchor{
            if let Some(old)=self.tools.crop_before{self.tools.crop=Some(NormalizedRect{x:(old.x+p[0]-start[0]).clamp(0.,1.-old.width),y:(old.y+p[1]-start[1]).clamp(0.,1.-old.height),..old});}
            else{let(sw,sh)=self.snapshot.source_size.unwrap_or((1,1));let(gw,gh)=output_dimensions(sw,sh,&self.visible_geometry());self.tools.crop=Some(rect_with_ratio(start,p,self.tools.ratio,gw,gh));}cx.notify();return;
        }
        let source=self.source_point(p);let radial=self.tools.mask_draw==Some(true);
        if let Some((_,mask))=&mut self.tools.mask_draft{
            if let Some(handle)=self.tools.mask_handle{match &mut mask.shape{
                MaskShape::Linear{start,end}=>if handle==0{*start=source}else{*end=source},
                MaskShape::Radial{center,radius,..}=>match handle{0=>*center=source,1=>radius[0]=(source[0]-center[0]).abs().max(0.005),_=>radius[1]=(source[1]-center[1]).abs().max(0.005)},
            }}else if let Some(start)=self.tools.mask_start{mask.shape=if radial{MaskShape::Radial{center:[(start[0]+source[0])/2.,(start[1]+source[1])/2.],radius:[((start[0]-source[0]).abs()/2.).max(0.005),((start[1]-source[1]).abs()/2.).max(0.005)],feather:0.5}}else{MaskShape::Linear{start,end:source}};}
            cx.notify();
        }
    }
    pub(super) fn tool_up(&mut self,w:&mut Window,cx:&mut Context<Self>){
        self.tools.crop_anchor=None;self.tools.crop_before=None;
        if let Some((index,mask))=self.tools.mask_draft.take(){
            let degenerate=matches!(mask.shape,MaskShape::Linear{start,end} if distance(start,end)<0.000001);
            if degenerate{self.dispatch(AppAction::CancelEdit,w,cx);}else{
                let mut masks=self.snapshot.settings.masks.clone();if index<masks.len(){masks[index].shape=mask.shape;}else{masks.push(mask);}
                self.dispatch(AppAction::SetMasks(masks),w,cx);self.dispatch(AppAction::SelectMask(Some(index)),w,cx);self.dispatch(AppAction::EndEdit,w,cx);
            }
        }
        self.tools.mask_start=None;self.tools.mask_handle=None;self.tools.mask_draw=None;cx.notify();
    }
}

fn number_label(id:&str,text:String,enabled:bool,listener:impl Fn(&gpui::ClickEvent,&mut Window,&mut App)+'static)->gpui::Stateful<gpui::Div>{
    let label=div().id(SharedString::from(format!("number-{id}"))).w(px(64.)).h(px(24.)).flex().items_center().justify_center()
        .px(px(5.)).rounded(px(4.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(if enabled{theme::text()}else{theme::faint()})
        .bg(theme::well()).border_1().border_color(if enabled{theme::edge()}else{gpui::Rgba{a:0.5,..theme::edge()}}).child(text);
    if enabled{label.cursor_text().hover(|style|style.bg(theme::hover()).border_color(theme::accent())).on_click(listener)}else{label}
}
fn parse_numeric_input(text:&str,min:f32,max:f32,step:f32)->Result<f32,String>{
    if !min.is_finite() || !max.is_finite() || min>max || !step.is_finite() || step<=0. {return Err("输入范围无效".into());}
    let value=text.trim().parse::<f32>().map_err(|_|"请输入有效数字".to_string())?;
    if !value.is_finite() || value<min || value>max {return Err(format!("请输入 {min} 到 {max} 之间的数字"));}
    Ok(((value/step).round()*step).clamp(min,max))
}
fn parse_mask_name(text:&str)->Result<String,String>{
    let name=text.trim();
    if name.is_empty() || name.chars().count()>40 {return Err("名称应为1到40个字".into());}
    Ok(name.to_owned())
}
fn slider_value(p:Point<Pixels>,bounds:Bounds<Pixels>,min:f32,max:f32,step:f32)->f32{let f=((f32::from(p.x)-f32::from(bounds.origin.x))/f32::from(bounds.size.width).max(1.)).clamp(0.,1.);((min+f*(max-min))/step).round().mul_add(step,0.).clamp(min,max)}
fn this_ratio(a:Option<f32>,b:Option<f32>)->bool{match(a,b){(None,None)=>true,(Some(a),Some(b))=>(a-b).abs()<0.001,_=>false}}
fn inside(p:[f32;2],r:NormalizedRect)->bool{p[0]>=r.x&&p[0]<=r.x+r.width&&p[1]>=r.y&&p[1]<=r.y+r.height}
fn centered_ratio(ratio:Option<f32>,w:u32,h:u32)->NormalizedRect{let Some(r)=ratio else{return NormalizedRect::default();};let n=r*h.max(1)as f32/w.max(1)as f32;let(cw,ch)=if n>1.{(1.,1./n)}else{(n,1.)};NormalizedRect{x:(1.-cw)/2.,y:(1.-ch)/2.,width:cw,height:ch}}
fn rect_with_ratio(a:[f32;2],b:[f32;2],ratio:Option<f32>,w:u32,h:u32)->NormalizedRect{let a=a.map(|v|v.clamp(0.0001,0.9999));let mut dx=(b[0]-a[0]).abs().max(0.002);let mut dy=(b[1]-a[1]).abs().max(0.002);if let Some(r)=ratio{let n=r*h.max(1)as f32/w.max(1)as f32;if dx/dy>n{dy=dx/n}else{dx=dy*n;}}
    let maxx=if b[0]>=a[0]{1.-a[0]}else{a[0]};let maxy=if b[1]>=a[1]{1.-a[1]}else{a[1]};let scale=(maxx/dx).min(maxy/dy).min(1.);dx=(dx*scale).max(0.0001);dy=(dy*scale).max(0.0001);NormalizedRect{x:if b[0]>=a[0]{a[0]}else{a[0]-dx},y:if b[1]>=a[1]{a[1]}else{a[1]-dy},width:dx,height:dy}}
fn mask_handles(mask:&LocalMask)->Vec<[f32;2]>{match mask.shape{MaskShape::Linear{start,end}=>vec![start,end],MaskShape::Radial{center,radius,..}=>vec![center,[center[0]+radius[0],center[1]],[center[0],center[1]+radius[1]]]}}

pub(super) fn paint_overlay(snapshot:&AppSnapshot,geometry:GeometrySettings,crop:Option<NormalizedRect>,draft:Option<(usize,LocalMask)>,bounds:Bounds<Pixels>,window:&mut Window){
    let(sw,sh)=snapshot.source_size.unwrap_or((1,1));let map=|p:[f32;2]|point(bounds.origin.x+bounds.size.width*p[0],bounds.origin.y+bounds.size.height*p[1]);
    if snapshot.panel=="portrait"{if let Some(r)=snapshot.settings.skin_region{let pts=[[r.x,r.y],[r.x+r.width,r.y],[r.x+r.width,r.y+r.height],[r.x,r.y+r.height],[r.x,r.y]];paint_line(window,&pts.map(|p|map(source_to_output(p,sw,sh,&geometry))),theme::paper());}}
    if snapshot.crop_editing{if let Some(r)=crop{let x=r.x;let y=r.y;let right=x+r.width;let bottom=y+r.height;
        for n in 0..=3{let t=n as f32/3.;paint_line(window,&[map([x+r.width*t,y]),map([x+r.width*t,bottom])],theme::paper());paint_line(window,&[map([x,y+r.height*t]),map([right,y+r.height*t])],theme::paper());}
        for p in [[x,y],[right,y],[right,bottom],[x,bottom]]{let p=map(p);window.paint_quad(gpui::fill(Bounds::new(point(p.x-px(4.),p.y-px(4.)),gpui::size(px(8.),px(8.))),theme::paper()));}
    }}
    if snapshot.panel!="masks"{return;}
    let selected=draft.as_ref().map(|(_,m)|m).or_else(||snapshot.selected_mask.and_then(|i|snapshot.settings.masks.get(i)));
    let Some(mask)=selected else{return;};
    if snapshot.mask_overlay{for y in 0..40{for x in 0..60{let p=[(x as f32+0.5)/60.,(y as f32+0.5)/40.];let source=output_to_source(p,sw,sh,&geometry);let alpha=(mask.coverage_at_with_size(source,sw,sh)*100.).clamp(0.,100.)as u32;if alpha>0{window.paint_quad(gpui::fill(Bounds::new(map([x as f32/60.,y as f32/40.]),gpui::size(bounds.size.width/60.,bounds.size.height/40.)),gpui::rgba(0xd8676700|alpha)));}}}}
    let handles=mask_handles(mask);let projected:Vec<_>=handles.iter().map(|p|map(source_to_output(*p,sw,sh,&geometry))).collect();
    match mask.shape{MaskShape::Linear{..}=>paint_line(window,&projected,theme::paper()),MaskShape::Radial{center,radius,..}=>{
        let pts:Vec<_>=(0..=64).map(|i|{let a=i as f32/64.*std::f32::consts::TAU;map(source_to_output([center[0]+radius[0]*a.cos(),center[1]+radius[1]*a.sin()],sw,sh,&geometry))}).collect();paint_line(window,&pts,theme::paper());
    }}
    for p in projected{window.paint_quad(gpui::fill(Bounds::new(point(p.x-px(4.),p.y-px(4.)),gpui::size(px(8.),px(8.))),theme::paper()));}
}

#[cfg(test)]mod tests{use super::*;
    #[test]fn numeric_drafts_validate_before_quantizing(){
        assert!((parse_numeric_input(" 0.53 ",-3.,3.,0.05).unwrap()-0.55).abs()<1e-6);
        assert_eq!(parse_numeric_input("-3",-3.,3.,0.05).unwrap(),-3.);
        assert_eq!(parse_numeric_input("3",-3.,3.,0.05).unwrap(),3.);
        for text in ["", "invalid", "NaN", "inf", "-inf", "3.01", "-3.01"] {assert!(parse_numeric_input(text,-3.,3.,0.05).is_err(),"{text}");}
        assert!(parse_numeric_input("1",0.,100.,0.).is_err());
        assert_eq!(parse_numeric_input("96",1.,100.,1.).unwrap(),96.);
        assert!(parse_numeric_input("0",1.,30000.,1.).is_err());
    }
    #[test]fn mask_names_count_characters_and_reject_blank_drafts(){
        assert_eq!(parse_mask_name("  天空补光  ").unwrap(),"天空补光");
        assert!(parse_mask_name(" \t ").is_err());
        assert!(parse_mask_name(&"天".repeat(40)).is_ok());
        assert!(parse_mask_name(&"天".repeat(41)).is_err());
    }
    #[test]fn constrained_crop_keeps_requested_physical_ratio(){let r=rect_with_ratio([0.1,0.1],[0.7,0.8],Some(4./5.),1600,1000);assert!((r.width*1600./(r.height*1000.)-0.8).abs()<0.001);assert!(r.validate().is_ok());}
    #[test]fn centered_crop_stays_inside_frame(){for ratio in [0.8,1.,1.5,16./9.]{let r=centered_ratio(Some(ratio),1000,1600);r.validate().unwrap();assert!((r.width*1000./(r.height*1600.)-ratio).abs()<0.001);}}
    #[test]fn crop_drag_at_frame_edges_always_produces_valid_rectangle(){for a in [[0.,0.],[1.,1.],[0.,1.],[1.,0.]]{for b in [[0.,0.],[1.,1.],[0.5,0.5]]{for ratio in [None,Some(0.8),Some(16./9.)]{rect_with_ratio(a,b,ratio,1600,1000).validate().unwrap();}}}}
}
