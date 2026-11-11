use std::{ops::Range, rc::Rc};
use gpui::{App, Bounds, Context, ElementInputHandler, EntityInputHandler, FocusHandle,
    IntoElement, Pixels, Point, Render, UTF16Selection, Window, canvas, div, prelude::*, px};
use super::theme;

pub type Submit = Rc<dyn Fn(String, &mut Window, &mut App) -> Result<(), String>>;
pub type Cancel = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct InlineInput {
    pub focus: FocusHandle,
    text: String,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
    submit: Submit,
    cancel: Cancel,
    error: Option<String>,
}

impl InlineInput {
    pub fn new(text: String, submit: Submit, cancel: Cancel, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let end = text.len();
        let focus = cx.focus_handle();
        cx.on_blur(&focus, window, |this, window, cx| this.submit_deferred(None, window, cx)).detach();
        Self { focus, text, selection: 0..end, marked: None, submit, cancel, error: None }
    }
    pub fn text(&self) -> &str { &self.text }
    pub fn show_error(&mut self, error: String, cx: &mut Context<Self>) { self.error = Some(error); cx.notify(); }
    fn submit_deferred(&self, tab_backwards: Option<bool>, window: &mut Window, cx: &mut Context<Self>) {
        let submit = self.submit.clone(); let text = self.text.clone(); let input = cx.weak_entity();
        // The callback updates Workbench and may remove this input. Leave its entity update first.
        window.defer(cx, move |window, app| {
            match submit(text, window, app) {
                Ok(()) => if let Some(backwards) = tab_backwards {
                    if backwards { window.focus_prev(); } else { window.focus_next(); }
                },
                Err(error) => { let _ = input.update(app, |this, cx| {
                    if !error.is_empty() { this.show_error(error, cx); }
                    window.focus(&this.focus);
                }); },
            }
        });
    }
    fn utf8(&self, offset: usize) -> usize {
        let mut count = 0;
        for (index, ch) in self.text.char_indices() {
            if count >= offset { return index; }
            count += ch.len_utf16();
        }
        self.text.len()
    }
    fn utf16(&self, offset: usize) -> usize { self.text[..offset].encode_utf16().count() }
    fn from_utf16(&self, range: Range<usize>) -> Range<usize> { self.utf8(range.start)..self.utf8(range.end) }
    fn replace(&mut self, range: Range<usize>, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if self.text.len() - (range.end-range.start) + text.len() > 512 { return; }
        self.text.replace_range(range.clone(), &text);
        self.selection = range.start + text.len()..range.start + text.len();
        self.marked = None; self.error = None;
    }
    fn previous(&self, offset: usize) -> usize { self.text[..offset].char_indices().next_back().map(|(i,_)|i).unwrap_or(0) }
    fn next(&self, offset: usize) -> usize { self.text[offset..].chars().next().map(|c|offset+c.len_utf8()).unwrap_or(offset) }
}

