//! The picture Layout dialog (Wrap Text > More Layout Options…), as in
//! Word: Position (alignment or absolute position along each axis, Move
//! object with text, Lock anchor), Text Wrapping (style, distance from text)
//! and Size (height, width, lock aspect ratio). OK applies everything as one
//! undo step; a value that cannot be used is reported instead.
//!
//! `LayoutForm` holds the dialog's values and the position arithmetic
//! without widgets, so they are testable; `show` wires it to the XRC controls.
use super::*;
use super::format_dialogs::Chrome;
use super::picture_layout::{paint_wrap, WRAPS, DEFAULT_GAP_MM};
use crate::LayoutDialogUi;
use model::{AnchorReference, ImageAnchor, ImageWrap, SectionProperties};
use tiny_skia::Pixmap;
use wxdragon::{dc::AutoBufferedPaintDC, event::window_events::WindowEvents, window::BackgroundStyle, DeviceContext};

const MM_TO_PT: f32 = 72.0 / 25.4;
const MM_PER_CM: f32 = 10.0;
/// Wrapping style tiles: width, picture size and caption height, in DIP.
const STYLE_TILE: i32 = 76;
const STYLE_PICTURE: (i32, i32) = (48, 40);
const STYLE_SELECTED: (u8, u8, u8) = (43, 111, 232);
const STYLE_HOVER: (u8, u8, u8) = (226, 236, 253);

/// What a position along one axis is measured from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum From { Margin, Page, Paragraph }

/// How the picture is placed along one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Axis {
    /// Start (left/top), centre or end (right/bottom) of the margins or page.
    Align(u8, From),
    /// A distance in millimetres from the page, margin or paragraph.
    Absolute(f32, From),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct LayoutForm {
    pub wrap: ImageWrap,
    pub horizontal: Axis,
    pub vertical: Axis,
    pub move_with_text: bool,
    pub lock: bool,
    pub gap_mm: f32,
    pub size_mm: (f32, f32),
    pub lock_ratio: bool,
}

impl LayoutForm {
    /// The picture's top-left corner on its page, in points, for the
    /// section's page setup and the anchor paragraph's top (points).
    pub fn page_origin(&self, section: &SectionProperties, paragraph_top_pt: f32) -> (f32, f32) {
        let along = |axis: Axis, page: f32, start: f32, end: f32, size: f32, paragraph: f32| -> f32 {
            match axis {
                Axis::Align(at, from) => {
                    let (low, high) = if from == From::Page { (0.0, page) } else { (start, page - end) };
                    (low + (high - low - size).max(0.0) * f32::from(at.min(2)) / 2.0) * MM_TO_PT
                }
                Axis::Absolute(mm, From::Page) => mm * MM_TO_PT,
                Axis::Absolute(mm, From::Margin) => (start + mm) * MM_TO_PT,
                Axis::Absolute(mm, From::Paragraph) => paragraph + mm * MM_TO_PT,
            }
        };
        (along(self.horizontal, section.page_width_mm, section.margin_left_mm, section.margin_right_mm, self.size_mm.0, section.margin_left_mm * MM_TO_PT),
         along(self.vertical, section.page_height_mm, section.margin_top_mm, section.margin_bottom_mm, self.size_mm.1, paragraph_top_pt))
    }

    /// A new height or width; with the ratio locked the other follows.
    pub fn set_size(&mut self, width: Option<f32>, height: Option<f32>) {
        let (old_w, old_h) = self.size_mm;
        match (width, height) {
            (Some(w), None) => self.size_mm = (w, if self.lock_ratio && old_w > 0.0 { old_h * w / old_w } else { old_h }),
            (None, Some(h)) => self.size_mm = (if self.lock_ratio && old_h > 0.0 { old_w * h / old_h } else { old_w }, h),
            (Some(w), Some(h)) => self.size_mm = (w, h),
            (None, None) => {}
        }
    }
}

/// A length typed in centimetres ("2,5" or "2.5"), in millimetres.
fn parse_cm(text: &str) -> Option<f32> {
    text.trim().trim_end_matches("cm").trim().replace(',', ".").parse::<f32>().ok().filter(|v| v.is_finite()).map(|v| v * MM_PER_CM)
}

fn format_cm(mm: f32) -> String { format!("{:.2}", mm / MM_PER_CM).replace('.', ",") }

fn style_at(panel: &Panel, x: i32) -> Option<usize> {
    let index = (x / panel.from_dip_int(STYLE_TILE)).max(0) as usize;
    (index < WRAPS.len()).then_some(index)
}

