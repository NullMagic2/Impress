//! Style editing dialog and docked Styles pane.
use super::*;

mod dialog {
//! Style dialogs edit a private draft; Cancel never changes the document.
//!
//! Modify Style keeps one draft `Style`. The main dialog's own controls (name,
//! base, following style, font, size, colour) are read into it on demand; the
//! toolbar and the Format ▾ dialogs (format_dialogs.rs) edit it directly.
use super::*;
use super::super::format_dialogs::{self, FormatItem, PendingFormat};

use model::{Style,StyleKind,StyleId,StyleSheet};
use wxdragon::font::{Font,FontFamily,FontStyle,FontWeight};
use wxdragon::{BrushStyle,DeviceContext,PenStyle,dc::AutoBufferedPaintDC,window::BackgroundStyle,widgets::panel::Panel,
    widgets::BitmapComboBox,event::window_events::WindowEvents};
use crate::StyleEditorUi;
use wxdragon::bitmap::Bitmap;

fn string_value(value:&str)->Option<String>{let value=value.trim();(!value.is_empty()).then(||value.to_owned())}
fn combo_value(value:&str)->Option<String>{if value.trim()=="Inherit from base"{None}else{string_value(value)}}
fn number_value(value:&str,label:&str)->Result<Option<f32>,String>{
    let value=value.trim();if value.is_empty(){return Ok(None);}
    let number=value.parse::<f32>().map_err(|_|format!("{label} must be a number"))?;
    if !number.is_finite(){return Err(format!("{label} must be finite"));}Ok(Some(number))
}
fn valid_color(value:&Option<String>,label:&str,allow_none:bool)->Result<(),String>{
    if let Some(value)=value{if !(allow_none&&value=="none")&&!(value.len()==7&&value.starts_with('#')&&value[1..].bytes().all(|b|b.is_ascii_hexdigit())){
        return Err(format!("{label}: use #RRGGBB{}",if allow_none{" or none"}else{""}));}}
    Ok(())
}
/// Colour palette for the swatch drop-downs (Word's standard colours, with the
/// interface accent). Item labels are the names; custom colours are listed as
/// "Custom #RRGGBB".
pub(crate) const FONT_COLOURS:[(&str,&str);14]=[
    ("Black","#000000"),("Dark gray","#404040"),("Gray","#7F7F7F"),("Dark blue","#1F3864"),("Blue","#2F5597"),
    ("Accent blue","#3A68C2"),("Teal","#12877F"),("Green","#548235"),("Dark red","#C00000"),("Red","#FF0000"),
    ("Orange","#ED7D31"),("Gold","#BF9000"),("Purple","#7030A0"),("White","#FFFFFF"),
];
pub(crate) const AUTOMATIC:&str="Automatic";
/// The "no colour" entry of fill / highlight drop-downs.
pub(crate) const NO_COLOUR:&str="No Color";
const MORE_COLOURS:&str="More colors…";
/// The #RRGGBB of a colour drop-down label; Automatic / No Color is None.
pub(crate) fn colour_value(label:&str)->Option<String>{
    let canonical=crate::i18n::source(label);let label=canonical.trim();
    if label==AUTOMATIC||label==NO_COLOUR||label==MORE_COLOURS||label.is_empty(){return None;}
    if let Some(i)=label.find('#'){return Some(label[i..].to_ascii_uppercase());}
    FONT_COLOURS.iter().find(|(name,_)|*name==label).map(|(_,hex)|hex.to_string())
}
/// Drop-down label for a stored colour: a palette name, else "Custom #RRGGBB".
pub(crate) fn colour_label(value:Option<&str>)->String{colour_label_or(value,AUTOMATIC)}
fn colour_label_or(value:Option<&str>,none:&str)->String{
    match value{None=>none.into(),Some(v)=>FONT_COLOURS.iter().find(|(_,hex)|hex.eq_ignore_ascii_case(v))
        .map(|(name,_)|name.to_string()).unwrap_or_else(||format!("Custom {}",v.to_ascii_uppercase()))}
}
/// A 1-DIP-framed colour chip for the drop-down, at the window's DPI.
/// `None` draws the Automatic chip (black) or, with `empty`, a crossed chip.
fn colour_swatch(window:&impl WxWidget,hex:Option<&str>,empty:bool)->Option<Bitmap>{
    let size=window.from_dip(Size::new(18,12));let (w,h)=(size.width.max(4) as u32,size.height.max(4) as u32);
    let frame=window.from_dip(Size::new(1,1)).width.max(1) as u32;
    let n=hex.and_then(|h|u32::from_str_radix(h.trim_start_matches('#'),16).ok());
    let fill=match n{Some(n)=>[(n>>16) as u8,(n>>8) as u8,n as u8],None if empty=>[255,255,255],None=>[0,0,0]};
    let edge=if hex.is_none()&&!empty{[0,0,0]}else{[fill[0]/4*3,fill[1]/4*3,fill[2]/4*3].map(|c|c.min(200))};
    let mut rgba=Vec::with_capacity((w*h*4) as usize);
    for y in 0..h{for x in 0..w{
        let border=x<frame||y<frame||x>=w-frame||y>=h-frame;
        // "No Color": a white chip with a red diagonal, as in Word.
        let slash=empty&&hex.is_none()&&((x as f32/w as f32)-(1.0-y as f32/h as f32)).abs()<0.09;
        let c=if border{if fill==[255,255,255]{[170,178,190]}else{edge}}else if slash{[196,43,28]}else{fill};
        rgba.extend_from_slice(&[c[0],c[1],c[2],255]);
    }}
    Bitmap::from_rgba(&rgba,w,h)
}
/// Fill a colour drop-down (first entry `none`: Automatic or No Color) and
/// select `current`, adding it as a custom colour when it is not in the palette.
pub(crate) fn fill_colour_list(combo:BitmapComboBox,window:&impl WxWidget,current:Option<&str>,none:&str){
    combo.clear();
    combo.append(&crate::i18n::text(none),colour_swatch(window,None,none==NO_COLOUR).as_ref());
    for (name,hex) in FONT_COLOURS{combo.append(&crate::i18n::text(name),colour_swatch(window,Some(hex),false).as_ref());}
    let label=colour_label_or(current,none);
    if label.starts_with("Custom "){combo.append(&crate::i18n::text(&label),colour_swatch(window,current,false).as_ref());}
    combo.append(&crate::i18n::text(MORE_COLOURS),None);
    let index=(0..combo.get_count()).find(|&i|combo.get_string(i)==crate::i18n::text(&label)).unwrap_or(0);
    combo.set_selection(index as i32);
}
/// A palette pick applies at once; "More colors…" opens the colour dialog and
/// adds the result as a custom swatch above it (Cancel restores the previous
/// choice). `changed` runs after every effective change.
pub(crate) fn bind_colour_list(combo:BitmapComboBox,parent:Dialog,changed:impl Fn()+'static){
    let previous=Rc::new(Cell::new(combo.get_selection()));
    combo.on_selection_changed(move |_|{
        if crate::i18n::source(&combo.get_value())==MORE_COLOURS{
            let start=combo.get_string(previous.get().max(0) as u32);
            let initial=colour_value(&start).and_then(|h|u32::from_str_radix(h.trim_start_matches('#'),16).ok())
                .map(|n|Colour::rgb((n>>16) as u8,(n>>8) as u8,n as u8)).unwrap_or(Colour::rgb(0,0,0));
            let dialog=ColourDialog::builder(&parent).with_title(&crate::i18n::text(&("Colors"))).with_initial_colour(initial).with_choose_full(true).build();
            let picked=(dialog.show_modal()==ID_OK).then(||dialog.get_colour()).flatten();
            match picked{
                Some(c)=>{
                    let hex=format!("#{:02X}{:02X}{:02X}",c.r,c.g,c.b);let label=colour_label(Some(&hex));
                    let existing=(0..combo.get_count()).find(|&i|combo.get_string(i)==crate::i18n::text(&label));
                    let index=existing.unwrap_or_else(||{let at=combo.get_count().saturating_sub(1);
                        combo.insert(&label,colour_swatch(&parent,Some(&hex),false).as_ref(),at as usize);at});
                    combo.set_selection(index as i32);
                }
                None=>combo.set_selection(previous.get()),
            }
        }
        previous.set(combo.get_selection());
        changed();
    });
}
/// Apply the main dialog's own controls (base, following style, font, size,
/// colour) to `draft`, without validation: the Format dialogs and the preview
/// start from this.
fn following_style(ui:&StyleEditorUi,sheet:&StyleSheet)->Option<StyleId> {
    let index=ui.style_next.get_selection()? as usize;
    if index==0 {return None;}
    let mut styles=sheet.styles.values().filter(|style|style.kind==StyleKind::Paragraph).collect::<Vec<_>>();
    styles.sort_by(|a,b|a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    styles.get(index-1).map(|style|style.id.clone())
}
fn merge_controls(ui:&StyleEditorUi,draft:&mut Style,base_ids:&[Option<StyleId>],sheet:&StyleSheet){
    let base=ui.style_base.get_selection().unwrap_or(0) as usize;
    if let Some(id)=base_ids.get(base){draft.based_on=id.clone();}
    if draft.kind==StyleKind::Paragraph{
        draft.next_style=following_style(ui,sheet);
    }
    merge_font_controls(ui,draft,sheet);
}

/// Read displayed inherited values against the current base before changing it.
fn merge_font_controls(ui:&StyleEditorUi,draft:&mut Style,sheet:&StyleSheet){
    let inherited=sheet.resolve_character(draft.based_on.as_ref());
    let family=crate::i18n::choice_source(&ui.style_font_family.get_value(), &["Inherit from base"]);
    let family=if draft.character.font_family.is_none()&&inherited.font_family.as_deref()==Some(family.trim()){None}else{combo_value(&family)};
    if family!=draft.character.font_family{draft.character.set_font_family(family);}
    let size=crate::i18n::choice_source(&ui.style_font_size.get_value(), &["Inherit from base"]);
    draft.character.font_size_pt=if size.trim()=="Inherit from base"||(draft.character.font_size_pt.is_none()&&inherited.font_size_pt.is_some_and(|n|n.to_string()==size.trim())){None}
        else{size.trim().parse::<f32>().ok().filter(|n|n.is_finite()).or(draft.character.font_size_pt)};
    draft.character.color=colour_value(&crate::i18n::source(&ui.style_font_color.get_value()));
}
/// Show the draft's font, size and colour in the main dialog's controls
/// (inherited values when the style does not set them).
fn sync_controls(ui:&StyleEditorUi,draft:&Style,sheet:&StyleSheet){
    let resolved=sheet.resolve_character(draft.based_on.as_ref());
    ui.style_font_family.change_value(&draft.character.font_family.as_ref().or(resolved.font_family.as_ref()).cloned().unwrap_or_else(||crate::i18n::text("Inherit from base")));
    ui.style_font_size.change_value(&draft.character.font_size_pt.or(resolved.font_size_pt).map(|n|n.to_string()).unwrap_or_else(||crate::i18n::text("Inherit from base")));
    fill_colour_list(ui.style_font_color,&ui.style_editor,draft.character.color.as_deref(),AUTOMATIC);
    ui.style_color_display.refresh(false,None);
}

/// The main Modify Style color control is painted as a single rounded field.
/// Its hidden bitmap combo retains the selected value for the existing style
/// draft, while this surface presents the compact palette shown in the design.
fn install_style_color_display(ui:&StyleEditorUi){
    let panel=ui.style_color_display;
    panel.set_background_style(BackgroundStyle::Paint);
    let choices=ui.style_font_color;
    panel.on_paint(move |_|{
        let dc=AutoBufferedPaintDC::new(&panel);
        let size=panel.get_client_size();let dip=|n:i32|panel.from_dip(Size::new(n,n)).width;
        dc.set_background(Colour::rgb(255,255,255));dc.clear();
        dc.set_brush(Colour::rgb(255,255,255),BrushStyle::Solid);
        dc.set_pen(if panel.has_focus(){Colour::rgb(58,104,194)}else{Colour::rgb(202,211,223)},dip(1).max(1),PenStyle::Solid);
        dc.draw_rounded_rectangle(0,0,size.width-1,size.height-1,dip(5) as f64);
        let value=choices.get_value();
        let color=colour_value(&value).and_then(|hex|hex_colour(&hex)).unwrap_or(Colour::rgb(0,0,0));
        let chip=(dip(9), (size.height-dip(12))/2,dip(18),dip(12));
        dc.set_brush(color,BrushStyle::Solid);dc.set_pen(if value=="White"{Colour::rgb(202,211,223)}else{color},dip(1).max(1),PenStyle::Solid);
        dc.draw_rectangle(chip.0,chip.1,chip.2,chip.3);
        if let Some(font)=Font::new_with_details(10,FontFamily::Default as i32,FontStyle::Normal as i32,
            FontWeight::Normal as i32,false,"Segoe UI"){dc.set_font(&font);}
        dc.set_text_foreground(Colour::rgb(36,48,66));
        let label=if value.is_empty(){AUTOMATIC}else{value.as_str()};
        let (_,th)=dc.get_text_extent(&crate::i18n::text(&(label)));
        dc.draw_text(&crate::i18n::text(&(label)),dip(34),(size.height-th)/2);
        dc.set_pen(Colour::rgb(107,119,138),dip(1).max(1),PenStyle::Solid);
        let x=size.width-dip(12);let y=size.height/2;
        dc.draw_line(x-dip(3),y-dip(1),x,y+dip(2));
        dc.draw_line(x,y+dip(2),x+dip(3),y-dip(1));
    });
    panel.on_set_focus(move |event|{panel.refresh(false,None);event.skip(true);});
    panel.on_kill_focus(move |event|{panel.refresh(false,None);event.skip(true);});
    panel.on_size(move |event|{panel.refresh(false,None);event.skip(true);});
}

fn choose_style_color(parent:&StyleEditorUi,initial:Option<&str>)->Option<Option<String>>{
    let ui=crate::i18n::localized(crate::StyleColorPaletteUi::new(Some(&parent.style_editor),false));
    let dialog=ui.style_color_palette_dialog;let surface=ui.style_color_palette_surface;
    surface.set_background_style(BackgroundStyle::Paint);
    let selected=Rc::new(RefCell::new(None::<String>));
    let current=initial.map(str::to_ascii_uppercase);
    let current_for_paint=current.clone();
    let selected_for_paint=Rc::clone(&selected);
    surface.on_paint(move |_|{
        let dc=AutoBufferedPaintDC::new(&surface);
        paint_rounded_surface(&dc,&surface,Colour::rgb(255,255,255),Colour::rgb(202,211,223),5);
        let dip=|n:i32|surface.from_dip(Size::new(n,n)).width;
        if let Some(font)=Font::new_with_details(10,FontFamily::Default as i32,FontStyle::Normal as i32,
            FontWeight::Normal as i32,false,"Segoe UI"){dc.set_font(&font);}
        let chosen=selected_for_paint.borrow();
        let active=chosen.as_ref().or(current_for_paint.as_ref());
        if active.is_none(){
            dc.set_brush(Colour::rgb(229,237,252),BrushStyle::Solid);
            dc.set_pen(Colour::rgb(181,200,238),dip(1).max(1),PenStyle::Solid);
            dc.draw_rounded_rectangle(dip(6),dip(5),dip(184),dip(27),dip(4) as f64);
        }
        dc.set_brush(Colour::rgb(0,0,0),BrushStyle::Solid);dc.set_pen(Colour::rgb(0,0,0),dip(1).max(1),PenStyle::Solid);
        dc.draw_rectangle(dip(13),dip(12),dip(18),dip(12));
        dc.set_text_foreground(Colour::rgb(36,48,66));dc.draw_text(&crate::i18n::text(&(AUTOMATIC)),dip(40),dip(9));
        dc.set_pen(Colour::rgb(223,228,236),dip(1).max(1),PenStyle::Solid);
        dc.draw_line(dip(10),dip(38),dip(186),dip(38));
        for (index,(_,hex)) in FONT_COLOURS.iter().enumerate(){
            let x=dip(12+(index%7) as i32*25);let y=dip(47+(index/7) as i32*26);
            let color=hex_colour(hex).unwrap_or(Colour::rgb(0,0,0));
            dc.set_brush(color,BrushStyle::Solid);
            dc.set_pen(if *hex=="#FFFFFF"{Colour::rgb(202,211,223)}else{color},dip(1).max(1),PenStyle::Solid);
            dc.draw_rounded_rectangle(x,y,dip(20),dip(20),dip(3) as f64);
            if active.is_some_and(|value|value.eq_ignore_ascii_case(hex)){
                dc.set_brush(Colour::rgb(255,255,255),BrushStyle::Transparent);
                dc.set_pen(Colour::rgb(58,104,194),dip(1).max(1),PenStyle::Solid);
                dc.draw_rounded_rectangle(x-dip(2),y-dip(2),dip(24),dip(24),dip(4) as f64);
            }
        }
        dc.set_pen(Colour::rgb(223,228,236),dip(1).max(1),PenStyle::Solid);
        dc.draw_line(dip(10),dip(106),dip(186),dip(106));
        dc.set_text_foreground(Colour::rgb(36,48,66));dc.draw_text(&crate::i18n::text(&(MORE_COLOURS)),dip(12),dip(116));
    });
    {let selected=Rc::clone(&selected);surface.on_mouse_left_down(move |event|{
        let WindowEventData::MouseButton(mouse)=&event else{return};
        let Some(point)=mouse.get_position()else{return};
        let px=|n:i32|surface.from_dip(Size::new(n,n)).width;
        if point.y<px(36){selected.replace(None);dialog.end_modal(ID_OK);return;}
        if (px(44)..px(100)).contains(&point.y){
            let col=(point.x-px(12))/px(25);let row=(point.y-px(47))/px(26);
            if (0..7).contains(&col)&&(0..2).contains(&row){
                let index=(row*7+col) as usize;
                if let Some((_,hex))=FONT_COLOURS.get(index){selected.replace(Some((*hex).to_owned()));dialog.end_modal(ID_OK);}
            }
            return;
        }
        if point.y>=px(108){
            let initial=current.as_deref().and_then(hex_colour).unwrap_or(Colour::rgb(0,0,0));
            let picker=ColourDialog::builder(&dialog).with_title(&crate::i18n::text(&("More colors"))).with_initial_colour(initial).with_choose_full(true).build();
            if picker.show_modal()==ID_OK{if let Some(color)=picker.get_colour(){
                selected.replace(Some(format!("#{:02X}{:02X}{:02X}",color.r,color.g,color.b)));dialog.end_modal(ID_OK);
            }}
        }
    });}
    surface.on_key_down(move |event|{
        if let WindowEventData::Keyboard(key)=&event{
            if key.get_key_code()==Some(wxdragon::keycode::WXK_ESCAPE){dialog.end_modal(wxdragon::id::ID_CANCEL);return;}
        }
        event.skip(true);
    });
    let anchor=crate::gtk_native::to_screen(&parent.style_color_display,wxdragon::geometry::Point::new(0,parent.style_color_display.get_size().height+2));
    #[cfg(windows)] unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos,SWP_NOSIZE,SWP_NOZORDER};
        SetWindowPos(dialog.get_handle(),std::ptr::null_mut(),anchor.x,anchor.y,0,0,SWP_NOSIZE|SWP_NOZORDER);
    }
    #[cfg(not(windows))] crate::placement::centre(&dialog);
    let accepted=dialog.show_modal()==ID_OK;
    let choice=if accepted{Some(selected.borrow().clone())}else{None};
    dialog.destroy();choice
}
fn read_style(ui:&StyleEditorUi,draft:&Style,original:&Style,base_ids:&[Option<StyleId>],sheet:&StyleSheet)->Result<Style,String>{
    let mut draft=draft.clone();draft.name=ui.style_name.get_value().trim().to_owned();
    if draft.name==crate::i18n::style_name(original) {draft.name=original.name.clone();}
    if draft.name.is_empty()||draft.name.len()>256||draft.name.contains(['\0','\r','\n']){return Err("Enter a one-line style name of at most 256 bytes".into());}
    if sheet.id_by_name(&draft.name).is_some_and(|id|id!=draft.id){return Err("Another style already has this name".into());}
    let base=ui.style_base.get_selection().unwrap_or(0)as usize;
    draft.based_on=base_ids.get(base).cloned().ok_or("Choose a base style")?;
    if draft.kind==StyleKind::Paragraph{
        draft.next_style=following_style(ui,sheet);
    }
    let family=crate::i18n::choice_source(&ui.style_font_family.get_value(), &["Inherit from base"]);
    let inherited=sheet.resolve_character(draft.based_on.as_ref());
    let family=if draft.character.font_family.is_none()&&inherited.font_family.as_deref()==Some(family.trim()){None}else{combo_value(&family)};
    if family!=draft.character.font_family{draft.character.set_font_family(family);}
    let size=crate::i18n::choice_source(&ui.style_font_size.get_value(), &["Inherit from base"]);
    draft.character.font_size_pt=if size.trim()=="Inherit from base" || (draft.character.font_size_pt.is_none()&&inherited.font_size_pt.is_some_and(|n|n.to_string()==size.trim())){None}else{number_value(&size,"Font size")?};
    if draft.character.font_size_pt.is_some_and(|n|!(1.0..=400.0).contains(&n)){return Err("Font size must be 1–400 pt".into());}
    draft.character.color=colour_value(&crate::i18n::source(&ui.style_font_color.get_value()));
    valid_color(&draft.character.color,"Font color",false)?;
    valid_color(&draft.character.highlight,"Highlight",true)?;
    if draft.kind==StyleKind::Paragraph{
        for (label,value) in [("Space before",draft.paragraph.space_before_pt),("Space after",draft.paragraph.space_after_pt)]{
            if value.is_some_and(|n|n.abs()>2000.0){return Err(format!("{label} must be within ±2000 pt"));}}
        for (label,value) in [("Left indent",draft.paragraph.left_indent_mm),("Right indent",draft.paragraph.right_indent_mm),("First-line indent",draft.paragraph.first_line_indent_mm)]{
            if value.is_some_and(|n|n.abs()>2000.0){return Err(format!("{label} must be within ±2000 mm"));}}
        if draft.paragraph.line_spacing.is_some_and(|n|!(0.5..=10.0).contains(&n)){return Err("Line spacing must be 0.5–10".into());}
    }
    let mut candidate=sheet.clone();candidate.styles.insert(draft.id.clone(),draft.clone());
    editor::validate_styles(&candidate)?;
    Ok(draft)
}
/// One paragraph of the Modify Style preview, resolved exactly as the layout
/// engine resolves it (layout: 11 pt default size, 1.2 line spacing,
/// space after defaulting to 0.3 em, additive before/after spacing).
#[derive(Clone)]
pub(crate) struct PreviewParagraph{pub(crate) face:String,pub(crate) size:f32,pub(crate) bold:bool,pub(crate) italic:bool,
    pub(crate) underline:bool,pub(crate) color:Option<Colour>,pub(crate) alignment:ParagraphAlignment,pub(crate) indent_pt:f32,
    /// Right indent and first-line offset (negative = hanging), in points.
    pub(crate) right_pt:f32,pub(crate) first_pt:f32,
    pub(crate) line_spacing:f32,pub(crate) before:f32,pub(crate) after:f32}
impl PreviewParagraph{
    pub(crate) fn apply_character(&mut self,c:&CharacterProperties){
        if let Some(face)=&c.font_family{self.face=face.clone();}
        if let Some(size)=c.font_size_pt{self.size=size;}
        self.bold=c.bold==Some(true);self.italic=c.italic==Some(true);self.underline=c.underline==Some(true);
        self.color=c.color.as_deref().and_then(hex_colour);
    }
}
pub(crate) struct PreviewModel{pub(crate) previous:PreviewParagraph,pub(crate) sample:PreviewParagraph,pub(crate) following:PreviewParagraph}
thread_local!{static PREVIEW:RefCell<Option<PreviewModel>>=const{RefCell::new(None)};}
const PREVIEW_DEFAULT_SIZE:f32=11.0;
const PREVIEW_DEFAULT_LINE:f32=1.2;
/// Word draws its preview as a reduced page: 60% of true size, and no text
/// larger than 18 pt on screen (so very large body text still fits).
const PREVIEW_SCALE:f32=0.6;
const PREVIEW_MAX_SAMPLE_PT:f32=18.0;
pub(crate) fn hex_colour(value:&str)->Option<Colour>{
    let hex=value.strip_prefix('#').filter(|h|h.len()==6)?;
    let n=u32::from_str_radix(hex,16).ok()?;Some(Colour::rgb((n>>16) as u8,(n>>8) as u8,n as u8))
}
pub(crate) fn preview_paragraph(sheet:&StyleSheet,id:&StyleId)->PreviewParagraph{
    preview_from(&sheet.resolve_character(Some(id)),&sheet.resolve_paragraph(Some(id)))
}
/// A preview paragraph from resolved character and paragraph properties.
pub(crate) fn preview_from(c:&CharacterProperties,p:&ParagraphProperties)->PreviewParagraph{
    let size=c.font_size_pt.unwrap_or(PREVIEW_DEFAULT_SIZE);
    PreviewParagraph{face:c.font_family.clone().unwrap_or_else(||"Linux Libertine G".into()),size,
        bold:c.bold==Some(true),italic:c.italic==Some(true),underline:c.underline==Some(true),
        color:c.color.as_deref().and_then(hex_colour),alignment:p.alignment.unwrap_or(ParagraphAlignment::Left),
        indent_pt:p.left_indent_mm.unwrap_or(0.0)*72.0/25.4,right_pt:p.right_indent_mm.unwrap_or(0.0)*72.0/25.4,
        first_pt:p.first_line_indent_mm.unwrap_or(0.0)*72.0/25.4,line_spacing:p.line_spacing.unwrap_or(PREVIEW_DEFAULT_LINE),
        before:p.space_before_pt.unwrap_or(0.0),after:p.space_after_pt.unwrap_or(size*0.3)}
}
/// Greedy word wrap of a repeating phrase into `max_lines` lines; the final
/// line stops part-way, like a real paragraph end. Returns words with widths.
fn preview_lines(dc:&impl DeviceContext,phrase:&str,avail:i32,space:i32,max_lines:usize)->Vec<Vec<(String,i32)>>{
    let words=phrase.split_whitespace().collect::<Vec<_>>();
    let mut lines:Vec<Vec<(String,i32)>>=Vec::new();let mut line:Vec<(String,i32)>=Vec::new();let mut width=0;
    for word in words.iter().cycle().take(400){
        let w=dc.get_text_extent(&crate::i18n::text(&(word))).0;
        let next=if line.is_empty(){w}else{width+space+w};
        if next>avail&&!line.is_empty(){lines.push(std::mem::take(&mut line));width=0;if lines.len()==max_lines{break;}}
        width=if line.is_empty(){w}else{width+space+w};line.push((word.to_string(),w));
    }
    if lines.len()<max_lines&&!line.is_empty(){lines.push(line);}
    if let Some(last)=lines.last_mut(){let keep=(last.len()*3).div_ceil(5).max(1);last.truncate(keep);}
    lines
}
/// Paint the preview as a scaled-down page, like Word: the paragraphs are laid
/// out at true size (1 pt = DPI/72 px, fonts at their real point sizes, the
/// layout engine's line and paragraph spacing) and the whole page is drawn
/// through one DC user scale, so every proportion is exact.
/// The preview never scrolls: when the styles' spacing does not fit, the gaps
/// shrink to the space that is left (then the previous paragraph loses a
/// line), so previous, sample and following paragraphs always stay visible.
/// The following paragraph repeats to the bottom edge, as in Word.
pub(crate) fn paint_style_preview(dc:&impl DeviceContext,panel:&Panel,model:&PreviewModel){
    let size=panel.get_client_size();
    if size.width<4||size.height<4{return;}
    let largest=model.previous.size.max(model.sample.size).max(model.following.size).max(1.0);
    let zoom=PREVIEW_SCALE.min(PREVIEW_MAX_SAMPLE_PT/largest);
    let px_per_pt=panel.from_dip(Size::new(960,960)).width as f32/720.0;
    // Page margins in screen pixels (Word keeps ~5% at each side), then logical.
    let margin_x=(size.width as f32*0.05).max(panel.from_dip(Size::new(14,14)).width as f32)/zoom;
    let margin_y=panel.from_dip(Size::new(12,12)).height as f32/zoom;
    let page_w=size.width as f32/zoom;let page_h=size.height as f32/zoom;
    let ghost=Colour::rgb(163,174,191);
    let ink=Colour::rgb(27,37,51);
    dc.set_clipping_region(1,1,size.width-2,size.height-2);
    dc.set_user_scale(zoom as f64,zoom as f64);
    struct Block{font:Font,colour:Colour,left:i32,avail:i32,first:i32,space:i32,line_h:f32,alignment:ParagraphAlignment,lines:Vec<Vec<(String,i32)>>}
    let specs=[
        (&model.previous,"Previous Paragraph",2usize,ghost),
        (&model.sample,"Sample Text",3,model.sample.color.unwrap_or(ink)),
        (&model.following,"Following Paragraph",24,ghost),
    ];
    let mut blocks=Vec::new();
    for (paragraph,phrase,max_lines,colour) in specs{
        let localized_phrase=crate::i18n::text(phrase);let phrase=localized_phrase.as_str();
        let points=(paragraph.size.round() as i32).max(1);
        let Some(font)=Font::new_with_details(points,FontFamily::Default as i32,
            if paragraph.italic{FontStyle::Italic as i32}else{FontStyle::Normal as i32},
            if paragraph.bold{FontWeight::Bold as i32}else{FontWeight::Normal as i32},paragraph.underline,&paragraph.face) else {break};
        dc.set_font(&font);
        let left=(margin_x+paragraph.indent_pt*px_per_pt).round() as i32;
        let right=(margin_x+paragraph.right_pt*px_per_pt).round() as i32;
        let avail=((page_w as i32-right)-left).max(1);
        let first=(paragraph.first_pt*px_per_pt).round() as i32;
        let space=dc.get_text_extent(&crate::i18n::text(&(" "))).0.max(1);
        let mut lines=preview_lines(dc,phrase,avail,space,max_lines);
        // Only the sample ends part-way; the context paragraphs run on.
        if max_lines!=3{lines=preview_lines(dc,phrase,avail,space,max_lines+1);lines.truncate(max_lines);}
        blocks.push(Block{font,colour,left,avail,first,space,line_h:paragraph.size*paragraph.line_spacing*px_per_pt,alignment:paragraph.alignment,lines});
    }
    if blocks.len()==3{
        let mut gaps=[0.0,(model.previous.after+model.sample.before)*px_per_pt,(model.sample.after+model.following.before)*px_per_pt];
        let room=page_h-2.0*margin_y;
        // What must stay visible: previous, the sample and one following line.
        let needed=|blocks:&[Block]|blocks[0].lines.len() as f32*blocks[0].line_h
            +blocks[1].lines.len() as f32*blocks[1].line_h+blocks[2].line_h;
        let gap_total:f32=gaps.iter().sum();
        if needed(&blocks)+gap_total>room{
            if needed(&blocks)>room{blocks[0].lines.truncate(1);}
            let left_over=(room-needed(&blocks)).max(0.0);
            if gap_total>left_over&&gap_total>0.0{for gap in &mut gaps{*gap*=left_over/gap_total;}}
        }
        let mut y=margin_y;
        for (block,gap) in blocks.iter().zip(gaps){
            y+=gap;
            dc.set_font(&block.font);dc.set_text_foreground(block.colour);
            let text_h=dc.get_text_extent(&crate::i18n::text(&("Hg"))).1;
            let count=block.lines.len();
            for (index,line) in block.lines.iter().enumerate(){
                if y+block.line_h>page_h-margin_y{break;}
                let text_w=line.iter().map(|(_,w)|*w).sum::<i32>();
                let words=line.len().saturating_sub(1) as i32;
                let natural=text_w+words*block.space;
                // The first line starts at the first-line offset (negative = hanging).
                let (left,avail)=if index==0{(block.left+block.first,block.avail-block.first)}else{(block.left,block.avail)};
                let (mut x,gap)=match block.alignment{
                    ParagraphAlignment::Center=>(left+(avail-natural)/2,block.space),
                    ParagraphAlignment::Right=>(left+avail-natural,block.space),
                    ParagraphAlignment::Justify if index+1<count&&words>0=>(left,(avail-text_w)/words),
                    _=>(left,block.space),
                };
                let top=(y+(block.line_h-text_h as f32)/2.0).round() as i32;
                for (word,w) in line{dc.draw_text(&crate::i18n::text(&(word)),x,top);x+=w+gap;}
                y+=block.line_h;
            }
        }
    }
    dc.set_user_scale(1.0,1.0);
    dc.destroy_clipping_region();
}
/// Themed rounded surface: the corners take the parent colour, the face keeps
/// the panel's own background colour, and a 1-DIP border matches the Ribbon.
pub(crate) fn paint_rounded_surface(dc:&impl DeviceContext,panel:&Panel,outside:Colour,border:Colour,radius_dip:i32){
    let size=panel.get_client_size();
    let stroke=panel.from_dip(Size::new(1,1)).width.max(1);
    let radius=panel.from_dip(Size::new(radius_dip,radius_dip)).width as f64;
    dc.set_background(outside);dc.clear();
    dc.set_brush(panel.get_background_color(),BrushStyle::Solid);
    dc.set_pen(border,stroke,PenStyle::Solid);
    let inset=stroke/2;
    dc.draw_rounded_rectangle(inset,inset,size.width-stroke,size.height-stroke,radius);
}
/// Install rounded painting on the dialog's card, preview and description.
fn install_rounded_surfaces(ui:&StyleEditorUi){
    let chrome=Colour::rgb(232,236,242);let card=Colour::rgb(255,255,255);
    for (panel,outside,border,radius,preview) in [
        (ui.style_card,chrome,Colour::rgb(223,228,236),7,false),
        (ui.style_preview_panel,card,Colour::rgb(202,211,223),5,true),
        (ui.style_summary_panel,card,Colour::rgb(223,228,236),5,false),
    ]{
        panel.set_background_style(BackgroundStyle::Paint);
        panel.on_paint(move |_|{
            let dc=AutoBufferedPaintDC::new(&panel);
            paint_rounded_surface(&dc,&panel,outside,border,radius);
            if preview{PREVIEW.with(|model|if let Some(model)=model.borrow().as_ref(){paint_style_preview(&dc,&panel,model);});}
        });
        panel.on_size(move |event|{panel.refresh(false,None);event.skip(true);});
    }
}
/// Everything the Modify Style session shares between its handlers.
struct Session{ui:Rc<StyleEditorUi>,draft:RefCell<Style>,original:Style,ids:Vec<Option<StyleId>>,sheet:StyleSheet,pending:format_dialogs::Pending}
impl Session{
    /// The draft with the main dialog's controls applied (no validation).
    fn current(&self)->Style{let mut draft=self.draft.borrow().clone();merge_controls(&self.ui,&mut draft,&self.ids,&self.sheet);draft}
    /// Edit the draft (after taking the main controls into it) and refresh.
    fn edit(&self,change:impl FnOnce(&mut Style,&StyleSheet)){
        let mut draft=self.current();change(&mut draft,&self.sheet);self.draft.replace(draft);self.refresh();
    }
    fn refresh(&self){update_style_preview(self);}
    fn change_base(&self){
        let selected=self.ui.style_base.get_selection().unwrap_or(0) as usize;
        let Some(base)=self.ids.get(selected) else{return;};
        let mut draft=self.draft.borrow().clone();
        merge_font_controls(&self.ui,&mut draft,&self.sheet);
        draft.based_on=base.clone();
        self.draft.replace(draft.clone());
        sync_controls(&self.ui,&draft,&self.sheet);
        self.refresh();
    }
}
fn update_style_preview(session:&Session){
    let ui=&session.ui;let sheet=&session.sheet;
    let draft=match read_style(ui,&session.draft.borrow(),&session.original,&session.ids,sheet){Ok(d)=>d,Err(error)=>{ui.style_error.set_label(&crate::i18n::text(&(&error)));return;}};
    ui.style_error.set_label(&crate::i18n::text(&("")));
    let mut resolved=sheet.clone();resolved.styles.insert(draft.id.clone(),draft.clone());
    let c=resolved.resolve_character(Some(&draft.id));
    let p=resolved.resolve_paragraph(Some(&draft.id));
    let size=c.font_size_pt.unwrap_or(11.0);
    // Preview context: the following paragraph uses "Style for following
    // paragraph" (the style itself for "Same style"); the previous paragraph
    // uses a style that names this one as its follower, else the base style.
    let normal=StyleId::from(model::STYLE_NORMAL);
    let (previous,sample_paragraph,following)=if draft.kind==StyleKind::Paragraph{
        let previous=resolved.styles.values()
            .filter(|s|s.kind==StyleKind::Paragraph&&s.id!=draft.id&&s.next_style.as_ref()==Some(&draft.id))
            .min_by_key(|s|s.name.to_lowercase()).map(|s|s.id.clone())
            .or_else(||draft.based_on.clone()).unwrap_or_else(||normal.clone());
        (previous,draft.id.clone(),draft.next_style.clone().unwrap_or_else(||draft.id.clone()))
    }else{(normal.clone(),normal.clone(),normal)};
    let mut sample=preview_paragraph(&resolved,&sample_paragraph);
    if draft.kind==StyleKind::Character{sample.apply_character(&c);}
    let previous=preview_paragraph(&resolved,&previous);
    // Keep the sample's resolved size so explicit overrides remain visible
    // when the base style uses a different point size.
    PREVIEW.with(|model|model.replace(Some(PreviewModel{
        previous,sample,following:preview_paragraph(&resolved,&following)})));
    ui.style_preview_panel.refresh(false,None);
    let alignment=p.alignment.unwrap_or(ParagraphAlignment::Left);
    let line_spacing=p.line_spacing.unwrap_or(1.2);
    let indent=p.left_indent_mm.unwrap_or(0.0);
    let summary=format!("Font: {}, {} pt{}{}{}{}\nParagraph: {} alignment · before {} pt · after {} pt · line spacing {}{}\nBased on: {}",
        c.font_family.as_deref().unwrap_or("Default"),size,
        if c.bold==Some(true){crate::i18n::text(", bold")}else{String::new()},if c.italic==Some(true){crate::i18n::text(", italic")}else{String::new()},
        if c.underline==Some(true){crate::i18n::text(", underline")}else{String::new()},
        c.color.as_deref().map(|v|crate::i18n::text(&format!(", color: {}",crate::i18n::text(&colour_label(Some(v)))))).unwrap_or_default(),
        crate::i18n::text(match alignment{ParagraphAlignment::Left=>"Left",ParagraphAlignment::Center=>"Center",ParagraphAlignment::Right=>"Right",ParagraphAlignment::Justify=>"Justify"}),
        p.space_before_pt.unwrap_or(0.0),p.space_after_pt.unwrap_or(0.0),line_spacing,
        if indent!=0.0{format!(" · left indent {indent} mm")}else{String::new()},
        draft.based_on.as_ref().and_then(|id|sheet.get(id)).map(crate::i18n::style_name).unwrap_or_else(||crate::i18n::text("No base style")));
    ui.style_summary.set_label(&crate::i18n::text(&(&summary)));
    fit_style_summary(ui);
    // Toolbar state mirrors the resolved style, exactly like the Ribbon's toggles.
    let paragraph=draft.kind==StyleKind::Paragraph;
    for (button,on) in [
        (ui.style_bold_button,c.bold==Some(true)),(ui.style_italic_button,c.italic==Some(true)),
        (ui.style_underline_button,c.underline==Some(true)),
        (ui.style_left_button,paragraph&&alignment==ParagraphAlignment::Left),
        (ui.style_center_button,paragraph&&alignment==ParagraphAlignment::Center),
        (ui.style_right_button,paragraph&&alignment==ParagraphAlignment::Right),
        (ui.style_justify_button,paragraph&&alignment==ParagraphAlignment::Justify),
        (ui.style_single_spacing,paragraph&&(line_spacing-1.0).abs()<0.01),
        (ui.style_onehalf_spacing,paragraph&&(line_spacing-1.5).abs()<0.01),
        (ui.style_double_spacing,paragraph&&(line_spacing-2.0).abs()<0.01),
    ]{skin::selected(&button,on);}
}
/// Word-style Modify Style toolbar: (button, icon, tooltip). Row 1 then row 2.
fn style_toolbar(ui:&StyleEditorUi)->[(Button,&'static str,&'static str);14]{[
    (ui.style_bold_button,"bold","Bold"),(ui.style_italic_button,"italic","Italic"),(ui.style_underline_button,"underline","Underline"),
    (ui.style_left_button,"style-align-left","Align left"),(ui.style_center_button,"style-align-center","Center"),
    (ui.style_right_button,"style-align-right","Align right"),(ui.style_justify_button,"style-align-justify","Justify"),
    (ui.style_single_spacing,"style-line-single","Single line spacing"),(ui.style_onehalf_spacing,"style-line-onehalf","1.5 line spacing"),
    (ui.style_double_spacing,"style-line-double","Double line spacing"),
    (ui.style_spacing_increase,"style-spacing-increase","Increase paragraph spacing"),
    (ui.style_spacing_decrease,"style-spacing-decrease","Decrease paragraph spacing"),
    (ui.style_indent_decrease,"style-indent-decrease","Decrease indent"),(ui.style_indent_increase,"style-indent-increase","Increase indent"),
]}
/// Paint a dialog button with the Ribbon's own skin (flat white face, hover and
/// selected states) and pixel-exact artwork, so the dialog matches the Ribbon.
fn skin_dialog_button(button:Button,icon:&'static str,tip:&'static str,side:i32,art_dip:u32){
    if let Some(art)=art::load(icon,art_dip,art_dip){
        button.set_label(&crate::i18n::text(&("")));bitmap_button::set_bundle(&button,&art.normal,ButtonBitmapPosition::Left);button.set_bitmap_bundle_disabled(&art.disabled);
    }
    button.set_min_size(Size::new(side,side));button.set_size(Size::new(side,side));
    skin::install(&ButtonSpec{button,name:"style_editor_tool",label:"",icon,action:Action::StyleModify,kind:"icon",segment:"single",
        tip,disabled:false,width_dip:26,height_dip:26,art_width_dip:art_dip as i32,art_height_dip:art_dip as i32});
}
/// Size the dialog to its sizer: the minimum is the laid-out content (so it
/// shrinks again when a Format panel closes); the width keeps Word's proportions.
const SUMMARY_PADDING_DIP:i32=10;
const SUMMARY_WRAP_WIDTH_DIP:i32=660;
const SUMMARY_MIN_HEIGHT_DIP:i32=48;

/// Size all summary lines, including translated and wrapped descriptions.
fn fit_style_summary(ui:&StyleEditorUi){
    let dip=|value|ui.style_editor.from_dip_int(value);
    let available=ui.style_summary_panel.get_client_size().width-2*dip(SUMMARY_PADDING_DIP);
    let width=if available>dip(300){available}else{dip(SUMMARY_WRAP_WIDTH_DIP)};
    ui.style_summary.wrap(width);
    let label=ui.style_summary.get_label();
    let height=label.lines().map(|line|ui.style_summary.get_text_extent(if line.is_empty(){"Hg"}else{line}).height).sum::<i32>();
    ui.style_summary.set_min_size(Size::new(-1,height.max(dip(SUMMARY_MIN_HEIGHT_DIP))));
    ui.style_editor.layout();
}

fn fit_style_editor(ui:&StyleEditorUi){
    ui.style_editor.set_min_size(Size::new(-1,-1));
    ui.style_editor.layout();
    fit_style_summary(ui);
    let best=ui.style_editor.get_best_size();
    let width=best.width.max(ui.style_editor.from_dip(Size::new(760,0)).width);
    ui.style_editor.set_min_size(best);
    ui.style_editor.set_size(Size::new(width,best.height));
    ui.style_editor.layout();
}
impl Ribbon{
 pub(crate) fn edit_style(&self,draft:Style)->Result<bool,String>{
  let result = self.edit_style_with_parent(draft, &self.ui.main_frame);
  self.focus_document();
  result
 }
 pub(crate) fn edit_style_with_parent(&self,draft:Style,parent:&impl WxWidget)->Result<bool,String>{
  let revision=self.state.borrow().editor.revision;
  let sheet=self.state.borrow().editor.document.styles.clone();
  let ui=Rc::new(crate::i18n::localized(StyleEditorUi::new(Some(parent),false)));
  populate_font_combo(ui.style_font_family,true);
  // One square for both toolbar rows: the Home tab's 28-DIP field height, or
  // the native combo height if a theme draws it taller, so the buttons line up
  // with the font boxes on every DPI.
  let field=ui.style_editor.from_dip(Size::new(28,28)).height;
  for combo in [ui.style_font_family,ui.style_font_size]{let w=combo.get_size().width;combo.set_min_size(Size::new(w,field));}
  ui.style_font_color.hide();
  ui.style_color_display.set_min_size(ui.style_color_display.from_dip(Size::new(150,28)));
  let side=ui.style_font_size.get_size().height.max(field);
  for (button,icon,tip) in style_toolbar(&ui){skin_dialog_button(button,icon,tip,side,16);}
  // Footer commands use the Ribbon painter too: outlined faces on the chrome,
  // OK filled with the accent as the default button, Format with the combo caret.
  for (button,kind,label) in [(ui.style_format,"dialog-menu","Format"),(ui.style_save,"dialog-default","OK"),(ui.style_cancel,"dialog","Cancel")]{
    // The painted face draws its own caret; the native fallback keeps "Format ▾".
    if skin::install(&ButtonSpec{button,name:"style_editor_command",label,icon:"",action:Action::StyleModify,kind,segment:"single",
      tip:label,disabled:false,width_dip:88,height_dip:28,art_width_dip:0,art_height_dip:0}){button.set_label(&crate::i18n::text(&(label)));}
  }
  install_rounded_surfaces(&ui);
  install_style_color_display(&ui);
  ui.style_editor.set_escape_id(StyleEditorUi::xrc_id("style_cancel"));
  ui.style_type.set_selection(if draft.kind==StyleKind::Paragraph{0}else{1});
  ui.style_name.change_value(&crate::i18n::style_name(&draft));
  let mut eligible=sheet.styles.values().filter(|s|s.id!=draft.id&&s.kind==draft.kind&&{
      let mut base=s.based_on.as_ref();let mut cycle=false;
      while let Some(id)=base{if id==&draft.id{cycle=true;break;}base=sheet.get(id).and_then(|style|style.based_on.as_ref());}
      !cycle
  }).collect::<Vec<_>>();
  eligible.sort_by(|a,b|a.name.to_lowercase().cmp(&b.name.to_lowercase()));
  let mut base_ids=vec![None];for style in eligible{ui.style_base.append(&crate::i18n::style_name(style));base_ids.push(Some(style.id.clone()));}
  ui.style_base.set_selection(base_ids.iter().position(|id|*id==draft.based_on).unwrap_or(0)as u32);
  ui.style_base.enable(base_ids.len()>1);
  let mut next_styles=sheet.styles.values().filter(|style|style.kind==StyleKind::Paragraph).collect::<Vec<_>>();
  next_styles.sort_by(|a,b|a.name.to_lowercase().cmp(&b.name.to_lowercase()));
  for style in &next_styles{ui.style_next.append(&crate::i18n::style_name(style));}
  ui.style_next.set_selection(draft.next_style.as_ref().and_then(|id|next_styles.iter().position(|style|&style.id==id)).map_or(0,|index|index as u32+1));
  if draft.kind==StyleKind::Character{ui.style_next.enable(false);}
  sync_controls(&ui,&draft,&sheet);
  if draft.kind==StyleKind::Character{for (button,_,_) in style_toolbar(&ui).into_iter().skip(3){button.enable(false);}}
  let pending=Rc::new(RefCell::new(PendingFormat::from_style(&draft,&sheet)));
  let session=Rc::new(Session{ui:Rc::clone(&ui),draft:RefCell::new(draft.clone()),original:draft,ids:base_ids,sheet:sheet.clone(),pending});
  // Toggle what the button shows: an inherited "on" turns off, anything else
  // turns on, matching the selected state.
  for (button,field) in [(ui.style_bold_button,0),(ui.style_italic_button,1),(ui.style_underline_button,2)]{
    let session=Rc::clone(&session);
    button.on_click(move |_|session.edit(|draft,sheet|{
      let inherited=sheet.resolve_character(draft.based_on.as_ref());
      let slot=match field{0=>&mut draft.character.bold,1=>&mut draft.character.italic,_=>&mut draft.character.underline};
      let base=match field{0=>inherited.bold,1=>inherited.italic,_=>inherited.underline};
      let on=slot.or(base)==Some(true);*slot=Some(!on);
    }));
  }
  for (button,alignment) in [(ui.style_left_button,ParagraphAlignment::Left),(ui.style_center_button,ParagraphAlignment::Center),
    (ui.style_right_button,ParagraphAlignment::Right),(ui.style_justify_button,ParagraphAlignment::Justify)]{
    let session=Rc::clone(&session);
    button.on_click(move |_|session.edit(|draft,_|draft.paragraph.alignment=Some(alignment)));
  }
  for (button,spacing) in [(ui.style_single_spacing,1.0_f32),(ui.style_onehalf_spacing,1.5),(ui.style_double_spacing,2.0)]{
    let session=Rc::clone(&session);
    button.on_click(move |_|session.edit(|draft,_|draft.paragraph.line_spacing=Some(spacing)));
  }
  // Word's paragraph-spacing buttons step the space before in 6 pt; indent
  // buttons use the Ribbon's 5 mm indent step. Unset values start from the
  // value inherited through the base style.
  for (button,delta) in [(ui.style_spacing_increase,6.0_f32),(ui.style_spacing_decrease,-6.0_f32)]{
    let session=Rc::clone(&session);
    button.on_click(move |_|session.edit(|draft,sheet|{
      let current=draft.paragraph.space_before_pt.or(sheet.resolve_paragraph(draft.based_on.as_ref()).space_before_pt).unwrap_or(0.0);
      draft.paragraph.space_before_pt=Some((current+delta).clamp(0.0,2000.0));
    }));
  }
  for (button,delta) in [(ui.style_indent_decrease,-5.0_f32),(ui.style_indent_increase,5.0_f32)]{
    let session=Rc::clone(&session);
    button.on_click(move |_|session.edit(|draft,sheet|{
      let current=draft.paragraph.left_indent_mm.or(sheet.resolve_paragraph(draft.based_on.as_ref()).left_indent_mm).unwrap_or(0.0);
      draft.paragraph.left_indent_mm=Some((current+delta).clamp(0.0,2000.0));
    }));
  }
  {let session=Rc::clone(&session);ui.style_name.on_text_updated(move |_|session.refresh());}
  for control in [ui.style_font_family,ui.style_font_size]{
    let session=Rc::clone(&session);control.on_text_updated(move |_|session.refresh());
  }
  {let session=Rc::clone(&session);let panel=ui.style_color_display;
   panel.on_mouse_left_down(move |event|{
    panel.set_focus();
    let initial=colour_value(&crate::i18n::source(&session.ui.style_font_color.get_value()));
    if let Some(choice)=choose_style_color(&session.ui,initial.as_deref()){
        fill_colour_list(session.ui.style_font_color,&session.ui.style_editor,choice.as_deref(),AUTOMATIC);
        panel.refresh(false,None);session.refresh();
    }
    event.skip(false);
   });}
  {let session=Rc::clone(&session);let panel=ui.style_color_display;
   panel.on_key_down(move |event|{
    if let WindowEventData::Keyboard(key)=&event{
        if matches!(key.get_key_code(),Some(wxdragon::keycode::WXK_RETURN|wxdragon::keycode::WXK_SPACE)){
            let initial=colour_value(&crate::i18n::source(&session.ui.style_font_color.get_value()));
            if let Some(choice)=choose_style_color(&session.ui,initial.as_deref()){
                fill_colour_list(session.ui.style_font_color,&session.ui.style_editor,choice.as_deref(),AUTOMATIC);
                panel.refresh(false,None);session.refresh();
            }
            event.skip(false);return;
        }
    }
    event.skip(true);
   });}
  {let session=Rc::clone(&session);ui.style_base.on_selection_changed(move |_|session.change_base());}
  for control in [ui.style_next,ui.style_font_family,ui.style_font_size]{
    let session=Rc::clone(&session);control.on_selection_changed(move |_|session.refresh());
  }
  let saved=Rc::new(RefCell::new(None));
  {let save_button=ui.style_save;let session=Rc::clone(&session);let saved=Rc::clone(&saved);
   save_button.on_click(move |_|match read_style(&session.ui,&session.draft.borrow(),&session.original,&session.ids,&session.sheet){
    Ok(style)=>{saved.replace(Some(style));session.ui.style_editor.end_modal(ID_OK);},
    Err(error)=>session.ui.style_error.set_label(&crate::i18n::text(&(&error))),
   });}
  {let dialog=ui.style_editor;ui.style_cancel.on_click(move |_|dialog.end_modal(wxdragon::id::ID_CANCEL));}
  // Format ▾: each item opens its own Word-style dialog on a copy of the
  // draft; OK writes the copy back.
  {const FIRST:i32=31101;
   let session_for_menu=Rc::clone(&session);
   ui.style_preview_panel.on_menu_selected(move |event|{
    let Some(item)=FormatItem::ALL.get((event.get_id()-FIRST) as usize).copied() else{event.skip(true);return;};
    let session=&session_for_menu;
    let mut draft=session.current();
    if format_dialogs::run(item,session.ui.style_editor,&mut draft,&session.sheet,&session.pending){
      sync_controls(&session.ui,&draft,&session.sheet);session.draft.replace(draft);session.refresh();
    }
   });
   let button=ui.style_format;let panel=ui.style_preview_panel;let paragraph=session.original.kind==StyleKind::Paragraph;
   button.on_click(move |_|{
    let mut builder=Menu::builder();
    for (index,item) in FormatItem::ALL.iter().enumerate(){builder=builder.append_item(FIRST+index as i32,&crate::i18n::text(&(item.menu_label())),&crate::i18n::text(&("")));}
    let mut menu=builder.build();
    if !paragraph{for (index,item) in FormatItem::ALL.iter().enumerate(){if item.paragraph_only(){menu.enable_item(FIRST+index as i32,false);}}}
    let anchor=crate::gtk_native::to_screen(&button,wxdragon::geometry::Point::new(0,button.get_size().height));
    panel.popup_menu(&mut menu,Some(anchor));
   });
  }
  session.refresh();
  fit_style_editor(&ui);
  crate::placement::centre(&ui.style_editor);let result=ui.style_editor.show_modal();
  let change=saved.borrow_mut().take();ui.style_editor.destroy();PREVIEW.with(|model|model.replace(None));
  if result!=ID_OK{return Ok(false);}
  let mut change=change.ok_or("The style editor closed without a style definition")?;
  if self.state.borrow().editor.revision!=revision{return Err("The document changed while the style editor was open; reopen the manager".into());}
  let shortcut_changed=change.formatting.remove("key_scope_changed").is_some();
  let default_language=change.formatting.remove("lng_make_default");
  if shortcut_changed || default_language.is_some() {
    let save_shortcuts_in_template=shortcut_changed && change.formatting.get("key_save_in").is_some_and(|value|value=="Normal template");
    let assignments=if shortcut_changed {change.formatting.iter().filter(|(key,_)|key.starts_with("key_keys:"))
        .map(|(key,value)|(key.clone(),value.clone())).collect::<Vec<_>>()}else{Vec::new()};
    let template_shortcuts=save_shortcuts_in_template.then(||assignments.iter().cloned().collect::<std::collections::BTreeMap<_,_>>());
    if shortcut_changed {change.formatting.retain(|key,_|!key.starts_with("key_keys:"));}
    let normal_id=StyleId::from(model::STYLE_NORMAL);
    let mut coordinated=std::collections::BTreeMap::new();
    if shortcut_changed {
      for old in sheet.styles.values() {
        if old.formatting.keys().any(|key|key.starts_with("key_keys:")) {
          let mut cleaned=old.clone();cleaned.formatting.retain(|key,_|!key.starts_with("key_keys:"));
          coordinated.insert(cleaned.id.clone(),cleaned);
        }
      }
    }
    coordinated.insert(change.id.clone(),change);
    let mut normal=coordinated.remove(&normal_id).or_else(||sheet.get(&normal_id).cloned()).ok_or("Normal style is missing")?;
    if shortcut_changed {normal.formatting.extend(assignments);}
    if let Some(language)=&default_language {normal.character.language=Some(language.clone());}
    coordinated.insert(normal_id,normal);
    self.edit_without_focus(|e|e.put_styles_with_selection(coordinated.into_values().collect()))?;
    if default_language.is_some() || template_shortcuts.is_some() {
      crate::document_defaults::update_template(default_language.as_deref(),template_shortcuts.as_ref())?;
    }
  } else { self.edit_without_focus(|e|e.put_styles_with_selection(vec![change]))?; }
  Ok(true)
 }
 pub(crate)fn highlight_dialog(&self)->Result<(),String>{let options=["Yellow".into(),"Green".into(),"Cyan".into(),"Pink".into(),"Remove highlight (override inherited highlight)".into(),"Custom #RRGGBB…".into()];let Some(i)=self.choose_review("Text highlight","Applies to selected text or subsequent typing.",&options)else{return Ok(());};let color=if i==5{let Some(v)=self.text_review("Highlight color","#RRGGBB:","#FFFF00",false)else{return Ok(());};v}else{["#FFFF00","#B6F5A5","#A5E7FF","#FFC2E2","none"][i].into()};self.edit(|e|e.apply_character_properties(&CharacterProperties{highlight:Some(color),..Default::default()}))}
 pub(crate) fn language_dialog(&self) -> Result<(), String> {
     self.show_spelling_menu();
     Ok(())
 }
 pub(crate) fn tabs_dialog(&self) -> Result<(), String> {
    let (tabs, default) = {
        let state = self.state.borrow();
        (state.editor.formatting_summary().tabs,
            state.editor.document.styles.resolve_paragraph(Some(&model::StyleId::from(model::STYLE_NORMAL))).default_tab_mm.unwrap_or(model::DEFAULT_TAB_INTERVAL_MM))
    };
    if let Some(patch) = super::format_dialogs::edit_tab_stops(&self.ui.main_frame, tabs, default) {
        self.edit(|editor| editor.set_tab_settings(patch.tabs, patch.default_tab_mm))?;
    }
    Ok(())
 }
 pub(crate)fn sync_style_caption(&self){let s=self.state.borrow();let label=s.editor.focus_position().and_then(|pos|model::document_paragraphs(&s.editor.document).into_iter().find(|p|p.id==pos.node_id)).map(|p|{let name=p.style.as_ref().and_then(|id|s.editor.document.styles.get(id)).map(|s|s.name.as_str()).unwrap_or("Normal");let direct=p.properties!=ParagraphProperties::default()||p.content.iter().any(|i|matches!(i,model::Inline::Text(t)if t.properties!=CharacterProperties::default()));format!("{}{}",name,if direct{" + direct"}else{""})}).unwrap_or_default();drop(s);if self.label_changed("style",&label){self.ui.style_context_label.set_label(&crate::i18n::text(&(&label)));}}
}
}