impl Render for InlineInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity(); let focus = self.focus.clone();
        div().id("inline-input").track_focus(&self.focus).w_full().flex().flex_col().gap(px(4.))
            .child(div().relative().w_full().min_h(px(24.)).px(px(6.)).py(px(3.)).rounded(px(4.))
                .bg(theme::elevated()).border_1().border_color(if self.error.is_some(){theme::error()}else{theme::accent()})
                .child(div().truncate().text_size(px(12.)).text_color(theme::text())
                    .bg(if self.selection.is_empty(){theme::elevated()}else{gpui::Rgba{a:0.18,..theme::accent()}})
                    .child(if self.text.is_empty(){"│".to_string()}else{self.text.clone()}))
                .child(canvas(|_,_,_|{}, move |bounds,_,window,cx| {
                    window.handle_input(&focus, ElementInputHandler::new(bounds, view.clone()), cx);
                }).absolute().size_full()))
            .when_some(self.error.clone(), |d,e|d.child(div().text_xs().text_color(theme::error()).child(e)))
            .on_key_down(cx.listener(|this,event:&gpui::KeyDownEvent,window,cx| {
                let key=event.keystroke.key.to_ascii_lowercase();
                let control=event.keystroke.modifiers.control;
                if key=="enter" || key=="return" || key=="tab" {
                    this.submit_deferred((key=="tab").then_some(event.keystroke.modifiers.shift), window, cx);
                } else if key=="escape" {
                    let cancel=this.cancel.clone(); window.defer(cx,move|window,app|cancel(window,app));
                }
                else if control && key=="a" {this.selection=0..this.text.len();cx.notify();}
                else if control && key=="v" {
                    if let Some(text)=cx.read_from_clipboard().and_then(|c|c.text()){this.replace(this.selection.clone(),&text);cx.notify();}
                } else if key=="backspace" {
                    let range=if this.selection.is_empty(){this.previous(this.selection.start)..this.selection.end}else{this.selection.clone()};
                    this.replace(range,"");cx.notify();
                } else if key=="delete" {
                    let range=if this.selection.is_empty(){this.selection.start..this.next(this.selection.end)}else{this.selection.clone()};
                    this.replace(range,"");cx.notify();
                } else if key=="left" || key=="right" || key=="home" || key=="end" {
                    let at=match key.as_str(){"left"=>this.previous(this.selection.start),"right"=>this.next(this.selection.end),"home"=>0,_=>this.text.len()};
                    this.selection=at..at;this.marked=None;cx.notify();
                }
                // Text fields own keyboard shortcuts while editing, including Ctrl+Z.
                cx.stop_propagation();
            }))
    }
}

impl EntityInputHandler for InlineInput {
    fn text_for_range(&mut self, range:Range<usize>, actual:&mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>)->Option<String>{
        let range=self.from_utf16(range);*actual=Some(self.utf16(range.start)..self.utf16(range.end));Some(self.text[range].into())
    }
    fn selected_text_range(&mut self,_:bool,_:&mut Window,_:&mut Context<Self>)->Option<UTF16Selection>{
        Some(UTF16Selection{range:self.utf16(self.selection.start)..self.utf16(self.selection.end),reversed:false})
    }
    fn marked_text_range(&self,_:&mut Window,_:&mut Context<Self>)->Option<Range<usize>>{self.marked.as_ref().map(|r|self.utf16(r.start)..self.utf16(r.end))}
    fn unmark_text(&mut self,_:&mut Window,_:&mut Context<Self>){self.marked=None;}
    fn replace_text_in_range(&mut self,range:Option<Range<usize>>,text:&str,_:&mut Window,cx:&mut Context<Self>){
        let range=range.map(|r|self.from_utf16(r)).or_else(||self.marked.clone()).unwrap_or_else(||self.selection.clone());
        self.replace(range,text);cx.notify();
    }
    fn replace_and_mark_text_in_range(&mut self,range:Option<Range<usize>>,text:&str,selected:Option<Range<usize>>,_:&mut Window,cx:&mut Context<Self>){
        let range=range.map(|r|self.from_utf16(r)).or_else(||self.marked.clone()).unwrap_or_else(||self.selection.clone());
        let clean:String=text.chars().filter(|c|!c.is_control()).collect();
        if self.text.len()-(range.end-range.start)+clean.len()>512{return;}
        let start=range.start;self.replace(range,&clean);
        self.marked=if clean.is_empty(){None}else{Some(start..start+clean.len())};
        if let Some(r)=selected{
            let offset=|utf16:usize|{let mut count=0;for(index,ch)in clean.char_indices(){if count>=utf16{return index;}count+=ch.len_utf16();}clean.len()};
            self.selection=start+offset(r.start)..start+offset(r.end);
        }
        cx.notify();
    }
    fn bounds_for_range(&mut self,_:Range<usize>,bounds:Bounds<Pixels>,_:&mut Window,_:&mut Context<Self>)->Option<Bounds<Pixels>>{Some(bounds)}
    fn character_index_for_point(&mut self,_:Point<Pixels>,_:&mut Window,_:&mut Context<Self>)->Option<usize>{Some(self.text.encode_utf16().count())}
}