fn paint_styles(panel: Panel, selected: ImageWrap, hovered: Option<usize>) {
    let dc = AutoBufferedPaintDC::new(&panel);
    let size = panel.get_client_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let d = |v: i32| panel.from_dip_int(v);
    let Some((mut pixmap, canvas)) = crate::crisp::Canvas::new(&panel, width as i32, height as i32) else { return; };
    pixmap.fill(tiny_skia::Color::WHITE);
    let fill = |pixmap: &mut Pixmap, x: i32, y: i32, w: i32, h: i32, (r, g, b): (u8, u8, u8)| {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(r, g, b, 255);
        if let Some(rect) = tiny_skia::Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
            pixmap.fill_path(&tiny_skia::PathBuilder::from_rect(rect), &paint, tiny_skia::FillRule::Winding, crate::crisp::transform(), None);
        }
    };
    for (index, (wrap, ..)) in WRAPS.iter().enumerate() {
        let x = index as i32 * d(STYLE_TILE);
        if *wrap == selected || hovered == Some(index) {
            let colour = if *wrap == selected { STYLE_SELECTED } else { STYLE_HOVER };
            fill(&mut pixmap, x + d(4), d(2), d(STYLE_TILE) - d(8), height - d(4), colour);
            fill(&mut pixmap, x + d(6), d(4), d(STYLE_TILE) - d(12), height - d(8), (255, 255, 255));
        }
        let (w, h) = (d(STYLE_PICTURE.0) as f32, d(STYLE_PICTURE.1) as f32);
        paint_wrap(&mut pixmap, (x as f32 + (d(STYLE_TILE) as f32 - w) / 2.0, d(10) as f32, w, h), *wrap);
    }
    canvas.present(&dc, &pixmap, 0, 0);
    dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
    if let Some(font) = wxdragon::font::Font::new_with_details(8, wxdragon::font::FontFamily::Default as i32, wxdragon::font::FontStyle::Normal as i32, wxdragon::font::FontWeight::Normal as i32, false, "") { dc.set_font(&font); }
    dc.set_text_foreground(Colour::rgb(36, 48, 66));
    for (index, (_, name, _)) in WRAPS.iter().enumerate() {
        // Long names take two lines, as in Word's tiles.
        let lines: Vec<&str> = if name.len() > 12 { name.splitn(2, ' ').collect() } else { vec![name] };
        let top = d(10) + d(STYLE_PICTURE.1) + d(6);
        for (row, line) in lines.iter().enumerate() {
            let (text_width, text_height) = dc.get_text_extent(&crate::i18n::text(&(line)));
            dc.draw_text(&crate::i18n::text(&(line)), index as i32 * d(STYLE_TILE) + (d(STYLE_TILE) - text_width) / 2, top + row as i32 * text_height);
        }
    }
}