mod pane {
//! Docked Styles pane. The list owns only stable style IDs; document changes
//! go through the same undoable editor commands as the Ribbon.
//!
//! The list is painted (Home tab design language): each style name is shown in
//! its own font, the kind marker sits at the right, and the selection uses the
//! Ribbon's accent face. Keyboard, wheel and double-click are handled here.
use super::*;
use super::dialog::paint_rounded_surface;
use model::{Style, StyleId, StyleKind, Inline};
use std::{collections::BTreeSet, time::{Duration, Instant}};
use wxdragon::font::{Font, FontFamily, FontStyle, FontWeight};
use wxdragon::{BrushStyle, DeviceContext, PenStyle, dc::AutoBufferedPaintDC, window::BackgroundStyle, event::window_events::WindowEvents,
    keycode::{WXK_UP, WXK_DOWN, WXK_HOME, WXK_END, WXK_PAGEUP, WXK_PAGEDOWN, WXK_RETURN, WXK_ESCAPE}};

const INK: (u8,u8,u8) = (36,48,66);
const MUTED: (u8,u8,u8) = (107,119,138);
const ROW_DIP: i32 = 30;
const INSET_DIP: i32 = 4;
const DOUBLE_CLICK: Duration = Duration::from_millis(500);
fn rgb(c:(u8,u8,u8))->Colour{Colour::rgb(c.0,c.1,c.2)}

/// One painted row: the style's own face, clamped to a readable list size.
pub(crate) struct StyleRow { id: StyleId, name: String, kind: StyleKind, face: String, points: i32, bold: bool, italic: bool, underline: bool, colour: Option<Colour> }
#[derive(Default)]
pub(crate) struct StylesView { rows: Vec<StyleRow>, selected: Option<usize>, hover: Option<usize>, scroll: i32, last_click: Option<(usize, Instant)> }
impl StylesView {
    pub(crate) fn selected_id(&self)->Option<StyleId>{self.selected.and_then(|i|self.rows.get(i)).map(|r|r.id.clone())}
    pub(crate) fn has_selection(&self)->bool{self.selected.is_some_and(|i|i<self.rows.len())}
}
fn hex(value:Option<&str>)->Option<Colour>{
    let hex=value?.strip_prefix('#').filter(|h|h.len()==6)?;let n=u32::from_str_radix(hex,16).ok()?;
    Some(Colour::rgb((n>>16) as u8,(n>>8) as u8,n as u8))
}
fn face_font(points:i32,bold:bool,italic:bool,underline:bool,face:&str)->Option<Font>{
    Font::new_with_details(points.max(1),FontFamily::Default as i32,if italic{FontStyle::Italic as i32}else{FontStyle::Normal as i32},
        if bold{FontWeight::Bold as i32}else{FontWeight::Normal as i32},underline,face)
}
/// Shorten `text` with an ellipsis until it fits `width` in the DC's font.
fn fit_text(dc:&impl DeviceContext,text:&str,width:i32)->String{
    if dc.get_text_extent(&crate::i18n::text(&(text))).0<=width{return text.to_owned();}
    let mut chars=text.chars().collect::<Vec<_>>();
    while !chars.is_empty(){chars.pop();let candidate=format!("{}…",chars.iter().collect::<String>().trim_end());
        if dc.get_text_extent(&crate::i18n::text(&(&candidate))).0<=width{return candidate;}}
    "…".into()
}
fn used_styles(document:&Document)->BTreeSet<StyleId> {
    fn runs(items:&[Inline],out:&mut BTreeSet<StyleId>){for item in items{match item{
        Inline::Text(t)=>{if let Some(id)=&t.style{out.insert(id.clone());}},
        Inline::Hyperlink(h)=>runs(&h.content,out),_=>{}
    }}}
    let mut out=BTreeSet::new();
    for p in model::document_paragraphs(document){
        out.insert(p.style.clone().unwrap_or_else(||StyleId::from(model::STYLE_NORMAL)));
        runs(&p.content,&mut out);
    }
    out
}
fn filtered_styles(document:&Document,filter:&str)->Vec<Style> {
    let used=if filter=="In use"{used_styles(document)}else{BTreeSet::new()};
    let mut styles=document.styles.styles.values().filter(|s|match filter{
        "Paragraph styles"=>s.kind==StyleKind::Paragraph,"Character styles"=>s.kind==StyleKind::Character,
        "In use"=>used.contains(&s.id),_=>true
    }).cloned().collect::<Vec<_>>();
    styles.sort_by(|a,b|a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    styles
}
impl Ribbon {
    pub(crate) fn bind_styles_pane(self:&Rc<Self>){
        let handle=self.ui.styles_resize_handle;
        handle.set_background_style(BackgroundStyle::Paint);
        handle.on_paint(move |_|{
            let dc=AutoBufferedPaintDC::new(&handle);
            dc.set_background(Colour::rgb(232,236,242));dc.clear();
            let size=handle.get_client_size();let stroke=handle.from_dip(Size::new(1,1)).width.max(1);
            dc.set_brush(Colour::rgb(202,211,223),BrushStyle::Solid);
            dc.set_pen(Colour::rgb(202,211,223),stroke,PenStyle::Solid);
            dc.draw_rectangle((size.width-stroke)/2,0,stroke,size.height);
        });
        // The handle's screen position changes during a resize. Measure the
        // pointer in screen coordinates so the width does not jump mid-drag.
        let drag=Rc::new(Cell::new(None::<(i32,i32)>));
        {let drag=Rc::clone(&drag);let pane=self.ui.styles_panel;
         handle.on_mouse_left_down(move |event|{
            if let WindowEventData::MouseButton(mouse)=&event{
                if let Some(point)=mouse.get_position(){
                    let screen=crate::gtk_native::to_screen(&handle,point);
                    drag.set(Some((screen.x,pane.get_size().width)));
                    if !handle.has_capture(){handle.capture_mouse();}
                    crate::native_input::set_pointer(crate::objects_ui::Pointer::ResizeColumn);
                }
            }
            event.skip(false);
         });}
        {let drag=Rc::clone(&drag);let weak=Rc::downgrade(self);
         handle.on_mouse_motion(move |event|{
            crate::native_input::set_pointer(crate::objects_ui::Pointer::ResizeColumn);
            if let (Some((start_x,start_width)),WindowEventData::MouseMotion(mouse))=(drag.get(),&event){
                if let (Some(point),Some(r))=(mouse.get_position(),weak.upgrade()){
                    let screen=crate::gtk_native::to_screen(&handle,point);
                    let minimum=handle.from_dip(Size::new(280,280)).width;
                    let canvas_min=handle.from_dip(Size::new(420,420)).width;
                    let maximum=(r.ui.workspace_panel.get_client_size().width-canvas_min-handle.get_size().width).max(minimum);
                    let width=(start_width+start_x-screen.x).clamp(minimum,maximum);
                    if width!=r.ui.styles_panel.get_size().width{
                        r.ui.styles_panel.set_min_size(Size::new(width,-1));
                        r.ui.workspace_panel.layout();r.ui.body_panel.layout();
                        r.state.borrow_mut().needs_present=true;
                    }
                }
            }
            event.skip(false);
         });}
        {let drag=Rc::clone(&drag);handle.on_mouse_left_up(move |event|{
            drag.set(None);if handle.has_capture(){handle.release_mouse();}
            crate::native_input::set_pointer(crate::objects_ui::Pointer::Arrow);
            event.skip(false);
        });}
        {let drag=Rc::clone(&drag);handle.on_mouse_leave(move |event|{
            if drag.get().is_none(){crate::native_input::set_pointer(crate::objects_ui::Pointer::Arrow);}event.skip(true);
        });}
        let weak=Rc::downgrade(self);
        self.ui.styles_filter.on_selection_changed(move |_|{if let Some(r)=weak.upgrade(){
            if !r.styles_syncing.get(){r.styles_revision.set(None);r.refresh_styles_pane();}
        }});
        let list=self.ui.styles_list;
        self.ui.styles_scroll.hide();
        list.set_background_style(BackgroundStyle::Paint);
        let weak=Rc::downgrade(self);
        list.on_paint(move |_|{if let Some(r)=weak.upgrade(){r.paint_styles_list();}});
        let weak=Rc::downgrade(self);
        list.on_size(move |event|{if let Some(r)=weak.upgrade(){r.sync_styles_scrollbar();}list.refresh(false,None);event.skip(true);});
        list.on_set_focus(move |event|{list.refresh(false,None);event.skip(true);});
        list.on_kill_focus(move |event|{list.refresh(false,None);event.skip(true);});
        let weak=Rc::downgrade(self);
        list.on_mouse_left_down(move |event|{
            if let Some(r)=weak.upgrade(){
                list.set_focus();
                let position=match &event{WindowEventData::MouseButton(m)=>m.event.get_position(),_=>None};
                if let Some(index)=position.and_then(|p|r.styles_row_at(p.y)){
                    let now=Instant::now();
                    let double=r.styles_view.borrow().last_click.is_some_and(|(i,t)|i==index&&now.duration_since(t)<DOUBLE_CLICK);
                    r.styles_view.borrow_mut().last_click=if double{None}else{Some((index,now))};
                    r.select_style_row(index);
                    if double{r.run(Action::StyleApply);}
                }
            }
            event.skip(true);
        });
        let weak=Rc::downgrade(self);
        list.on_mouse_right_up(move |event|{
            let Some(r)=weak.upgrade() else{return;};
            let point=match &event{WindowEventData::MouseButton(m)=>m.event.get_position(),_=>None};
            let Some(point)=point else{return;};
            let Some(index)=r.styles_row_at(point.y) else{return;};
            r.select_style_row(index);
            let id=r.styles_view.borrow().selected_id();
            if let Some(id)=id {r.show_style_context(id,crate::gtk_native::to_screen(&list,point));}
            event.skip(false);
        });
        let weak=Rc::downgrade(self);
        list.on_mouse_motion(move |event|{
            if let Some(r)=weak.upgrade(){
                let position=match &event{WindowEventData::MouseMotion(m)=>m.event.get_position(),WindowEventData::MouseButton(m)=>m.event.get_position(),_=>None};
                let hover=position.and_then(|p|r.styles_row_at(p.y));
                if r.styles_view.borrow().hover!=hover{r.styles_view.borrow_mut().hover=hover;list.refresh(false,None);}
            }
            event.skip(true);
        });
        let weak=Rc::downgrade(self);
        list.on_mouse_leave(move |event|{if let Some(r)=weak.upgrade(){r.styles_view.borrow_mut().hover=None;list.refresh(false,None);}event.skip(true);});
        let weak=Rc::downgrade(self);
        list.on_mouse_wheel(move |event|{
            if let Some(r)=weak.upgrade(){
                let (rotation,delta)=match &event{
                    WindowEventData::MouseButton(m)=>(m.event.get_wheel_rotation(),m.event.get_wheel_delta()),
                    WindowEventData::MouseMotion(m)=>(m.event.get_wheel_rotation(),m.event.get_wheel_delta()),
                    _=>(0,0)};
                if rotation!=0{let notches=rotation as f32/delta.max(1) as f32;
                    let row=list.from_dip(Size::new(ROW_DIP,ROW_DIP)).height as f32;
                    r.scroll_styles_list(-(notches*3.0*row).round() as i32);}
            }
        });
        let bar=self.ui.styles_scroll;
        let weak=Rc::downgrade(self);
        bar.on_thumb_track(move |event|{if let (Some(r),Some(position))=(weak.upgrade(),event.get_position()){r.scroll_styles_to(position);}});
        let weak=Rc::downgrade(self);
        bar.on_thumb_release(move |event|{if let (Some(r),Some(position))=(weak.upgrade(),event.get_position()){r.scroll_styles_to(position);}});
        for (up,page) in [(true,false),(false,false),(true,true),(false,true)] {
            let weak=Rc::downgrade(self);
            let callback=move |_|{if let Some(r)=weak.upgrade(){
                let height=r.ui.styles_list.get_client_size().height;
                let row=r.ui.styles_list.from_dip(Size::new(ROW_DIP,ROW_DIP)).height;
                r.scroll_styles_list(if up{-1}else{1}*if page{height-row}else{row});
            }};
            match (up,page){
                (true,false)=>bar.on_scroll_lineup(callback),
                (false,false)=>bar.on_scroll_linedown(callback),
                (true,true)=>bar.on_scroll_pageup(callback),
                (false,true)=>bar.on_scroll_pagedown(callback),
            };
        }
        let weak=Rc::downgrade(self);
        bar.on_scroll_top(move |_|{if let Some(r)=weak.upgrade(){r.scroll_styles_to(0);}});
        let weak=Rc::downgrade(self);
        bar.on_scroll_bottom(move |_|{if let Some(r)=weak.upgrade(){r.scroll_styles_to(i32::MAX);}});
        let weak=Rc::downgrade(self);
        list.on_key_down(move |event|{
            if let Some(r)=weak.upgrade(){
                if r.handle_find_shortcut(&event){return;}
                if let WindowEventData::Keyboard(key)=&event {
                    let count=r.styles_view.borrow().rows.len();
                    let current=r.styles_view.borrow().selected;
                    let page=(list.get_client_size().height/list.from_dip(Size::new(ROW_DIP,ROW_DIP)).height).max(1) as usize;
                    let target=match key.get_key_code(){
                        Some(WXK_UP)=>Some(current.map_or(0,|i|i.saturating_sub(1))),
                        Some(WXK_DOWN)=>Some(current.map_or(0,|i|i+1)),
                        Some(WXK_PAGEUP)=>Some(current.map_or(0,|i|i.saturating_sub(page))),
                        Some(WXK_PAGEDOWN)=>Some(current.map_or(0,|i|i+page)),
                        Some(WXK_HOME)=>Some(0),
                        Some(WXK_END)=>Some(count.saturating_sub(1)),
                        Some(WXK_RETURN)=>{r.run(Action::StyleApply);event.skip(false);return;}
                        Some(WXK_ESCAPE)=>{r.show_styles_pane(false);event.skip(false);return;}
                        _=>None};
                    if let Some(target)=target{if count>0{r.select_style_row(target.min(count-1));}event.skip(false);return;}
                }
            }event.skip(true);
        });
    }
    pub(crate) fn show_styles_pane(&self,show:bool){
        if !show && self.ui.styles_resize_handle.has_capture(){self.ui.styles_resize_handle.release_mouse();}
        self.ui.styles_resize_handle.show(show);
        self.ui.styles_panel.show(show);self.styles_revision.set(None);
        self.ui.workspace_panel.layout();self.ui.body_panel.layout();
        if show {self.install_face(&self.ui.styles_close_button);self.install_face(&self.ui.styles_manage_button);self.ui.styles_panel.layout();self.refresh_styles_pane();}
        self.focus_document();
        self.state.borrow_mut().needs_present=true;
    }
    fn selected_pane_style(&self)->Result<Style,String>{
        let id=self.styles_view.borrow().selected_id().ok_or("Select a style in the Styles pane first")?;
        self.state.borrow().editor.document.styles.get(&id).cloned().ok_or_else(||"The selected style was removed".to_owned())
    }
    /// Row index under a client y coordinate, if any.
    fn styles_row_at(&self,y:i32)->Option<usize>{
        let list=self.ui.styles_list;let row=list.from_dip(Size::new(ROW_DIP,ROW_DIP)).height;
        let inset=list.from_dip(Size::new(INSET_DIP,INSET_DIP)).height;
        let view=self.styles_view.borrow();let offset=y-inset+view.scroll;
        (offset>=0).then(||(offset/row) as usize).filter(|&i|i<view.rows.len())
    }
    fn select_style_row(&self,index:usize){
        self.styles_view.borrow_mut().selected=Some(index);self.ensure_visible(index);
        self.ui.styles_list.refresh(false,None);self.update_style_commands();
    }
    fn scroll_styles_list(&self,delta:i32){
        let next=self.styles_view.borrow().scroll.saturating_add(delta);
        self.scroll_styles_to(next);
    }
    fn scroll_styles_to(&self,position:i32){
        let list=self.ui.styles_list;let row=list.from_dip(Size::new(ROW_DIP,ROW_DIP)).height;
        let inset=list.from_dip(Size::new(INSET_DIP,INSET_DIP)).height;
        let mut view=self.styles_view.borrow_mut();
        let content=view.rows.len() as i32*row+2*inset;let max=(content-list.get_client_size().height).max(0);
        let next=position.clamp(0,max);
        if next!=view.scroll{view.scroll=next;drop(view);self.sync_styles_scrollbar();list.refresh(false,None);}
    }
    /// Scroll just enough to show the whole row, like a native list.
    fn ensure_visible(&self,index:usize){
        let list=self.ui.styles_list;let row=list.from_dip(Size::new(ROW_DIP,ROW_DIP)).height;
        let inset=list.from_dip(Size::new(INSET_DIP,INSET_DIP)).height;
        let height=list.get_client_size().height;
        let (top,bottom)=(index as i32*row,index as i32*row+row+2*inset);
        let mut view=self.styles_view.borrow_mut();
        if top<view.scroll{view.scroll=top;}else if bottom>view.scroll+height{view.scroll=(bottom-height).max(0);}
        drop(view);self.sync_styles_scrollbar();
    }
    fn sync_styles_scrollbar(&self){
        if !self.ui.styles_panel.is_shown(){return;}
        let list=self.ui.styles_list;
        let row=list.from_dip(Size::new(ROW_DIP,ROW_DIP)).height;
        let inset=list.from_dip(Size::new(INSET_DIP,INSET_DIP)).height;
        let height=list.get_client_size().height.max(1);
        let (content,position)={let mut view=self.styles_view.borrow_mut();
            let content=view.rows.len() as i32*row+2*inset;
            view.scroll=view.scroll.clamp(0,(content-height).max(0));
            (content,view.scroll)};
        let overflow=content>height;
        let bar=self.ui.styles_scroll;
        if bar.is_shown()!=overflow{bar.show(overflow);self.ui.styles_panel.layout();}
        if overflow{bar.set_scrollbar(position,height,content,height,true);}
    }
    fn paint_styles_list(&self){
        let list=self.ui.styles_list;let dc=AutoBufferedPaintDC::new(&list);
        let size=list.get_client_size();
        let dip=|v:i32|list.from_dip(Size::new(v,v)).width;
        let focused=list.has_focus();
        paint_rounded_surface(&dc,&list,Colour::rgb(255,255,255),if focused{Colour::rgb(58,104,194)}else{Colour::rgb(223,228,236)},5);
        let view=self.styles_view.borrow();
        let (row_h,inset,pad)=(dip(ROW_DIP),dip(INSET_DIP),dip(10));
        let width=size.width-2*inset;
        dc.set_clipping_region(inset/2,inset/2,size.width-inset,size.height-inset);
        let marker_font=face_font(9,false,false,false,"Segoe UI");
        if view.rows.is_empty(){
            if let Some(font)=&marker_font{dc.set_font(font);}
            dc.set_text_foreground(rgb(MUTED));dc.draw_text(&crate::i18n::text(&("No styles match this filter")),inset+pad,inset+(row_h-dc.get_text_extent(&crate::i18n::text(&("Hg"))).1)/2);
        }
        for (index,row) in view.rows.iter().enumerate(){
            let y=inset+index as i32*row_h-view.scroll;
            if y+row_h<0||y>size.height{continue;}
            let selected=view.selected==Some(index);
            if selected||view.hover==Some(index){
                let (fill,border)=if selected{(Colour::rgb(229,237,252),Colour::rgb(181,200,238))}else{(Colour::rgb(238,242,248),Colour::rgb(238,242,248))};
                dc.set_brush(fill,BrushStyle::Solid);dc.set_pen(border,dip(1).max(1),PenStyle::Solid);
                dc.draw_rounded_rectangle(inset,y+1,width,row_h-2,dip(5) as f64);
            }
            // Kind marker at the right, in the interface font.
            let marker=if row.kind==StyleKind::Paragraph{"¶"}else{"a"};
            if let Some(font)=&marker_font{dc.set_font(font);}
            let (mw,mh)=dc.get_text_extent(&crate::i18n::text(&(marker)));
            dc.set_text_foreground(if selected{Colour::rgb(29,61,120)}else{rgb(MUTED)});
            let right=inset+width-pad;dc.draw_text(&crate::i18n::text(&(marker)),right-mw,y+(row_h-mh)/2);
            // Name in the style's own face.
            if let Some(font)=face_font(row.points,row.bold,row.italic,row.underline,&row.face){dc.set_font(&font);}
            let avail=right-mw-dip(8)-(inset+pad);
            let name=fit_text(&dc,&row.name,avail);
            let th=dc.get_text_extent(&crate::i18n::text(&("Hg"))).1;
            dc.set_text_foreground(row.colour.unwrap_or(rgb(INK)));
            dc.draw_text(&crate::i18n::text(&(&name)),inset+pad,y+(row_h-th)/2);
        }
        dc.destroy_clipping_region();
    }
    pub(crate) fn refresh_styles_pane(&self){
        if !self.ui.styles_panel.is_shown(){return;}
        let revision=self.state.borrow().editor.revision;
        if self.styles_revision.get()==Some(revision){return;}
        // Typing changes the document revision but not its style definitions.
        // Rebuilding the list on every key reset its scroll/selection and made
        // the ribbon poll do unnecessary widget work during text input.
        if self.styles_revision.get().is_some() && self.styles_snapshot.borrow().as_ref().is_some_and(|prior|
            *prior==self.state.borrow().editor.document.styles) {
            self.styles_revision.set(Some(revision));
            return;
        }
        let selected=self.styles_view.borrow().selected_id();
        let (styles,current,rows)={let s=self.state.borrow();let sheet=&s.editor.document.styles;
            let styles=filtered_styles(&s.editor.document,&crate::i18n::choice_source(&self.ui.styles_filter.get_value(), &["All styles", "In use", "Paragraph styles", "Character styles"]));
            let rows=styles.iter().map(|style|{let c=sheet.resolve_character(Some(&style.id));
                StyleRow{id:style.id.clone(),name:crate::i18n::style_name(style),kind:style.kind,
                    face:c.font_family.clone().unwrap_or_else(||"Linux Libertine G".into()),
                    points:c.font_size_pt.unwrap_or(11.0).clamp(9.0,13.0).round() as i32,
                    bold:c.bold==Some(true),italic:c.italic==Some(true),underline:c.underline==Some(true),colour:hex(c.color.as_deref())}}).collect::<Vec<_>>();
            (styles,s.editor.formatting_summary().style.clone(),rows)};
        self.styles_syncing.set(true);
        let selected=selected.or(current);
        let index=selected.as_ref().and_then(|id|styles.iter().position(|s|&s.id==id)).or_else(||(!styles.is_empty()).then_some(0));
        {let mut view=self.styles_view.borrow_mut();view.rows=rows;view.selected=index;view.hover=None;view.last_click=None;view.scroll=0;}
        if let Some(index)=index{self.ensure_visible(index);}
        self.sync_styles_scrollbar();
        self.ui.styles_list.refresh(false,None);
        self.styles_syncing.set(false);self.styles_revision.set(Some(revision));
        self.styles_snapshot.replace(Some(self.state.borrow().editor.document.styles.clone()));
        self.update_style_commands();
    }
    /// Commands that need a selected style follow the list selection.
    fn update_style_commands(&self){
        let selected=self.styles_view.borrow().has_selection();
        for b in [self.ui.styles_apply_button,self.ui.styles_modify_button,self.ui.styles_inspect_button,self.ui.styles_delete_button]{b.enable(selected);}
    }
    pub(crate) fn apply_pane_style(&self)->Result<(),String>{
        let style=self.selected_pane_style()?;let clear=self.ui.styles_clear_direct.is_checked();
        self.edit(|e|e.apply_named_style(&style.id,clear))
    }
    pub(crate) fn create_pane_style(&self)->Result<(),String>{
        let options=["Paragraph style".into(),"Character style".into()];
        let Some(kind)=self.choose_review("New style","Choose the style type:",&options)else{return Ok(());};
        let style=Style{id:StyleId::new(),name:"New style".into(),kind:if kind==0{StyleKind::Paragraph}else{StyleKind::Character},
            based_on:if kind==0{Some(StyleId::from(model::STYLE_NORMAL))}else{None},next_style:None,character:Default::default(),paragraph:Default::default(),formatting:Default::default()};
        let id=style.id.clone();if !self.edit_style(style)?{return Ok(());}self.styles_revision.set(None);self.refresh_styles_pane();
        let row=self.styles_view.borrow().rows.iter().position(|r|r.id==id);if let Some(i)=row{self.select_style_row(i);}
        self.update_style_commands();Ok(())
    }
    pub(crate) fn modify_pane_style(&self)->Result<(),String>{let style=self.selected_pane_style()?;if self.edit_style(style)?{self.styles_revision.set(None);self.refresh_styles_pane();}Ok(())}
    pub(crate) fn delete_pane_style(&self)->Result<(),String>{
        let style=self.selected_pane_style()?;let choices=["Cancel".into(),"Delete unused style".into()];
        if self.choose_review("Delete style",&format!("Delete {}? Styles in use, base styles and protected built-ins cannot be deleted.",style.name),&choices)==Some(1){
            self.edit(|e|e.remove_unused_style(&style.id))?;self.styles_revision.set(None);self.refresh_styles_pane();
        }Ok(())
    }
    pub(crate) fn inspect_pane_style(&self){
        let text={let s=self.state.borrow();let summary=s.editor.formatting_summary();
            format!("Current selection\nParagraph style: {:?}\nCharacter formatting: {:#?}\nAlignment: {:?}\nDirect formatting is identified in the document context strip.",summary.style,summary.character,summary.alignment)};
        self.review_info("Style inspector",&text);
    }
}
}

pub(super) use dialog::*;
pub(super) use pane::*;
