use std::{collections::BTreeMap, path::{Path, PathBuf}, rc::Rc, sync::{Arc, Mutex}, time::Duration};

use gpui::{AnyWindowHandle, App, AppContext, Application, Bounds, Context, Entity,
    PathPromptOptions, PromptButton, PromptLevel, Render, RenderImage, Size, Window,
    WindowBounds, WindowOptions, prelude::*, px, size};
use image::{Frame as ImageFrame, RgbImage, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use chromora::{core::{self, AdjustmentSettings, CoreError, CoreResult, PhotoDocument,
    PhotoInfo, SkinRegion, WorkspaceDocument}, paths::StoragePaths};
use crate::ui::workbench::Workbench;
#[path = "edit_history.rs"] mod edit_history;
use edit_history::{EditHistory, EditState};
use chromora::core::{GeometrySettings, LocalMask};

#[derive(Clone, Debug, PartialEq)]
pub enum ZoomMode { Fit, Actual, Percent(f32) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat { Jpeg, Png, Tiff, Psd, Patchy }
impl ExportFormat {
    fn extension(self) -> &'static str { match self { Self::Jpeg=>"jpg", Self::Png=>"png", Self::Tiff|Self::Patchy=>"tif", Self::Psd=>"psd" } }
    pub fn label(self) -> &'static str { match self { Self::Jpeg=>"JPG",Self::Png=>"16 位 PNG",Self::Tiff=>"16 位 TIFF",Self::Psd=>"双图层 PSD",Self::Patchy=>"Patchy 精修" } }
    fn kind(self)->Option<core::ExportKind>{match self{Self::Jpeg=>Some(core::ExportKind::Jpeg),Self::Png=>Some(core::ExportKind::Png),Self::Tiff=>Some(core::ExportKind::Tiff),Self::Psd=>Some(core::ExportKind::Psd),Self::Patchy=>None}}
}
#[derive(Clone)]
pub struct PhotoThumbnail { pub name:String, pub preview:Arc<RenderImage>, pub adjusted:bool }
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OperationProgress {
    /// Files completed, including failures. A zero total means directory scanning.
    pub completed:usize, pub total:usize, pub failed:usize, pub current_file:String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)] pub luts:Vec<PathBuf>,
    #[serde(default)] pub favorites:Vec<PathBuf>,
    #[serde(default)] pub recent_presets:Vec<PathBuf>,
}
#[derive(Clone)]
pub struct AppSnapshot {
    pub data_root:String,
    pub project_name:String, pub project_path:Option<PathBuf>,
    pub presets:Vec<core::PresetEntry>, pub recent_presets:Vec<PathBuf>, pub presets_open:bool,
    pub last_preset:Option<String>, pub can_paste_color:bool,
    pub export_options:core::ExportOptions, pub export_format:ExportFormat,
    pub color_pipeline:Option<core::ColorPipeline>,
    pub image_name:Option<String>, pub image_size:Option<(u32,u32)>,
    pub preview:Option<Arc<RenderImage>>, pub original_preview:Option<Arc<RenderImage>>,
    pub photo_info:Option<PhotoInfo>, pub settings:AdjustmentSettings,
    pub photos:Vec<PhotoThumbnail>, pub current_index:usize,
    pub style_previews:BTreeMap<String,Arc<RenderImage>>,
    pub luts:Vec<PathBuf>, pub favorites:Vec<PathBuf>, pub selected_lut:Option<PathBuf>,
    pub zoom:ZoomMode, pub busy:bool, pub rendering:bool, pub dirty:bool,
    pub status:String, pub error:Option<String>, pub panel:&'static str,
    pub compare:bool, pub split:f32, pub peeking:bool,
    pub source_size:Option<(u32,u32)>, pub can_undo:bool, pub can_redo:bool,
    pub crop_editing:bool, pub selected_mask:Option<usize>, pub mask_overlay:bool,
    pub progress:Option<OperationProgress>, pub can_cancel_operation:bool,
    pub diagnostics:Option<Arc<core::PreviewDiagnostics>>,
}
#[derive(Clone, Debug)]
pub enum AppAction {
    OpenImage, OpenFolder, ImportPaths(Vec<PathBuf>), OpenProject, SaveProject, SaveProjectAs,
    Export(ExportFormat), SetValue(String,f32), SetCurve(String,Vec<[f32;2]>),
    SetSkinRegion(Option<SkinRegion>), SetZoom(ZoomMode), SelectPhoto(usize), ClearPhotos,
    SetPanel(&'static str), ImportLut, SelectLut(Option<PathBuf>), ToggleFavorite(PathBuf),
    SavePreset(bool), OpenPreset(bool), Reset, ToggleCompare, SetSplit(f32), PeekOriginal(bool),
    BeginEdit, EndEdit, CancelEdit, Undo, Redo, CancelOperation, FlushInputs,
    SetGeometry(GeometrySettings), SetMasks(Vec<LocalMask>), SetCropEditing(bool),
    SelectMask(Option<usize>), SetMaskOverlay(bool),
    TogglePresets, RefreshPresets, ApplyPreset(PathBuf,bool), CopyColor, PasteColor,
    SetExportFormat(ExportFormat), SetExportLongEdge(Option<u32>), SetExportColorSpace(core::ExportColorSpace), ExportAll,
    SetControl(InputTarget,f32), StepControl(InputTarget,f32), ResetGeometry, RotateGeometry(u8), SetCrop(Option<core::NormalizedRect>),
    RenameMask(usize,String), MaskCommand(usize,u8),
}
pub type ActionHandler = Rc<dyn Fn(AppAction,&mut Window,&mut App)>;
#[derive(Clone,Debug)]
pub enum InputTarget { Global(String), Local(usize,String), Straighten, Feather(usize), Name(usize), ExportQuality, ExportLongEdge }

pub struct AppState {
    pub data_root:String,
    pub project_name:String, pub project_path:Option<PathBuf>,
    pub presets:Vec<core::PresetEntry>, pub presets_open:bool, pub last_preset:Option<String>,
    pub color_clipboard:Option<AdjustmentSettings>,
    pub export_options:core::ExportOptions, pub export_format:ExportFormat,
    pub documents:Vec<PhotoDocument>, pub thumbnails:Vec<PhotoThumbnail>, pub current_index:usize,
    pub style_previews:BTreeMap<String,Arc<RenderImage>>,
    pub preview:Option<Arc<RenderImage>>, pub original_preview:Option<Arc<RenderImage>>,
    pub preferences:Preferences, pub zoom:ZoomMode, pub busy:bool, pub rendering:bool,
    pub dirty:bool, pub status:String, pub error:Option<String>, pub panel:&'static str,
    pub compare:bool, pub split:f32, pub peeking:bool,
    histories:Vec<EditHistory>, pub crop_editing:bool, pub selected_mask:Option<usize>, pub mask_overlay:bool,
    original_geometry:Option<GeometrySettings>, original_is_full:bool,
    edit_dirty_before:Option<bool>,
    pub progress:Option<OperationProgress>, pub can_cancel_operation:bool,
    pub diagnostics:Option<Arc<core::PreviewDiagnostics>>,
}
impl Default for AppState {
    fn default()->Self { Self { data_root:String::new(),documents:vec![],thumbnails:vec![],current_index:0,style_previews:BTreeMap::new(),
        project_name:"未命名工程".into(),project_path:None,presets:vec![],presets_open:false,last_preset:None,
        color_clipboard:None,export_options:core::ExportOptions::default(),export_format:ExportFormat::Jpeg,
        preview:None,original_preview:None,preferences:Preferences::default(),zoom:ZoomMode::Fit,
        busy:false,rendering:false,dirty:false,status:"打开照片，或将照片拖入画布".into(),
        error:None,panel:"landscape",compare:false,split:50.,peeking:false,
        histories:vec![],crop_editing:false,selected_mask:None,mask_overlay:false,original_geometry:None,original_is_full:false,edit_dirty_before:None,
        progress:None,can_cancel_operation:false,diagnostics:None } }
}
impl AppState {
    fn current(&self)->Option<&PhotoDocument> { self.documents.get(self.current_index) }
    fn snapshot(&self)->AppSnapshot {
        let doc=self.current();
        AppSnapshot { data_root:self.data_root.clone(),project_name:self.project_name.clone(),project_path:self.project_path.clone(),
            presets:self.presets.clone(),recent_presets:self.preferences.recent_presets.clone(),presets_open:self.presets_open,
            last_preset:self.last_preset.clone(),can_paste_color:self.color_clipboard.is_some(),export_options:self.export_options.clone(),export_format:self.export_format,
            color_pipeline:doc.map(PhotoDocument::color_pipeline),image_name:doc.and_then(|d|d.source_path.file_name()).map(|s|s.to_string_lossy().into_owned()),
            image_size:doc.map(|d| {let mut geometry=d.settings.geometry;if self.crop_editing {geometry.crop=None;}core::output_dimensions(d.width(),d.height(),&geometry)}),preview:self.preview.clone(),
            original_preview:self.original_preview.clone(),photo_info:doc.map(|d|d.photo_info().clone()),
            settings:doc.map(|d|d.settings.clone()).unwrap_or_default(),photos:self.thumbnails.clone(),style_previews:self.style_previews.clone(),
            current_index:self.current_index,luts:self.preferences.luts.clone(),favorites:self.preferences.favorites.clone(),
            selected_lut:doc.and_then(|d|d.lut_path.clone()),zoom:self.zoom.clone(),busy:self.busy,
            rendering:self.rendering,dirty:self.dirty,status:self.status.clone(),error:self.error.clone(),
            panel:self.panel,compare:self.compare,split:self.split,peeking:self.peeking,
            source_size:doc.map(|d|(d.width(),d.height())),
            can_undo:self.histories.get(self.current_index).is_some_and(EditHistory::can_undo),
            can_redo:self.histories.get(self.current_index).is_some_and(EditHistory::can_redo),
            crop_editing:self.crop_editing,selected_mask:self.selected_mask,mask_overlay:self.mask_overlay,
            progress:self.progress.clone(),can_cancel_operation:self.can_cancel_operation,diagnostics:self.diagnostics.clone() }
    }
}
#[derive(Clone,Copy)]
enum PickerKind { Images, Folder, Project, Lut, Export(ExportFormat), BatchExport, SavePreset(bool), OpenPreset(bool) }
#[derive(Clone,Copy)]
enum PendingAction { OpenProject, Close }

pub struct AppRoot {
    pub state:AppState, storage:StoragePaths, project_path:Option<PathBuf>, project_name:String,
    project_requires_save_as:bool,
    operation_id:u64, render_id:u64, render_in_flight:bool, workbench:Entity<Workbench>,window_handle:AnyWindowHandle,
    pending_action:Option<PendingAction>,
    operation_cancel:Option<core::RenderCancellation>, render_cancel:Option<core::RenderCancellation>,
    style_preview_id:u64, style_preview_photo:Option<String>, style_preview_cancel:Option<core::RenderCancellation>,
}
impl AppRoot {
    pub fn new(window:&mut Window,cx:&mut Context<Self>)->Self {
        let storage=StoragePaths::default_for_platform();
        let mut state=AppState::default();
        state.data_root=storage.root.to_string_lossy().into_owned();
        if let Err(e)=storage.ensure_data_dirs() {state.error=Some(e.to_string());}
        if let Ok(bytes)=std::fs::read(storage.root.join("数据").join("工作台设置-rust.json")) {
            if let Ok(prefs)=serde_json::from_slice(&bytes) {state.preferences=prefs;}
        }
        let weak=cx.weak_entity();
        let handler:ActionHandler=Rc::new(move|a,w,app| {
            let weak=weak.clone();
            w.defer(app,move|w,app|{
                let Ok(view)=weak.update(app,|r,_|r.workbench.clone()) else{return;};
                let pending=view.update(app,|view,cx|{
                    let pending=if matches!(a,AppAction::FlushInputs){Ok(None)}else{view.take_pending_input(w,cx)};
                    pending.map(|pending|{let mut queued=view.take_committed_inputs();queued.extend(pending);queued})
                });
                let Ok(pending)=pending else{return;};
                let _=weak.update(app,|r,cx|{
                    let before=r.state.current().map(|d|d.settings.clone());
                    for input in pending{r.handle_action(input,w,cx);}
                    let action=if let (Some(before),Some(doc))=(before,r.state.current()){rebase_action(a,&before,&doc.settings)}else{a};
                    r.handle_action(action,w,cx);
                });
            });
        });
        let workbench=cx.new(|cx|Workbench::new(state.snapshot(),handler,cx));
        let focus=workbench.read(cx).focus_handle();
        window.focus(&focus);
        let close=cx.weak_entity();
        window.on_window_should_close(cx,move|window,app|close.update(app,|r,cx|{
            if r.state.busy {return false;}
            let Ok(pending)=r.workbench.update(cx,|view,cx|view.take_pending_input(window,cx).map(|pending|{let mut queued=view.take_committed_inputs();queued.extend(pending);queued})) else{return false;};
            for input in pending{r.handle_action(input,window,cx);}
            if r.state.crop_editing {r.state.crop_editing=false;r.cancel_edit(cx);}
            if r.state.dirty {r.request_transition(PendingAction::Close,window,cx);return false;} true
        }).unwrap_or(true));
        cx.observe_window_activation(window,|r,window,cx|{
            if !window.is_window_active() && r.state.peeking {r.state.peeking=false;r.sync_workbench(cx);}
        }).detach();
        let mut root=Self {state,storage,project_path:None,project_name:"未命名工程".into(),project_requires_save_as:false,operation_id:0,
            render_id:0,render_in_flight:false,workbench,window_handle:window.window_handle(),pending_action:None,
            operation_cancel:None,render_cancel:None,style_preview_id:0,style_preview_photo:None,style_preview_cancel:None};
        root.refresh_presets(cx);root
    }
    fn handle_action(&mut self,a:AppAction,w:&mut Window,cx:&mut Context<Self>) {
        if self.state.busy && !matches!(a,AppAction::CancelOperation|AppAction::SetZoom(_)|AppAction::PeekOriginal(_)|AppAction::SetPanel(_)|AppAction::SetSplit(_)|AppAction::ToggleCompare) {return;}
        if self.state.crop_editing && matches!(a,AppAction::OpenImage|AppAction::OpenFolder|AppAction::ImportPaths(_)|AppAction::OpenProject|AppAction::SaveProject|AppAction::SaveProjectAs|AppAction::Export(_)|AppAction::ExportAll|AppAction::ImportLut|AppAction::SavePreset(_)|AppAction::OpenPreset(_)|AppAction::ApplyPreset(_,_)|AppAction::PasteColor|AppAction::Reset|AppAction::Undo|AppAction::Redo|AppAction::ClearPhotos) {
            self.state.error=Some("请先确认或取消裁剪".into());self.sync_workbench(cx);return;
        }
        match a {
            AppAction::FlushInputs=>{},
            AppAction::SetControl(target,value)=>self.set_control(target,value,cx),
            AppAction::StepControl(target,delta)=>{
                if let Some((value,min,max))=self.control_value(&target){self.set_control(target,(value+delta).clamp(min,max),cx);}
            },
            AppAction::ResetGeometry=>self.edit_settings(|s|{s.geometry=GeometrySettings::default();Ok(())},cx),
            AppAction::RotateGeometry(turns)=>{if let Some(doc)=self.state.current(){let mut g=doc.settings.geometry;g.quarter_turns=(g.quarter_turns+turns)%4;self.handle_action(AppAction::SetGeometry(g),w,cx);}},
            AppAction::SetCrop(crop)=>{if let Some(doc)=self.state.current(){let mut g=doc.settings.geometry;g.crop=crop;self.handle_action(AppAction::SetGeometry(g),w,cx);}},
            AppAction::RenameMask(index,name)=>self.edit_settings(|s|{s.masks.get_mut(index).ok_or_else(||CoreError::InvalidProject("蒙版已不存在".into()))?.name=name;Ok(())},cx),
            AppAction::MaskCommand(index,kind)=>{
                if kind==2{self.state.selected_mask=None;}
                self.edit_settings(|s|{let mask=s.masks.get_mut(index).ok_or_else(||CoreError::InvalidProject("蒙版已不存在".into()))?;match kind{0=>mask.enabled=!mask.enabled,1=>mask.invert=!mask.invert,_=>{s.masks.remove(index);}}Ok(())},cx);
            },
            AppAction::BeginEdit=>self.begin_edit(cx),
            AppAction::EndEdit=>self.end_edit(cx),
            AppAction::CancelEdit=>self.cancel_edit(cx),
            AppAction::CancelOperation=>self.cancel_operation(cx),
            AppAction::Undo=>self.apply_history(false,cx),
            AppAction::Redo=>self.apply_history(true,cx),
            AppAction::SetGeometry(geometry)=>self.edit_settings(move|s|{s.geometry=geometry;Ok(())},cx),
            AppAction::SetMasks(masks)=>self.edit_settings(move|s|{s.masks=masks;Ok(())},cx),
            AppAction::SetCropEditing(value)=>{self.state.crop_editing=value;self.schedule_preview(true,cx);},
            AppAction::SelectMask(index)=>{self.state.selected_mask=index.filter(|i|self.state.current().is_some_and(|d|*i<d.settings.masks.len()));self.sync_workbench(cx);},
            AppAction::SetMaskOverlay(value)=>{self.state.mask_overlay=value;self.sync_workbench(cx);},
            AppAction::OpenImage=>self.open_picker(PickerKind::Images,cx),
            AppAction::OpenFolder=>self.open_picker(PickerKind::Folder,cx),
            AppAction::ImportPaths(paths)=>self.import_paths(paths,cx),
            AppAction::OpenProject=>self.request_transition(PendingAction::OpenProject,w,cx),
            AppAction::SaveProject=>self.save_or_prompt(false,cx),
            AppAction::SaveProjectAs=>self.save_or_prompt(true,cx),
            AppAction::Export(f)=>self.open_picker(PickerKind::Export(f),cx),
            AppAction::SetZoom(z)=>{self.state.zoom=z;self.schedule_preview(false,cx);},
            AppAction::SetPanel(p)=>{
                if p!=self.state.panel {
                    if self.state.crop_editing{self.state.crop_editing=false;self.cancel_edit(cx);}else{self.end_edit(cx);}
                }
                self.state.panel=p;self.sync_workbench(cx);
            },
            AppAction::SelectPhoto(i)=>self.select_photo(i,cx),
            AppAction::ClearPhotos=>self.confirm_clear_photos(w,cx),
            AppAction::SetValue(k,v)=>self.edit_settings(move|s|s.set_value(&k,v),cx),
            AppAction::SetCurve(channel,points)=>self.edit_settings(move|s|{s.curves.insert(channel,points);Ok(())},cx),
            AppAction::SetSkinRegion(region)=>self.edit_settings(move|s|{s.skin_region=region;Ok(())},cx),
            AppAction::Reset=>{
                self.begin_edit(cx);
                if let Some(doc)=self.state.documents.get_mut(self.state.current_index){
                    if let Err(e)=core::set_lut(doc,None){self.finish_error(e.to_string(),cx);return;}
                }
                self.edit_settings(|s|{*s=AdjustmentSettings::default();Ok(())},cx);
                self.end_edit(cx);
            },
            AppAction::ImportLut=>self.open_picker(PickerKind::Lut,cx),
            AppAction::SelectLut(path)=>self.select_lut(path,cx),
            AppAction::ToggleFavorite(path)=>{
                if self.state.preferences.favorites.contains(&path) {self.state.preferences.favorites.retain(|p|p!=&path);}
                else {self.state.preferences.favorites.push(path);}
                self.save_preferences();self.sync_workbench(cx);
            },
            AppAction::SavePreset(p)=>self.open_picker(PickerKind::SavePreset(p),cx),
            AppAction::OpenPreset(p)=>self.open_picker(PickerKind::OpenPreset(p),cx),
            AppAction::ApplyPreset(path,p)=>self.open_preset(path,p,cx),
            AppAction::RefreshPresets=>self.refresh_presets(cx),
            AppAction::TogglePresets=>{self.state.presets_open=!self.state.presets_open;self.sync_workbench(cx);},
            AppAction::CopyColor=>{if let Some(doc)=self.state.current(){self.state.color_clipboard=Some(doc.settings.clone());self.state.status="已复制调色；粘贴保留目标曝光、白平衡、风格及局部区域".into();self.sync_workbench(cx);}},
            AppAction::PasteColor=>{
                if let (Some(source),Some(doc))=(self.state.color_clipboard.as_ref(),self.state.current()){
                    match core::copy_color_settings(source,&doc.settings){Ok(settings)=>{
                        if settings!=doc.settings{self.edit_settings(|s|{*s=settings;Ok(())},cx);}
                        self.state.status="已粘贴调色，保留曝光、白平衡、风格及局部区域".into();self.sync_workbench(cx);
                    },Err(e)=>self.finish_error(e.to_string(),cx)}
                }
            },
            AppAction::SetExportFormat(f)=>{self.state.export_format=f;self.sync_workbench(cx);},
            AppAction::SetExportLongEdge(edge)=>{if edge.is_none_or(|v|(1..=30000).contains(&v)){self.state.export_options.max_long_edge=edge;self.sync_workbench(cx);}},
            AppAction::SetExportColorSpace(space)=>{self.state.export_options.color_space=space;self.sync_workbench(cx);},
            AppAction::ExportAll=>self.open_picker(PickerKind::BatchExport,cx),
            AppAction::ToggleCompare=>{self.state.compare=!self.state.compare;self.sync_workbench(cx);},
            AppAction::SetSplit(x)=>{self.state.split=x.clamp(0.,100.);self.sync_workbench(cx);},
            AppAction::PeekOriginal(x)=>{self.state.peeking=x;self.sync_workbench(cx);},
        }
    }
    fn edit_settings(&mut self,edit:impl FnOnce(&mut AdjustmentSettings)->CoreResult<()>,cx:&mut Context<Self>) {
        let Some(doc)=self.state.documents.get_mut(self.state.current_index) else{return;};
        let before=EditState::capture(doc);
        let mut settings=doc.settings.clone();
        let result=edit(&mut settings).and_then(|_|core::set_settings(doc,settings));
        if let Err(e)=result {self.state.error=Some(e.to_string());self.sync_workbench(cx);return;}
        let current=EditState::capture(doc);
        if before==current{self.sync_workbench(cx);return;}
        self.state.histories.resize_with(self.state.documents.len(),EditHistory::default);
        self.state.histories[self.state.current_index].changed(before,&current);
        self.state.selected_mask=self.state.selected_mask.filter(|i|*i<current.settings.masks.len());
        self.state.dirty=true;self.state.error=None;self.schedule_preview(false,cx);
    }
    fn control_value(&self,target:&InputTarget)->Option<(f32,f32,f32)>{
        match target{
            InputTarget::ExportQuality=>Some((self.state.export_options.jpeg_quality as f32,1.,100.)),
            InputTarget::ExportLongEdge=>Some((self.state.export_options.max_long_edge.unwrap_or(2048) as f32,1.,30000.)),
            InputTarget::Global(key)=>{let &(_,min,max)=AdjustmentSettings::CONTROLS.iter().find(|(name,_,_)|*name==key)?;Some((self.state.current()?.settings.value(key)?,min,max))},
            InputTarget::Local(index,key)=>{let &(_,min,max)=core::LocalAdjustments::CONTROLS.iter().find(|(name,_,_)|*name==key)?;Some((self.state.current()?.settings.masks.get(*index)?.adjustments.value(key)?,min,max))},
            InputTarget::Straighten=>Some((self.state.current()?.settings.geometry.straighten_degrees,-15.,15.)),
            InputTarget::Feather(index)=>if let core::MaskShape::Radial{feather,..}=self.state.current()?.settings.masks.get(*index)?.shape{Some((feather*100.,0.,100.))}else{None},
            InputTarget::Name(_)=>None,
        }
    }
    fn set_control(&mut self,target:InputTarget,value:f32,cx:&mut Context<Self>){
        let Some((_,min,max))=self.control_value(&target) else{return;};
        if !value.is_finite() || !(min..=max).contains(&value){self.finish_error("输入数值超出范围".into(),cx);return;}
        match target{
            InputTarget::ExportQuality=>{self.state.export_options.jpeg_quality=value.round() as u8;self.sync_workbench(cx);},
            InputTarget::ExportLongEdge=>{self.state.export_options.max_long_edge=Some(value.round() as u32);self.sync_workbench(cx);},
            InputTarget::Global(key)=>self.edit_settings(|s|s.set_value(&key,value),cx),
            InputTarget::Local(index,key)=>self.edit_settings(|s|s.masks.get_mut(index).ok_or_else(||CoreError::InvalidProject("蒙版已不存在".into()))?.adjustments.set_value(&key,value),cx),
            InputTarget::Straighten=>self.edit_settings(|s|{s.geometry.straighten_degrees=value;Ok(())},cx),
            InputTarget::Feather(index)=>self.edit_settings(|s|{if let core::MaskShape::Radial{feather,..}=&mut s.masks.get_mut(index).ok_or_else(||CoreError::InvalidProject("蒙版已不存在".into()))?.shape{*feather=value/100.;}Ok(())},cx),
            InputTarget::Name(_)=>{},
        }
    }