/// Opens the Layout dialog for picture `id`.
pub(super) fn show(ribbon: &Ribbon, id: &NodeId) -> Result<(), String> {
    let (section, size, anchor, wrap) = {
        let state = ribbon.state.borrow();
        (state.editor.picture_section(id).cloned().ok_or("Only pictures in the body text have layout options")?,
         state.editor.picture_size(id).ok_or("Picture not found")?,
         state.editor.picture_anchor(id).cloned(), state.editor.picture_wrap(id))
    };
    let shown = objects_ui::region_for(&ribbon.state.borrow(), id).map(|(page, region)| (page, region.rect));
    let paragraph = anchor.as_ref().map(|a| a.paragraph.clone()).or_else(|| ribbon.state.borrow().editor.picture_body_paragraph(id));
    let paragraph_top = shown.zip(paragraph.as_ref()).and_then(|((page, _), p)| picture_layout::paragraph_origin(ribbon, page, p)).map(|o| o.1);
    let move_with_text = anchor.as_ref().map_or(true, |a| a.reference == AnchorReference::Paragraph);
    let (x, y) = shown.map_or((section.margin_left_mm * MM_TO_PT, section.margin_top_mm * MM_TO_PT), |(_, r)| (r.x, r.y));
    let form = Rc::new(RefCell::new(LayoutForm {
        wrap,
        horizontal: Axis::Absolute(x / MM_TO_PT, From::Page),
        vertical: match paragraph_top { Some(top) if move_with_text => Axis::Absolute((y - top) / MM_TO_PT, From::Paragraph), _ => Axis::Absolute(y / MM_TO_PT, From::Page) },
        move_with_text, lock: anchor.as_ref().is_some_and(|a| a.locked),
        gap_mm: anchor.as_ref().map_or(DEFAULT_GAP_MM, |a| a.gap_mm), size_mm: size, lock_ratio: true,
    }));

    let ui = Rc::new(crate::i18n::localized(LayoutDialogUi::new(Some(&ribbon.ui.main_frame), false)));
    let chrome = Chrome::new(ui.lay_dialog, ui.lay_card, &[(ui.lay_tab_0, "Position"), (ui.lay_tab_1, "Text Wrapping"), (ui.lay_tab_2, "Size")],
        &[ui.lay_page_0, ui.lay_page_1, ui.lay_page_2], (ui.lay_ok, "OK"), Some(ui.lay_cancel), &[], &[]);
    let hovered = Rc::new(Cell::new(None::<usize>));

    // Show the form in the controls.
    let show_form = { let ui = Rc::clone(&ui); let form = Rc::clone(&form); move || {
        let form = form.borrow();
        let index_of = |from: From, choices: &[From]| choices.iter().position(|c| *c == from).unwrap_or(0) as u32;
        match form.horizontal {
            Axis::Align(at, from) => { ui.lay_h_align.set_value(true); ui.lay_h_align_value.set_selection(u32::from(at)); ui.lay_h_align_from.set_selection(index_of(from, &[From::Margin, From::Page])); }
            Axis::Absolute(mm, from) => { ui.lay_h_abs.set_value(true); ui.lay_h_abs_value.change_value(&format_cm(mm)); ui.lay_h_abs_from.set_selection(index_of(from, &[From::Page, From::Margin])); }
        }
        match form.vertical {
            Axis::Align(at, from) => { ui.lay_v_align.set_value(true); ui.lay_v_align_value.set_selection(u32::from(at)); ui.lay_v_align_from.set_selection(index_of(from, &[From::Margin, From::Page])); }
            Axis::Absolute(mm, from) => { ui.lay_v_abs.set_value(true); ui.lay_v_abs_value.change_value(&format_cm(mm)); ui.lay_v_abs_from.set_selection(index_of(from, &[From::Page, From::Margin, From::Paragraph])); }
        }
        // Defaults for the way not chosen, so switching shows sensible values.
        if ui.lay_h_align_value.get_selection().is_none() { ui.lay_h_align_value.set_selection(0); ui.lay_h_align_from.set_selection(0); }
        if ui.lay_v_align_value.get_selection().is_none() { ui.lay_v_align_value.set_selection(0); ui.lay_v_align_from.set_selection(0); }
        if ui.lay_h_abs_value.get_value().is_empty() { ui.lay_h_abs_value.change_value("0"); ui.lay_h_abs_from.set_selection(1); }
        if ui.lay_v_abs_value.get_value().is_empty() { ui.lay_v_abs_value.change_value("0"); ui.lay_v_abs_from.set_selection(2); }
        ui.lay_move.set_value(form.move_with_text);
        ui.lay_lock.set_value(form.lock);
        ui.lay_gap.change_value(&format_cm(form.gap_mm));
        ui.lay_width.change_value(&format_cm(form.size_mm.0));
        ui.lay_height.change_value(&format_cm(form.size_mm.1));
        ui.lay_ratio.set_value(form.lock_ratio);
        // In line with the text, a picture has no position or distance.
        let floating = form.wrap != ImageWrap::Inline;
        for control in [ui.lay_h_align_value, ui.lay_h_align_from, ui.lay_h_abs_from, ui.lay_v_align_value, ui.lay_v_align_from, ui.lay_v_abs_from] { control.enable(floating); }
        for control in [ui.lay_h_abs_value, ui.lay_v_abs_value, ui.lay_gap] { control.enable(floating); }
        for control in [ui.lay_h_align, ui.lay_h_abs, ui.lay_v_align, ui.lay_v_abs] { control.enable(floating); }
        for control in [ui.lay_move, ui.lay_lock] { control.enable(floating); }
        ui.lay_styles.refresh(false, None);
    } };
    show_form();

    // Read the controls back into the form; None names what is wrong.
    let read = { let ui = Rc::clone(&ui); let form = Rc::clone(&form); move || -> Result<(), String> {
        let mut form = form.borrow_mut();
        let pick = |index: Option<u32>, choices: &[From]| choices[(index.unwrap_or(0) as usize).min(choices.len() - 1)];
        let at = |index: Option<u32>| index.unwrap_or(0).min(2) as u8;
        form.horizontal = if ui.lay_h_align.get_value() { Axis::Align(at(ui.lay_h_align_value.get_selection()), pick(ui.lay_h_align_from.get_selection(), &[From::Margin, From::Page])) }
            else { Axis::Absolute(parse_cm(&ui.lay_h_abs_value.get_value()).ok_or("Enter the horizontal position in centimetres")?, pick(ui.lay_h_abs_from.get_selection(), &[From::Page, From::Margin])) };
        form.vertical = if ui.lay_v_align.get_value() { Axis::Align(at(ui.lay_v_align_value.get_selection()), pick(ui.lay_v_align_from.get_selection(), &[From::Margin, From::Page])) }
            else { Axis::Absolute(parse_cm(&ui.lay_v_abs_value.get_value()).ok_or("Enter the vertical position in centimetres")?, pick(ui.lay_v_abs_from.get_selection(), &[From::Page, From::Margin, From::Paragraph])) };
        form.move_with_text = ui.lay_move.get_value();
        form.lock = ui.lay_lock.get_value();
        form.gap_mm = parse_cm(&ui.lay_gap.get_value()).filter(|v| (0.0..=50.0).contains(v)).ok_or("Distance from text is 0 to 5 cm")?;
        let width = parse_cm(&ui.lay_width.get_value()).filter(|v| (1.0..=1000.0).contains(v)).ok_or("Width is 0,1 to 100 cm")?;
        let height = parse_cm(&ui.lay_height.get_value()).filter(|v| (1.0..=1000.0).contains(v)).ok_or("Height is 0,1 to 100 cm")?;
        form.size_mm = (width, height);
        form.lock_ratio = ui.lay_ratio.get_value();
        Ok(())
    } };

    // Wrapping style tiles.
    let styles = ui.lay_styles;
    styles.set_background_style(BackgroundStyle::Paint);
    { let form = Rc::clone(&form); let hovered = Rc::clone(&hovered); styles.on_paint(move |_| paint_styles(styles, form.borrow().wrap, hovered.get())); }
    { let hovered = Rc::clone(&hovered); styles.on_mouse_motion(move |event| {
        let WindowEventData::MouseMotion(m) = &event else { return; };
        let next = m.event.get_position().and_then(|p| style_at(&styles, p.x));
        if hovered.replace(next) != next {
            styles.set_tooltip(&crate::i18n::text(&(next.map_or("", |i| WRAPS[i].2))));
            styles.refresh(false, None);
        }
    }); }
    { let form = Rc::clone(&form); let read = read.clone(); let show_form = show_form.clone(); styles.on_mouse_left_down(move |event| {
        let WindowEventData::MouseButton(m) = &event else { return; };
        let Some(index) = m.event.get_position().and_then(|p| style_at(&styles, p.x)) else { return; };
        let _ = read();
        form.borrow_mut().wrap = WRAPS[index].0;
        show_form();
    }); }

    // With the ratio locked, a new width sets the height and the other way round.
    for (field, is_width) in [(ui.lay_width, true), (ui.lay_height, false)] {
        let ui2 = Rc::clone(&ui); let form = Rc::clone(&form);
        field.on_text_updated(move |_| {
            let Some(value) = parse_cm(&field.get_value()).filter(|v| *v >= 1.0) else { return; };
            let mut form = form.borrow_mut();
            form.lock_ratio = ui2.lay_ratio.get_value();
            if is_width { form.set_size(Some(value), None); ui2.lay_height.change_value(&format_cm(form.size_mm.1)); }
            else { form.set_size(None, Some(value)); ui2.lay_width.change_value(&format_cm(form.size_mm.0)); }
        });
    }
    // Typing a position chooses "Absolute position" on that axis.
    { let ui2 = Rc::clone(&ui); ui.lay_h_abs_value.on_text_updated(move |_| ui2.lay_h_abs.set_value(true)); }
    { let ui2 = Rc::clone(&ui); ui.lay_v_abs_value.on_text_updated(move |_| ui2.lay_v_abs.set_value(true)); }

    let accepted = chrome.show();
    let values = if accepted { read() } else { Ok(()) };
    ui.lay_dialog.destroy();
    if !accepted { return Ok(()); }
    values?;
    let form = form.borrow().clone();
    let anchor = if form.wrap == ImageWrap::Inline { None } else {
        let paragraph = paragraph.ok_or("Only pictures in the body text can float")?;
        let (page_x, page_y) = form.page_origin(&section, paragraph_top.unwrap_or(section.margin_top_mm * MM_TO_PT));
        // Moving with the text measures from the anchor paragraph, as layout does.
        let (reference, x_mm, y_mm) = match paragraph_top {
            Some(top) if form.move_with_text => (AnchorReference::Paragraph, page_x / MM_TO_PT - section.margin_left_mm, (page_y - top) / MM_TO_PT),
            _ => (AnchorReference::Page, page_x / MM_TO_PT, page_y / MM_TO_PT),
        };
        Some(ImageAnchor { paragraph, reference, x_mm, y_mm, gap_mm: form.gap_mm, locked: form.lock })
    };
    ribbon.edit(|e| e.lay_out_picture(id, form.wrap, anchor, Some(form.size_mm)))
}