    fn begin_edit(&mut self,cx:&mut Context<Self>) {
        let Some(doc)=self.state.current() else{return;};let state=EditState::capture(doc);
        if self.state.edit_dirty_before.is_none(){self.state.edit_dirty_before=Some(self.state.dirty);}
        self.state.histories.resize_with(self.state.documents.len(),EditHistory::default);
        self.state.histories[self.state.current_index].begin(state);self.sync_workbench(cx);
    }
    fn end_edit(&mut self,cx:&mut Context<Self>) {
        let Some(doc)=self.state.current() else{return;};let state=EditState::capture(doc);
        if let Some(history)=self.state.histories.get_mut(self.state.current_index){history.commit(&state);}
        self.state.edit_dirty_before=None;
        self.sync_workbench(cx);
    }
    fn cancel_edit(&mut self,cx:&mut Context<Self>) {
        let state=self.state.histories.get_mut(self.state.current_index).and_then(EditHistory::cancel);
        if let (Some(state),Some(doc))=(state,self.state.documents.get_mut(self.state.current_index)){
            state.restore(doc);self.state.selected_mask=None;self.schedule_preview(true,cx);
            if let Some(dirty)=self.state.edit_dirty_before.take(){self.state.dirty=dirty;self.sync_workbench(cx);}
        }else{self.sync_workbench(cx);}
    }
    fn apply_history(&mut self,redo:bool,cx:&mut Context<Self>) {
        let Some(doc)=self.state.current() else{return;};let current=EditState::capture(doc);
        let Some(history)=self.state.histories.get_mut(self.state.current_index) else{return;};
        let state=if redo{history.redo(current)}else{history.undo(current)};
        self.state.edit_dirty_before=None;
        if let Some(state)=state {state.restore(&mut self.state.documents[self.state.current_index]);
            self.state.dirty=true;self.state.crop_editing=false;self.state.selected_mask=None;
            self.state.error=None;self.schedule_preview(true,cx);}
    }
    fn request_transition(&mut self,a:PendingAction,w:&mut Window,cx:&mut Context<Self>) {
        if self.state.busy {return;}
        if !self.state.dirty {self.perform_transition(a,w,cx);return;}
        let id=self.begin_operation("请处理未保存的修改",cx);
        let receiver=w.prompt(PromptLevel::Warning,"保存当前工程的修改？",Some("所有照片的调整将保存在同一工程。"),
            &[PromptButton::ok("保存"),PromptButton::new("不保存"),PromptButton::cancel("取消")],cx);
        let handle=self.window_handle;
        cx.spawn(async move|root,cx| {let answer=receiver.await;
            let _=handle.update(cx,|_,w,app|{let _=root.update(app,|r,cx|{
                if id!=r.operation_id{return;} r.state.busy=false;
                match answer {Ok(0)=>{r.pending_action=Some(a);r.save_or_prompt(false,cx);},
                    Ok(1)=>r.perform_transition(a,w,cx),_=>r.finish_cancelled("已取消，保留当前修改",cx)}
            });});
        }).detach();
    }
    fn perform_transition(&mut self,a:PendingAction,w:&mut Window,cx:&mut Context<Self>) {
        match a {PendingAction::OpenProject=>self.open_picker(PickerKind::Project,cx),PendingAction::Close=>w.remove_window()}
    }
    fn open_picker(&mut self,kind:PickerKind,cx:&mut Context<Self>) {
        let id=self.begin_operation("请选择文件或目录",cx);
        if matches!(kind,PickerKind::Export(_)|PickerKind::SavePreset(_)) {
            let (directory,name)=match kind {
                PickerKind::Export(f)=>(self.storage.exports.clone(),self.suggested_name(f.extension())),
                _=>(self.storage.root.join("数据").join("预设"),"我的预设.json".into())};
            let _=std::fs::create_dir_all(&directory);
            let receiver=cx.prompt_for_new_path(&directory,Some(&name));
            cx.spawn(async move|root,cx|{
                let result=match receiver.await {Ok(Ok(p))=>Ok(p.map(|p|vec![p])),Ok(Err(e))=>Err(e.to_string()),Err(e)=>Err(e.to_string())};
                let _=root.update(cx,|r,cx|r.handle_selection(kind,id,result,cx));
            }).detach();
        } else {
            let receiver=cx.prompt_for_paths(PathPromptOptions {files:!matches!(kind,PickerKind::Folder|PickerKind::BatchExport),
                directories:matches!(kind,PickerKind::Folder|PickerKind::BatchExport),multiple:matches!(kind,PickerKind::Images),
                prompt:Some(match kind {PickerKind::Images=>"导入照片",PickerKind::Folder=>"导入照片目录",
                    PickerKind::Project=>"打开 Chromora 工程",PickerKind::Lut=>"导入 sRGB 3D .cube",PickerKind::BatchExport=>"选择批量导出目录",_=>"打开个人预设"}.into())});
            cx.spawn(async move|root,cx|{
                let result=match receiver.await {Ok(Ok(p))=>Ok(p),Ok(Err(e))=>Err(e.to_string()),Err(e)=>Err(e.to_string())};
                let _=root.update(cx,|r,cx|r.handle_selection(kind,id,result,cx));
            }).detach();
        }
    }
    fn handle_selection(&mut self,kind:PickerKind,id:u64,result:Result<Option<Vec<PathBuf>>,String>,cx:&mut Context<Self>) {
        if id!=self.operation_id{return;}
        let paths=match result {Ok(Some(p)) if !p.is_empty()=>p,Ok(_)=>{self.finish_cancelled("已取消",cx);return;},Err(e)=>{self.finish_error(e,cx);return;}};
        let path=paths[0].clone();
        match kind {
            PickerKind::Images|PickerKind::Folder=>{self.state.busy=false;self.import_paths(paths,cx);},
            PickerKind::Project=>self.load_workspace(path,id,cx),
            PickerKind::Lut=>self.import_lut(path,id,cx),
            PickerKind::Export(f)=>self.export(path,f,id,cx),
            PickerKind::BatchExport=>self.export_all(path,id,cx),
            PickerKind::SavePreset(p)=>self.save_preset(path,p,cx),
            PickerKind::OpenPreset(p)=>self.open_preset(path,p,cx),
        }
    }
    fn import_paths(&mut self,paths:Vec<PathBuf>,cx:&mut Context<Self>) {
        let id=self.begin_operation("正在导入照片并生成缩略图",cx);
        let directory=self.storage.originals.clone();
        let existing=self.state.documents.len();
        let (cancel,progress)=self.enable_operation_progress(id,OperationProgress::default(),cx);
        cx.spawn(async move|root,cx|{
            let result=cx.background_spawn(async move{
                run_import_batch(paths,&directory,existing,&cancel,|value|{
                    if let Ok(mut latest)=progress.lock(){*latest=value;}
                })
            }).await;
            let _=root.update(cx,|r,cx|{
                if id!=r.operation_id{return;}
                let mut batch=match result {Ok(batch)=>batch,Err(error)=>{r.finish_error(error.to_string(),cx);return;}};
                if r.operation_cancel.as_ref().is_some_and(|cancel|cancel.check().is_err()){batch.cancelled=true;}
                r.clear_operation_progress();
                let first=r.state.documents.len();let count=batch.documents.len();
                r.state.documents.extend(batch.documents);r.state.thumbnails.extend(batch.thumbnails);
                r.state.histories.resize_with(r.state.documents.len(),EditHistory::default);
                if count>0 {r.state.dirty=true;r.select_photo(first,cx);}
                r.state.status=if batch.cancelled{format!("已取消导入，保留已完成的 {count} 张照片")}else{format!("已导入 {count} 张照片")};
                if !batch.errors.is_empty(){r.state.error=Some(batch.errors.join("\n"));}
                if count==0 && !batch.cancelled && r.state.error.is_none(){r.state.error=Some("所选位置没有可导入的照片".into());}
                r.sync_workbench(cx);
            });
        }).detach();
    }
    fn select_photo(&mut self,index:usize,cx:&mut Context<Self>) {
        if index>=self.state.documents.len(){
            if self.state.documents.is_empty(){self.schedule_style_previews(cx,true);}
            return;
        }
        if self.state.crop_editing {self.state.crop_editing=false;self.cancel_edit(cx);}else{self.end_edit(cx);}
        self.render_id=self.render_id.wrapping_add(1);
        for (i,doc) in self.state.documents.iter_mut().enumerate(){if i!=index {doc.evict_pixels();}}
        self.state.current_index=index;self.state.preview=None;self.state.original_preview=None;self.state.diagnostics=None;
        self.state.original_geometry=None;self.state.original_is_full=false;
        self.state.crop_editing=false;self.state.selected_mask=None;self.state.mask_overlay=false;
        self.state.peeking=false;self.schedule_style_previews(cx,true);self.schedule_preview(true,cx);
    }
    fn schedule_preview(&mut self,immediate:bool,cx:&mut Context<Self>) {
        if let Some(cancel)=self.render_cancel.take(){cancel.cancel();}
        self.render_cancel=Some(core::RenderCancellation::new());
        self.render_id=self.render_id.wrapping_add(1);
        self.state.rendering=self.state.current().is_some();
        if !self.state.busy {self.state.status="正在更新预览".into();}
        self.sync_workbench(cx);
        // Keep at most one renderer alive; subsequent edits replace the pending parameters.
        if self.render_in_flight || self.state.current().is_none(){return;}
        self.render_in_flight=true;
        cx.spawn(async move|root,cx|{
            let mut first=true;
            loop {
                cx.background_executor().timer(Duration::from_millis(if first&&immediate{0}else{170})).await;
                first=false;
                let job=root.update(cx,|r,_|r.state.current().cloned().map(|doc| {
                    let mut geometry=doc.settings.geometry;if r.state.crop_editing {geometry.crop=None;}
                    let matching=r.state.original_geometry==Some(geometry)&&r.state.original_preview.is_some();
                    (doc,r.render_id,r.operation_id,r.state.current_index,!matching,
                        !matching||!r.state.original_is_full,!matches!(r.state.zoom,ZoomMode::Fit),geometry,
                        r.render_cancel.as_ref().expect("preview token").clone())
                })).ok().flatten();
                let Some((mut doc,id,operation,index,need_original,need_full_original,full,geometry,cancel))=job else{
                    let _=root.update(cx,|r,cx|{r.render_in_flight=false;r.state.rendering=false;r.sync_workbench(cx);});return;
                };
                let preview_cancel=cancel.clone();
                let result=cx.background_spawn(async move{
                    core::prepare_preview_cancellable(&mut doc,&preview_cancel)?;
                    let mut render_doc=doc.clone();render_doc.settings.geometry=geometry;
                    let rgb=core::render_preview_at_cancellable(&render_doc,Some(1440),&preview_cancel)?;
                    let diagnostics=Arc::new(core::PreviewDiagnostics::from_rgb8(&rgb));
                    preview_cancel.check()?;
                    let thumb=to_render_image(thumbnail_from_preview(&rgb));
                    let preview=to_render_image(rgb);
                    let original=if need_original {Some(to_render_image(core::render_original_cancellable(&render_doc,Some(1440),&preview_cancel)?))}else{None};
                    Ok::<_,CoreError>((doc,render_doc,preview,original,diagnostics,thumb))
                }).await;
                let mut full_doc=None;
                let current=root.update(cx,|r,cx|{
                    if index==r.state.current_index {
                        if let (Ok((loaded,_,_,_,_,_)),Some(current))=(&result,r.state.documents.get_mut(index)){
                            current.reuse_source_cache(loaded);
                        }
                    }
                    if id!=r.render_id || index!=r.state.current_index{return false;}
                    match result {Ok((doc,render_doc,preview,original,diagnostics,thumb))=>{
                        if !r.state.crop_editing{if let Some(item)=r.state.thumbnails.get_mut(index){item.preview=thumb;item.adjusted=photo_is_adjusted(&doc);}}
                        full_doc=Some(render_doc);r.state.documents[index]=doc;r.state.preview=Some(preview);r.state.diagnostics=Some(diagnostics);
                        if let Some(original)=original{r.state.original_preview=Some(original);r.state.original_geometry=Some(geometry);r.state.original_is_full=false;}
                        if !r.state.busy&&r.operation_id==operation&&r.state.error.is_none(){r.state.status="快速预览".into();}
                    },Err(CoreError::Cancelled)=>{},Err(e)=>{if !r.state.busy&&r.operation_id==operation{r.state.error=Some(e.to_string());}}}
                    r.sync_workbench(cx);true
                }).unwrap_or(true);
                if !current {continue;}
                if full {if let Some(doc)=full_doc {
                    cx.background_executor().timer(Duration::from_millis(450)).await;
                    if !root.update(cx,|r,_|r.render_id==id).unwrap_or(false){continue;}
                    let result=cx.background_spawn(async move{
                        let rgb=core::render_preview_at_cancellable(&doc,None,&cancel)?;
                        let diagnostics=Arc::new(core::PreviewDiagnostics::from_rgb8(&rgb));
                        cancel.check()?;
                        Ok::<_,CoreError>((to_render_image(rgb),
                            if need_full_original{Some(to_render_image(core::render_original_cancellable(&doc,None,&cancel)?))}else{None},diagnostics))
                    }).await;
                    let _=root.update(cx,|r,cx|{
                        if id!=r.render_id || index!=r.state.current_index{return;}
                        match result {Ok((preview,original,diagnostics))=>{r.state.preview=Some(preview);r.state.diagnostics=Some(diagnostics);
                            if let Some(original)=original{r.state.original_preview=Some(original);r.state.original_geometry=Some(geometry);r.state.original_is_full=true;}
                            if !r.state.busy&&r.operation_id==operation&&r.state.error.is_none(){r.state.status="全分辨率".into();}},
                            Err(CoreError::Cancelled)=>{},Err(e)=>if !r.state.busy&&r.operation_id==operation{r.state.error=Some(e.to_string());}}
                        r.sync_workbench(cx);
                    });
                }}
                let finished=root.update(cx,|r,cx|{
                    if r.render_id!=id{return false;}
                    r.render_in_flight=false;r.state.rendering=false;r.sync_workbench(cx);true
                }).unwrap_or(true);
                if finished{return;}
            }
        }).detach();
    }
    fn load_workspace(&mut self,path:PathBuf,id:u64,cx:&mut Context<Self>) {
        self.state.status="正在打开工程".into();
        let (cancel,progress)=self.enable_operation_progress(id,OperationProgress{current_file:path.file_name().unwrap_or_default().to_string_lossy().into_owned(),..Default::default()},cx);
        cx.spawn(async move|root,cx|{
            let worker_path=path.clone();
            let result=cx.background_spawn(async move{
                let loaded=core::load_workspace_with_progress(&worker_path,&cancel,|completed,total,file|{
                    if let Ok(mut latest)=progress.lock(){*latest=OperationProgress{completed,total,failed:0,current_file:file.file_name().unwrap_or_default().to_string_lossy().into_owned()};}
                })?;
                cancel.check()?;
                let requires_save_as=loaded.requires_save_as;
                let mut workspace=loaded.workspace;let mut warnings=loaded.history_warnings;
                let mut histories=Vec::with_capacity(workspace.photos.len());
                for (index,saved) in loaded.histories.into_iter().enumerate() {
                    cancel.check()?;
                    match EditHistory::from_project(saved) {
                        Ok(history)=>histories.push(history),
                        Err(error)=>{histories.push(EditHistory::default());warnings.push(format!("第{}张照片的撤销历史无法恢复：{error}；当前调整已保留",index+1));}
                    }
                }
                histories.resize_with(workspace.photos.len(),EditHistory::default);
                let mut thumbs=vec![];
                for (index,doc) in workspace.photos.iter_mut().enumerate() {
                    cancel.check()?;
                    if let core::ColorConversionState::LegacyIccFallback{reason}= &doc.photo_info().color_conversion {
                        warnings.push(format!("第{}张照片保留旧版色彩处理，ICC 未转换：{reason}",index+1));
                    }
                    let thumbnail=core::render_preview_at_cancellable(doc,Some(240),&cancel)?;
                    thumbs.push(PhotoThumbnail{name:photo_name(doc),preview:to_render_image(thumbnail),adjusted:photo_is_adjusted(doc)});
                    if index!=workspace.current_index {doc.evict_pixels();}
                } cancel.check()?;Ok::<_,CoreError>((workspace,thumbs,histories,warnings,requires_save_as))
            }).await;
            let _=root.update(cx,|r,cx|{
                if id!=r.operation_id{return;}
                if r.operation_cancel.as_ref().is_some_and(|cancel|cancel.check().is_err()){
                    r.finish_cancelled("已取消打开工程，保留当前工作区",cx);return;
                }
                match result {Ok((workspace,thumbs,histories,warnings,requires_save_as))=>{
                    r.state.documents=workspace.photos;r.state.thumbnails=thumbs;r.project_name=workspace.name;
                    r.state.histories=histories;
                    r.state.edit_dirty_before=None;
                    r.state.current_index=workspace.current_index.min(r.state.documents.len().saturating_sub(1));
                    r.project_path=if path.extension().is_some_and(|s|s.eq_ignore_ascii_case("json")){None}else{Some(path)};
                    r.project_requires_save_as=requires_save_as;
                    r.state.dirty=false;r.clear_operation_progress();r.state.error=history_notice(warnings);r.state.diagnostics=None;
                    r.workbench.update(cx,|workbench,_|workbench.reset_interaction());
                    let index=r.state.current_index;r.select_photo(index,cx);
                },Err(CoreError::Cancelled)=>r.finish_cancelled("已取消打开工程，保留当前工作区",cx),Err(e)=>r.finish_error(e.to_string(),cx)}
            });
        }).detach();
    }
    fn save_or_prompt(&mut self,save_as:bool,cx:&mut Context<Self>) {
        if self.state.documents.is_empty(){self.finish_error("没有可保存的照片".into(),cx);return;}
        if !save_as && !self.project_requires_save_as {if let Some(path)=self.project_path.clone(){self.save_to(path,cx);return;}}
        let id=self.begin_operation("命名并保存工程",cx);
        let name=if self.project_requires_save_as {format!("{}-0.8.chromora",self.project_name)}else{
            self.project_path.as_ref().and_then(|p|p.file_name()).map(|s|s.to_string_lossy().into_owned()).unwrap_or_else(||self.suggested_name("chromora"))};
        let receiver=cx.prompt_for_new_path(&self.storage.projects,Some(&name));
        cx.spawn(async move|root,cx|{
            let result=receiver.await;
            let _=root.update(cx,|r,cx|{
                if id!=r.operation_id{return;}
                match result {Ok(Ok(Some(path)))=>r.save_to(path,cx),Ok(Ok(None))=>r.finish_cancelled("已取消保存",cx),
                    Ok(Err(e))=>r.finish_error(e.to_string(),cx),Err(e)=>r.finish_error(e.to_string(),cx)}
            });
        }).detach();
    }
    fn save_to(&mut self,path:PathBuf,cx:&mut Context<Self>) {
        let name=path.file_stem().map(|s|s.to_string_lossy().into_owned()).unwrap_or_else(||self.project_name.clone());
        let id=self.begin_operation("正在保存照片参数及撤销历史",cx);let handle=self.window_handle;
        let workspace=WorkspaceDocument{name:name.clone(),photos:self.state.documents.clone(),current_index:self.state.current_index};
        let histories=self.state.histories.iter().map(EditHistory::to_project).collect::<Vec<_>>();
        cx.spawn(async move|root,cx|{
            let result=cx.background_spawn(async move{let warnings=core::save_workspace_with_history(&workspace,&histories,&path)?;Ok::<_,CoreError>((path,warnings))}).await;
            let _=handle.update(cx,|_,window,app|{let _=root.update(app,|r,cx|{
                if id!=r.operation_id{return;}
                match result {Ok((path,warnings))=>{r.project_path=Some(path);r.project_name=name;r.state.dirty=false;r.state.busy=false;
                    r.project_requires_save_as=false;
                    r.state.error=history_notice(warnings);r.state.status="工程已保存（含撤销历史）".into();r.sync_workbench(cx);
                    if let Some(a)=r.pending_action.take(){r.perform_transition(a,window,cx);}},Err(e)=>r.finish_error(e.to_string(),cx)}
            });});
        }).detach();
    }
    fn import_lut(&mut self,path:PathBuf,id:u64,cx:&mut Context<Self>) {
        let directory=self.storage.root.join("数据").join("风格");
        cx.spawn(async move|root,cx|{
            let result=cx.background_spawn(async move{
                core::load_cube(&path)?;std::fs::create_dir_all(&directory)?;
                let name=path.file_name().unwrap_or_default();let destination=directory.join(name);
                if path!=destination {
                    if destination.exists() && std::fs::read(&path)?!=std::fs::read(&destination)? {
                        return Err(CoreError::Io(std::io::Error::new(std::io::ErrorKind::AlreadyExists,"已有同名但内容不同的风格，请先重命名")));
                    }
                    if !destination.exists(){std::fs::copy(&path,&destination)?;}
                }Ok::<_,CoreError>(destination)
            }).await;
            let _=root.update(cx,|r,cx|{
                if id!=r.operation_id{return;}r.state.busy=false;
                match result{Ok(path)=>{if !r.state.preferences.luts.contains(&path){r.state.preferences.luts.push(path.clone());}
                    r.save_preferences();r.select_lut(Some(path),cx);
                    if r.state.current().is_some(){r.schedule_style_previews(cx,false);}
                },Err(e)=>r.finish_error(e.to_string(),cx)}
            });
        }).detach();
    }
    fn select_lut(&mut self,path:Option<PathBuf>,cx:&mut Context<Self>) {
        let Some(doc)=self.state.documents.get_mut(self.state.current_index) else{self.sync_workbench(cx);return;};
        let before=EditState::capture(doc);
        match core::set_lut(doc,path.as_deref()){Ok(())=>{let current=EditState::capture(doc);
            self.state.histories.resize_with(self.state.documents.len(),EditHistory::default);
            self.state.histories[self.state.current_index].changed(before,&current);
            self.state.dirty=true;self.schedule_preview(false,cx);},Err(e)=>self.finish_error(e.to_string(),cx)}
    }
    fn save_preferences(&mut self) {
        let path=self.storage.root.join("数据").join("工作台设置-rust.json");
        if let Err(e)=serde_json::to_vec_pretty(&self.state.preferences).map_err(std::io::Error::other).and_then(|b|std::fs::write(path,b)) {self.state.error=Some(format!("保存风格收藏失败：{e}"));}
    }
    fn save_preset(&mut self,path:PathBuf,portrait:bool,cx:&mut Context<Self>) {
        let result=core::save_preset(&self.state.documents,self.state.current_index,portrait,&path)
            .map_err(|error|error.to_string());
        self.state.busy=false;match result{Ok(())=>{self.state.status=format!("已保存预设 {}",path.display());self.refresh_presets(cx);self.sync_workbench(cx);},Err(e)=>self.finish_error(e,cx)}
    }
    fn open_preset(&mut self,path:PathBuf,portrait:bool,cx:&mut Context<Self>) {
        let result=self.state.current().ok_or_else(||CoreError::InvalidProject("请先打开照片".into()))
            .and_then(|doc|core::load_preset(&doc.settings,&path,portrait));
        self.state.busy=false;
        if result.is_ok(){
            self.state.last_preset=Some(path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
            self.state.preferences.recent_presets.retain(|p|p!=&path);
            self.state.preferences.recent_presets.insert(0,path.clone());self.state.preferences.recent_presets.truncate(8);self.save_preferences();
        }
        match result {
            Ok(settings) if self.state.current().is_some_and(|doc|doc.settings==settings)=>{
                self.state.status="预设与当前参数相同".into();self.state.error=None;self.sync_workbench(cx);
            },
            Ok(settings)=>self.edit_settings(|s|{*s=settings;Ok(())},cx),
            Err(error)=>self.finish_error(error.to_string(),cx),
        }
    }
    fn refresh_presets(&mut self,cx:&mut Context<Self>){
        let directory=self.storage.root.join("数据").join("预设");
        cx.spawn(async move|root,cx|{
            let result=cx.background_spawn(async move{core::discover_presets(&directory)}).await;
            let _=root.update(cx,|r,cx|{
                match result{Ok(entries)=>r.state.presets=entries,Err(e)=>{if !r.state.busy{r.state.error=Some(format!("读取预设目录失败：{e}"));}}}
                r.sync_workbench(cx);
                if r.state.current().is_some(){r.schedule_style_previews(cx,false);}
            });
        }).detach();
    }
    fn export(&mut self,path:PathBuf,format:ExportFormat,id:u64,cx:&mut Context<Self>) {
        let Some(doc)=self.state.current().cloned() else{self.finish_error("请先打开照片".into(),cx);return;};
        let mut options=self.state.export_options.clone();
        if matches!(format,ExportFormat::Psd|ExportFormat::Patchy){options.max_long_edge=None;}
        self.state.status=format!("正在导出 {}",format.label());self.sync_workbench(cx);
        let cancel=if format==ExportFormat::Patchy{core::RenderCancellation::new()}else{
            self.enable_operation_progress(id,OperationProgress{total:1,current_file:path.file_name().unwrap_or_default().to_string_lossy().into_owned(),..Default::default()},cx).0};
        cx.spawn(async move|root,cx|{
            let result=cx.background_spawn(async move{
                if let Some(kind)=format.kind(){core::export_with_options(&doc,&path,kind,&options,&cancel)?;}
                else{core::open_in_patchy(&doc,&path)?;}
                Ok::<_,CoreError>(path)
            }).await;
            let _=root.update(cx,|r,cx|{if id!=r.operation_id{return;}
                match result{Ok(path)=>{r.clear_operation_progress();r.state.error=None;r.state.status=format!("已导出 {}",path.display());r.sync_workbench(cx);},Err(CoreError::Cancelled)=>r.finish_cancelled("已取消导出，未发布成片",cx),Err(e)=>r.finish_error(e.to_string(),cx)}
            });
        }).detach();
    }
    fn export_all(&mut self,directory:PathBuf,id:u64,cx:&mut Context<Self>){
        let format=self.state.export_format;
        if format==ExportFormat::Patchy{self.finish_error("Patchy 交接仅支持当前照片".into(),cx);return;}
        let mut options=self.state.export_options.clone();if format==ExportFormat::Psd{options.max_long_edge=None;}
        let documents=self.state.documents.clone();
        if documents.is_empty(){self.finish_error("请先打开照片".into(),cx);return;}
        let total=documents.len();self.state.status=format!("批量导出 {}，已完成照片保留",format.label());
        let (cancel,progress)=self.enable_operation_progress(id,OperationProgress{total,..Default::default()},cx);
        cx.spawn(async move|root,cx|{
            let result=cx.background_spawn(async move{
                run_export_batch(documents,&directory,format,&options,&cancel,|value|{if let Ok(mut latest)=progress.lock(){*latest=value;}})
            }).await;
            let _=root.update(cx,|r,cx|{if id!=r.operation_id{return;}match result{
                Ok(batch)=>{r.clear_operation_progress();r.state.status=format!("{}：成功 {} 张，失败 {} 张；已完成成片保留",if batch.cancelled{"已取消批量导出"}else{"批量导出完成"},batch.completed-batch.errors.len(),batch.errors.len());r.state.error=history_notice(batch.errors);r.sync_workbench(cx);},
                Err(CoreError::Cancelled)=>r.finish_cancelled("已取消批量导出，已完成成片保留",cx),Err(e)=>r.finish_error(e.to_string(),cx)
            }});
        }).detach();
    }
    fn suggested_name(&self,extension:&str)->String {
        let stem=self.state.current().and_then(|d|d.source_path.file_stem()).map(|s|s.to_string_lossy().into_owned()).unwrap_or_else(||"Chromora".into());
        format!("{stem}_编辑.{extension}")
    }
    fn begin_operation(&mut self,status:&str,cx:&mut Context<Self>)->u64 {
        if !self.state.crop_editing {self.end_edit(cx);}
        if let Some(cancel)=self.operation_cancel.take(){cancel.cancel();}
        self.state.progress=None;self.state.can_cancel_operation=false;
        self.operation_id=self.operation_id.wrapping_add(1);self.state.busy=true;self.state.error=None;
        self.state.status=status.into();self.sync_workbench(cx);self.operation_id
    }
    fn enable_operation_progress(&mut self,id:u64,initial:OperationProgress,cx:&mut Context<Self>)->(core::RenderCancellation,Arc<Mutex<OperationProgress>>) {
        let cancel=core::RenderCancellation::new();
        let progress=Arc::new(Mutex::new(initial.clone()));
        self.operation_cancel=Some(cancel.clone());self.state.progress=Some(initial);self.state.can_cancel_operation=true;
        self.sync_workbench(cx);
        let observed=progress.clone();
        cx.spawn(async move|root,cx|{
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let active=root.update(cx,|r,cx|{
                    if id!=r.operation_id||r.operation_cancel.is_none(){return false;}
                    if let Ok(latest)=observed.lock(){if r.state.progress.as_ref()!=Some(&*latest){
                        r.state.progress=Some(latest.clone());r.sync_workbench(cx);
                    }}
                    true
                }).unwrap_or(false);
                if !active{return;}
            }
        }).detach();
        (cancel,progress)
    }
    fn cancel_operation(&mut self,cx:&mut Context<Self>) {
        if self.state.can_cancel_operation {
            if let Some(cancel)=&self.operation_cancel{cancel.cancel();}
            self.state.can_cancel_operation=false;
            self.state.status="正在取消，等待当前文件处理完成".into();self.sync_workbench(cx);
        }
    }
    fn clear_operation_progress(&mut self) {
        self.operation_cancel=None;self.state.progress=None;self.state.can_cancel_operation=false;self.state.busy=false;
    }
    fn finish_cancelled(&mut self,status:&str,cx:&mut Context<Self>) {self.pending_action=None;self.clear_operation_progress();self.state.status=status.into();self.sync_workbench(cx);}
    fn finish_error(&mut self,error:String,cx:&mut Context<Self>) {self.pending_action=None;self.clear_operation_progress();self.state.status="操作失败".into();self.state.error=Some(error);self.sync_workbench(cx);}
    fn current_photo_key(&self)->Option<String>{
        self.state.current().map(|doc|format!("{}:{}:{}",self.state.current_index,doc.source_sha256,doc.source_path.display()))
    }
    fn cancel_style_previews(&mut self){
        if let Some(cancel)=self.style_preview_cancel.take(){cancel.cancel();}
        self.style_preview_id=self.style_preview_id.wrapping_add(1);
        self.style_preview_photo=None;
        self.state.style_previews.clear();
    }
    /// Builds style-row thumbnails off the UI thread. `clear_now` drops the previous photo's icons immediately.
    fn schedule_style_previews(&mut self,cx:&mut Context<Self>,clear_now:bool){
        let Some(identity)=self.current_photo_key() else{self.cancel_style_previews();self.sync_workbench(cx);return;};
        if clear_now && self.style_preview_photo.as_ref()==Some(&identity) && !self.state.style_previews.is_empty(){return;}
        if let Some(cancel)=self.style_preview_cancel.take(){cancel.cancel();}
        self.style_preview_id=self.style_preview_id.wrapping_add(1);
        let id=self.style_preview_id;
        if clear_now{self.state.style_previews.clear();self.style_preview_photo=None;self.sync_workbench(cx);}
        let doc=self.state.current().expect("photo key requires a document").clone();
        let presets:Vec<PathBuf>=self.state.presets.iter().filter(|entry|!entry.portrait).map(|entry|entry.path.clone()).collect();
        let luts=self.state.preferences.luts.clone();
        let cancel=core::RenderCancellation::new();
        self.style_preview_cancel=Some(cancel.clone());
        cx.spawn(async move|root,cx|{
            let result=cx.background_spawn(async move{
                cancel.check()?;
                let small=core::thumbnail_document(&doc,core::STYLE_PREVIEW_LONG_EDGE)?;
                drop(doc);
                cancel.check()?;
                let mut images=BTreeMap::new();
                for path in presets{
                    cancel.check()?;
                    if let Ok(rgb)=core::render_preset_style_preview(&small,&path){
                        images.insert(core::style_preview_key("preset",&path),rgb);
                    }
                }
                for path in luts{
                    cancel.check()?;
                    if let Ok(rgb)=core::render_lut_style_preview(&small,&path){
                        images.insert(core::style_preview_key("lut",&path),rgb);
                    }
                }
                cancel.check()?;
                Ok::<_,CoreError>(images)
            }).await;
            let _=root.update(cx,|r,cx|{
                if id!=r.style_preview_id{return;}
                r.style_preview_cancel=None;
                match result{
                    Ok(images)=>{
                        if r.current_photo_key().as_ref()!=Some(&identity){return;}
                        r.style_preview_photo=Some(identity);
                        r.state.style_previews=images.into_iter().map(|(key,rgb)|(key,to_render_image(rgb))).collect();
                    }
                    Err(CoreError::Cancelled)=>{}
                    Err(_)=>if clear_now{r.state.style_previews.clear();r.style_preview_photo=None;}
                }
                r.sync_workbench(cx);
            });
        }).detach();
    }
    fn confirm_clear_photos(&mut self,window:&mut Window,cx:&mut Context<Self>){
        if self.state.documents.is_empty(){return;}
        let id=self.begin_operation("请确认清空照片",cx);
        let receiver=window.prompt(PromptLevel::Warning,"清空当前照片？",Some("照片条会清空，风格图标恢复为色块。未保存的调整不会写入磁盘上的工程。"),
            &[PromptButton::ok("清空"),PromptButton::cancel("取消")],cx);
        let handle=self.window_handle;
        cx.spawn(async move|root,cx|{
            let answer=receiver.await;
            let _=handle.update(cx,|_,_,app|{let _=root.update(app,|r,cx|{
                if id!=r.operation_id{return;}
                match answer{Ok(0)=>r.clear_photos(cx),_=>r.finish_cancelled("已保留当前照片",cx)}
            });});
        }).detach();
    }
    fn clear_photos(&mut self,cx:&mut Context<Self>){
        if let Some(cancel)=self.render_cancel.take(){cancel.cancel();}
        self.render_id=self.render_id.wrapping_add(1);
        self.cancel_style_previews();
        self.clear_operation_progress();
        self.state.documents.clear();self.state.thumbnails.clear();self.state.histories.clear();
        self.state.current_index=0;self.state.preview=None;self.state.original_preview=None;self.state.diagnostics=None;
        self.state.rendering=false;self.state.crop_editing=false;self.state.selected_mask=None;self.state.peeking=false;self.state.compare=false;
        self.state.dirty=false;self.state.error=None;
        self.project_path=None;self.project_name="未命名工程".into();self.project_requires_save_as=false;
        self.state.status="打开照片，或将照片拖入画布".into();
        self.workbench.update(cx,|workbench,_|workbench.reset_interaction());
        self.sync_workbench(cx);
    }
    fn sync_workbench(&mut self,cx:&mut Context<Self>) {self.state.project_name=self.project_name.clone();self.state.project_path=self.project_path.clone();let snapshot=self.state.snapshot();self.workbench.update(cx,|v,cx|{v.set_snapshot(snapshot);cx.notify();});cx.notify();}
}
impl Render for AppRoot {fn render(&mut self,_:&mut Window,_:&mut Context<Self>)->impl IntoElement{self.workbench.clone()}}
fn photo_name(doc:&PhotoDocument)->String{doc.source_path.file_name().map(|s|s.to_string_lossy().into_owned()).unwrap_or_else(||"照片".into())}
fn photo_is_adjusted(doc:&PhotoDocument)->bool{doc.settings!=AdjustmentSettings::default()||doc.lut_path.is_some()}
fn thumbnail_from_preview(image:&RgbImage)->RgbImage{
    let edge=image.width().max(image.height());if edge<=240{return image.clone();}
    let scale=240. / f64::from(edge);
    image::imageops::thumbnail(image,(f64::from(image.width())*scale).round().max(1.) as u32,(f64::from(image.height())*scale).round().max(1.) as u32)
}
// Preserve a just-committed draft when a click carries a compound value from the previous UI snapshot.
fn rebase_action(action:AppAction,before:&AdjustmentSettings,current:&AdjustmentSettings)->AppAction{
    match action{
        AppAction::SetGeometry(mut geometry)=>{
            if geometry.crop==before.geometry.crop{geometry.crop=current.geometry.crop;}
            if geometry.quarter_turns==before.geometry.quarter_turns{geometry.quarter_turns=current.geometry.quarter_turns;}
            if geometry.straighten_degrees==before.geometry.straighten_degrees{geometry.straighten_degrees=current.geometry.straighten_degrees;}
            AppAction::SetGeometry(geometry)
        },
        AppAction::SetMasks(mut masks)=>{
            for mask in &mut masks{
                if let (Some(old),Some(now))=(before.masks.iter().find(|m|m.id==mask.id),current.masks.iter().find(|m|m.id==mask.id)){
                    if mask.name==old.name{mask.name=now.name.clone();}
                    if mask.enabled==old.enabled{mask.enabled=now.enabled;}
                    if mask.invert==old.invert{mask.invert=now.invert;}
                    if mask.shape==old.shape{mask.shape=now.shape;}
                    else if let (core::MaskShape::Radial{center,radius,feather},core::MaskShape::Radial{center:oc,radius:or,feather:of},core::MaskShape::Radial{center:nc,radius:nr,feather:nf})=(&mut mask.shape,old.shape,now.shape){
                        if *center==oc{*center=nc;}if *radius==or{*radius=nr;}if *feather==of{*feather=nf;}
                    }
                    for &(key,_,_) in core::LocalAdjustments::CONTROLS{
                        if mask.adjustments.value(key)==old.adjustments.value(key){mask.adjustments.set_value(key,now.adjustments.value(key).expect("known local key")).expect("current settings validated");}
                    }
                }
            }
            AppAction::SetMasks(masks)
        },other=>other,
    }
}
#[derive(Default)]
struct ExportBatch {completed:usize,errors:Vec<String>,cancelled:bool}
fn run_export_batch(documents:Vec<PhotoDocument>,directory:&Path,format:ExportFormat,options:&core::ExportOptions,
    cancel:&core::RenderCancellation,mut report:impl FnMut(OperationProgress))->CoreResult<ExportBatch>{
    let kind=format.kind().ok_or_else(||CoreError::InvalidExport("Patchy 交接仅支持当前照片".into()))?;
    options.validate(kind)?;cancel.check()?;
    std::fs::create_dir_all(directory)?;
    let total=documents.len();let mut batch=ExportBatch::default();
    for (index,doc) in documents.iter().enumerate(){
        if cancel.check().is_err(){batch.cancelled=true;break;}
        let stem=doc.source_path.file_stem().unwrap_or_default().to_string_lossy();
        let filename=format!("{:04}_{}_编辑.{}",index+1,stem,format.extension());
        report(OperationProgress{completed:batch.completed,total,failed:batch.errors.len(),current_file:filename.clone()});
        let destination=directory.join(&filename);
        match core::protect_export_destinations(&documents,&destination,cancel).and_then(|_|core::export_with_options(doc,&destination,kind,options,cancel)){
            Ok(())=>{},Err(CoreError::Cancelled)=>{batch.cancelled=true;break;},
            Err(error)=>batch.errors.push(format!("{filename}：{error}")),
        }
        batch.completed+=1;
        report(OperationProgress{completed:batch.completed,total,failed:batch.errors.len(),current_file:filename});
    }
    batch.cancelled|=cancel.check().is_err();Ok(batch)
}
fn to_render_image(image:RgbImage)->Arc<RenderImage>{
    let mut rgba=RgbaImage::new(image.width(),image.height());
    for (dst,src) in rgba.pixels_mut().zip(image.pixels()){*dst=Rgba([src[2],src[1],src[0],255]);}
    Arc::new(RenderImage::new(vec![ImageFrame::new(rgba)]))
}
fn history_notice(warnings:Vec<String>)->Option<String>{if warnings.is_empty(){None}else{Some(format!("工程提示：{}",warnings.join("\n")))}}
fn supported_path(path:&Path)->bool{
    core::is_raw_path(path) || path.extension().and_then(|s|s.to_str()).is_some_and(|s|matches!(s.to_ascii_lowercase().as_str(),"jpg"|"jpeg"|"png"|"tif"|"tiff"))
}
#[derive(Default)]
struct ImportBatch {
    documents:Vec<PhotoDocument>, thumbnails:Vec<PhotoThumbnail>, errors:Vec<String>, cancelled:bool,
}
fn check_import_capacity(existing:usize,selected:usize)->CoreResult<()> {
    if existing>core::MAX_WORKSPACE_PHOTOS || selected>core::MAX_WORKSPACE_PHOTOS.saturating_sub(existing) {
        return Err(CoreError::InvalidProject(format!("工程最多支持 {} 张照片，当前已有 {existing} 张，本次选择 {selected} 张；请减少选择或另建工程",core::MAX_WORKSPACE_PHOTOS)));
    }
    Ok(())
}
fn run_import_batch(paths:Vec<PathBuf>,directory:&Path,existing:usize,cancel:&core::RenderCancellation,
    mut report:impl FnMut(OperationProgress))->CoreResult<ImportBatch> {
    let mut batch=ImportBatch::default();let mut files=vec![];
    for path in paths {
        if let Err(error)=collect_images_cancellable(&path,&mut files,&mut batch.errors,cancel) {
            if matches!(&error,CoreError::Cancelled){batch.cancelled=true;return Ok(batch);}return Err(error);
        }
    }
    files.sort();files.dedup();
    if cancel.check().is_err(){batch.cancelled=true;return Ok(batch);}
    check_import_capacity(existing,files.len())?;
    let total=files.len();let mut completed=0;let mut failed=0;
    for path in files {
        if cancel.check().is_err(){batch.cancelled=true;break;}
        let current_file=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        report(OperationProgress{completed,total,failed,current_file:current_file.clone()});
        match core::import_image(&path,directory).and_then(|mut doc|{
            core::prepare_thumbnail(&mut doc)?;
            let thumbnail=doc.thumbnail_rgb.as_ref().unwrap().as_ref().clone();
            let thumb=PhotoThumbnail{name:photo_name(&doc),preview:to_render_image(thumbnail),adjusted:photo_is_adjusted(&doc)};
            if !batch.documents.is_empty(){doc.evict_pixels();}
            Ok((doc,thumb))
        }) {
            Ok((doc,thumb))=>{batch.documents.push(doc);batch.thumbnails.push(thumb);},
            Err(error)=>{failed+=1;batch.errors.push(format!("{}：{error}",path.display()));},
        }
        completed+=1;
        report(OperationProgress{completed,total,failed,current_file});
    }
    if cancel.check().is_err(){batch.cancelled=true;}
    Ok(batch)
}
#[cfg(test)]
fn collect_images(path:&Path,files:&mut Vec<PathBuf>,errors:&mut Vec<String>){
    collect_images_cancellable(path,files,errors,&core::RenderCancellation::new()).expect("uncancelled scan");
}
fn collect_images_cancellable(path:&Path,files:&mut Vec<PathBuf>,errors:&mut Vec<String>,cancel:&core::RenderCancellation)->CoreResult<()> {
    cancel.check()?;
    if path.is_file(){if supported_path(path){files.push(path.to_path_buf());}return Ok(());}
    if path.is_dir(){match std::fs::read_dir(path){Ok(entries)=>for entry in entries{
        cancel.check()?;
        match entry{Ok(e)=>{match e.file_type(){Ok(t) if !t.is_symlink()=>collect_images_cancellable(&e.path(),files,errors,cancel)?,Ok(_)=>{},Err(e)=>errors.push(e.to_string())}},Err(e)=>errors.push(e.to_string())}
    },Err(e)=>errors.push(format!("{}：{e}",path.display()))}}
    Ok(())
}
pub fn run(){Application::new().run(|cx:&mut App|{
    let bounds=Bounds::centered(None,size(px(1440.),px(920.)),cx);
    cx.open_window(WindowOptions{window_bounds:Some(WindowBounds::Windowed(bounds)),window_min_size:Some(Size{width:px(1024.),height:px(650.)}),..Default::default()},
        |w,cx|{w.set_window_title("Chromora");cx.new(|cx|AppRoot::new(w,cx))}).expect("无法创建 Chromora 窗口");cx.activate(true);
});}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn committing_draft_preserves_other_click_changes(){
        let mut before=AdjustmentSettings::default();before.masks.push(LocalMask::default());
        let mut current=before.clone();current.geometry.straighten_degrees=4.;current.masks[0].name="脸部补光".into();current.masks[0].adjustments.exposure=0.5;
        let mut clicked=before.geometry;clicked.quarter_turns=1;
        let AppAction::SetGeometry(result)=rebase_action(AppAction::SetGeometry(clicked),&before,&current) else{panic!()};
        assert_eq!(result.straighten_degrees,4.);assert_eq!(result.quarter_turns,1);
        let mut masks=before.masks.clone();masks[0].enabled=false;
        let AppAction::SetMasks(result)=rebase_action(AppAction::SetMasks(masks),&before,&current) else{panic!()};
        assert_eq!(result[0].name,"脸部补光");assert_eq!(result[0].adjustments.exposure,0.5);assert!(!result[0].enabled);
    }
    #[test]fn batch_export_cancel_keeps_completed_outputs_only(){
        let temp=tempfile::tempdir().unwrap();let path=batch_source(temp.path(),"a.png");
        let doc=core::open_image(&path).unwrap();let dir=temp.path().join("out");let cancel=core::RenderCancellation::new();
        let batch=run_export_batch(vec![doc.clone(),doc],&dir,ExportFormat::Png,&core::ExportOptions::default(),&cancel,|progress|{if progress.completed==1{cancel.cancel();}}).unwrap();
        assert!(batch.cancelled);assert_eq!(batch.completed,1);assert!(batch.errors.is_empty());
        assert!(dir.join("0001_a_编辑.png").is_file());assert!(!dir.join("0002_a_编辑.png").exists());
    }
    #[test]fn batch_export_reports_conflict_and_continues_without_overwriting(){
        let temp=tempfile::tempdir().unwrap();let path=batch_source(temp.path(),"a.png");
        let doc=core::open_image(&path).unwrap();let dir=temp.path().join("out");std::fs::create_dir(&dir).unwrap();
        let existing=dir.join("0001_a_编辑.png");std::fs::write(&existing,b"keep").unwrap();
        let batch=run_export_batch(vec![doc.clone(),doc],&dir,ExportFormat::Png,&core::ExportOptions::default(),&core::RenderCancellation::new(),|_|{}).unwrap();
        assert!(!batch.cancelled);assert_eq!(batch.completed,2);assert_eq!(batch.errors.len(),1);
        assert_eq!(std::fs::read(existing).unwrap(),b"keep");assert!(dir.join("0002_a_编辑.png").is_file());
    }
    #[test]fn batch_export_preserves_other_photos_missing_original_path(){
        let temp=tempfile::tempdir().unwrap();let path=batch_source(temp.path(),"a.png");let first=core::open_image(&path).unwrap();
        let dir=temp.path().join("out");std::fs::create_dir(&dir).unwrap();let protected=batch_source(&dir,"0001_a_编辑.png");
        let second=core::import_image(&protected,&temp.path().join("originals")).unwrap();std::fs::remove_file(&protected).unwrap();
        let batch=run_export_batch(vec![first,second],&dir,ExportFormat::Png,&core::ExportOptions::default(),&core::RenderCancellation::new(),|_|{}).unwrap();
        assert_eq!(batch.completed,2);assert_eq!(batch.errors.len(),1);assert!(!protected.exists());assert!(!batch.cancelled);
        assert!(dir.join("0002_0001_a_编辑_编辑.png").is_file());
    }
    #[test]fn export_cancelled_before_start_creates_no_output_directory(){
        let temp=tempfile::tempdir().unwrap();let path=batch_source(temp.path(),"a.png");let doc=core::open_image(&path).unwrap();
        let directory=temp.path().join("out");let cancel=core::RenderCancellation::new();cancel.cancel();
        assert!(matches!(run_export_batch(vec![doc],&directory,ExportFormat::Png,&core::ExportOptions::default(),&cancel,|_|panic!()),Err(CoreError::Cancelled)));
        assert!(!directory.exists());
    }
    #[test]fn edited_thumbnail_reflects_rendered_color_and_geometry(){
        let temp=tempfile::tempdir().unwrap();let path=batch_source(temp.path(),"a.png");let mut doc=core::open_image(&path).unwrap();
        let original=core::render_preview_at(&doc,Some(1440)).unwrap();doc.settings.exposure=1.;
        let edited=core::render_preview_at(&doc,Some(1440)).unwrap();
        assert_ne!(thumbnail_from_preview(&original).as_raw(),thumbnail_from_preview(&edited).as_raw());assert!(photo_is_adjusted(&doc));
        doc.settings.geometry.crop=Some(core::NormalizedRect{x:0.,y:0.,width:0.5,height:1.});
        assert_eq!(thumbnail_from_preview(&core::render_preview_at(&doc,Some(1440)).unwrap()).dimensions(),(1,2));
    }
    #[test]fn folder_import_filters_supported_files_and_avoids_unrelated_data(){
        let dir=tempfile::tempdir().unwrap();std::fs::write(dir.path().join("a.JPG"),b"x").unwrap();
        std::fs::write(dir.path().join("secret.txt"),b"x").unwrap();std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested").join("b.NEF"),b"x").unwrap();
        let mut files=vec![];let mut errors=vec![];collect_images(dir.path(),&mut files,&mut errors);
        assert_eq!(files.len(),2);assert!(errors.is_empty());
    }
    #[test]fn raw_import_uses_the_same_extensions_as_the_decoder(){
        for extension in ["CRW","dcr","FFF","k25","mos","rwl","SR2","srf"] {
            let path=PathBuf::from(format!("camera.{extension}"));
            assert!(core::is_raw_path(&path));assert!(supported_path(&path));
        }
        assert!(!supported_path(Path::new("notes.txt")));
    }
    fn batch_source(root:&Path,name:&str)->PathBuf {
        let path=root.join(name);
        image::RgbImage::from_pixel(2,2,image::Rgb([100,120,140])).save(&path).unwrap();
        path
    }
    #[test]fn import_capacity_is_checked_before_creating_any_managed_copy(){
        assert!(check_import_capacity(core::MAX_WORKSPACE_PHOTOS-1,1).is_ok());
        assert!(check_import_capacity(core::MAX_WORKSPACE_PHOTOS-1,2).is_err());
        let temp=tempfile::tempdir().unwrap();
        let first=batch_source(temp.path(),"a.png");let second=batch_source(temp.path(),"b.png");
        let originals=temp.path().join("originals");
        let result=run_import_batch(vec![first.clone(),second.clone()],&originals,core::MAX_WORKSPACE_PHOTOS-1,&core::RenderCancellation::new(),|_|{});
        assert!(matches!(result,Err(CoreError::InvalidProject(_))));
        assert!(!originals.exists());assert!(first.is_file()&&second.is_file());
    }
    #[test]fn cancelling_import_keeps_the_completed_photo_and_stops_before_the_next_file(){
        let temp=tempfile::tempdir().unwrap();
        let first=batch_source(temp.path(),"a.png");let second=batch_source(temp.path(),"b.png");
        let originals=temp.path().join("originals");let cancel=core::RenderCancellation::new();
        let mut progress=Vec::new();
        let batch=run_import_batch(vec![first.clone(),second.clone()],&originals,0,&cancel,|value|{
            if value.completed==1{cancel.cancel();}progress.push(value);
        }).unwrap();
        assert!(batch.cancelled&&batch.errors.is_empty());
        assert_eq!(batch.documents.len(),1);assert_eq!(batch.thumbnails.len(),1);
        assert_eq!(std::fs::read_dir(&originals).unwrap().count(),1);
        assert_eq!(std::fs::read(batch.documents[0].original_copy_path.as_ref().unwrap()).unwrap(),std::fs::read(&first).unwrap());
        assert!(second.is_file());
        assert_eq!(progress.last().unwrap().completed,1);assert_eq!(progress.last().unwrap().total,2);
    }
    #[test]fn cancelling_before_scan_does_not_publish_an_import(){
        let temp=tempfile::tempdir().unwrap();let source=batch_source(temp.path(),"a.png");
        let originals=temp.path().join("originals");let cancel=core::RenderCancellation::new();cancel.cancel();
        let batch=run_import_batch(vec![source],&originals,0,&cancel,|_|panic!("cancelled scan reported a file")).unwrap();
        assert!(batch.cancelled&&batch.documents.is_empty());assert!(!originals.exists());
    }
    #[test]fn import_progress_counts_failed_files_and_retains_valid_files(){
        let temp=tempfile::tempdir().unwrap();let invalid=temp.path().join("a.png");
        std::fs::write(&invalid,b"invalid image").unwrap();let valid=batch_source(temp.path(),"b.png");
        let originals=temp.path().join("originals");let mut progress=OperationProgress::default();
        let batch=run_import_batch(vec![invalid,valid],&originals,0,&core::RenderCancellation::new(),|value|progress=value).unwrap();
        assert!(!batch.cancelled);assert_eq!(batch.documents.len(),1);assert_eq!(batch.errors.len(),1);
        assert_eq!((progress.completed,progress.failed,progress.total),(2,1,2));
    }
}
