//! Recover Documents: the unsaved copies Impress kept, as cards with a
//! preview of the selected one. Restore opens it as a new unsaved document;
//! Delete discards a copy; Later keeps them all for next time.
use super::*;
use super::format_dialogs::Chrome;
use super::output_dialogs::{preview_layout, PagePreview};
use crate::recovery::Candidate;
use crate::RecoveryDialogUi;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke};
use wxdragon::{
    dc::AutoBufferedPaintDC, event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    window::BackgroundStyle, DeviceContext,
};

const CARD_H: i32 = 62;

fn age(seconds: u64) -> String {
    match seconds {
        0..=59 => "Saved just now".into(),
        60..=3599 => { let m = seconds / 60; format!("Saved {m} minute{} ago", if m == 1 { "" } else { "s" }) }
        3600..=86_399 => { let h = seconds / 3600; format!("Saved {h} hour{} ago", if h == 1 { "" } else { "s" }) }
        _ => { let d = seconds / 86_400; format!("Saved {d} day{} ago", if d == 1 { "" } else { "s" }) }
    }
}

fn paint_list(panel: &wxdragon::widgets::panel::Panel, candidates: &[Candidate], selected: usize, hover: Option<usize>) {
    let dc = AutoBufferedPaintDC::new(panel);
    let size = panel.get_client_size();
    let (w, h) = (size.width.max(1), size.height.max(1));
    let s = panel.from_dip_int(1000) as f32 / 1000.0;
    let d = |v: i32| panel.from_dip_int(v);
    let Some((mut pix, canvas)) = crate::crisp::Canvas::new(panel, w as i32, h as i32) else { return; };
    pix.fill(tiny_skia::Color::WHITE);
    let round = |pix: &mut Pixmap, r: (f32, f32, f32, f32), fill: Option<(u8, u8, u8)>, stroke: Option<(u8, u8, u8)>| {
        let (x, y, rw, rh) = r;
        let radius = 6.0 * s;
        let mut p = PathBuilder::new();
        p.move_to(x + radius, y); p.line_to(x + rw - radius, y); p.quad_to(x + rw, y, x + rw, y + radius);
        p.line_to(x + rw, y + rh - radius); p.quad_to(x + rw, y + rh, x + rw - radius, y + rh);
        p.line_to(x + radius, y + rh); p.quad_to(x, y + rh, x, y + rh - radius);
        p.line_to(x, y + radius); p.quad_to(x, y, x + radius, y); p.close();
        let Some(path) = p.finish() else { return; };
        let paint = |c: (u8, u8, u8)| { let mut p = Paint::default(); p.set_color_rgba8(c.0, c.1, c.2, 255); p.anti_alias = true; p };
        if let Some(c) = fill { pix.fill_path(&path, &paint(c), FillRule::Winding, crate::crisp::transform(), None); }
        if let Some(c) = stroke { pix.stroke_path(&path, &paint(c), &Stroke { width: s.max(1.0), ..Stroke::default() }, crate::crisp::transform(), None); }
    };
    for (i, _) in candidates.iter().enumerate() {
        let y = (i as i32 * d(CARD_H + 6)) as f32;
        if y > h as f32 { break; }
        let rect = (0.5 * s, y + 0.5 * s, w as f32 - s, d(CARD_H) as f32);
        let (fill, line) = if i == selected { ((232, 240, 254), (120, 160, 230)) } else if hover == Some(i) { ((245, 248, 253), (214, 222, 234)) } else { ((255, 255, 255), (223, 228, 236)) };
        round(&mut pix, rect, Some(fill), Some(line));
        // A small document icon.
        let (ix, iy) = (d(14) as f32, y + d(14) as f32);
        round(&mut pix, (ix, iy, 22.0 * s, 28.0 * s), Some((255, 255, 255)), Some((120, 160, 230)));
        for k in 0..3 { round(&mut pix, (ix + 5.0 * s, iy + (8.0 + 5.0 * k as f32) * s, 12.0 * s, 1.5 * s), Some((150, 180, 235)), None); }
    }
    canvas.present(&dc, &pix, 0, 0);
    dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
    let font = |bold: bool, points: i32| Font::new_with_details(points, FontFamily::Default as i32, FontStyle::Normal as i32,
        if bold { FontWeight::Bold } else { FontWeight::Normal } as i32, false, "Segoe UI");
    for (i, c) in candidates.iter().enumerate() {
        let y = i as i32 * d(CARD_H + 6);
        let x = d(50);
        if let Some(f) = font(true, 10) { dc.set_font(&f); }
        dc.set_text_foreground(Colour::rgb(36, 48, 66));
        dc.draw_text(&crate::i18n::text(&(&c.title)), x, y + d(10));
        if let Some(f) = font(false, 9) { dc.set_font(&f); }
        dc.set_text_foreground(Colour::rgb(80, 96, 120));
        dc.draw_text(&crate::i18n::text(&(&age(c.age_seconds))), x, y + d(28));
        dc.set_text_foreground(Colour::rgb(130, 142, 160));
        let mut place = c.original.as_ref().and_then(|p| p.parent()).map(|p| p.display().to_string()).unwrap_or_else(|| "Never saved".into());
        let limit = w - x - d(10);
        if dc.get_text_extent(&crate::i18n::text(&(&place))).0 > limit {
            while place.chars().count() > 4 && dc.get_text_extent(&crate::i18n::text(&(&format!("…{place}")))).0 > limit { place.remove(0); }
            place.insert(0, '…');
        }
        dc.draw_text(&crate::i18n::text(&(&place)), x, y + d(44));
    }
}

/// Shows the Recover Documents dialog when snapshots exist. Returns the
/// restored document, if the user chose one.
pub fn recovery_dialog(frame: &impl WxWidget, state: &Rc<RefCell<CanvasState>>) -> Result<Option<Document>, String> {
    let formats = Arc::clone(&state.borrow().formats);
    let candidates = state.borrow().recovery.candidates(&formats);
    if candidates.is_empty() { return Ok(None); }
    let ui = Rc::new(crate::i18n::localized(RecoveryDialogUi::new(Some(frame), false)));
    let chrome = Chrome::new(ui.rec_dialog, ui.rec_card, &[], &[], (ui.rec_ok, "Restore"), None,
        &[(ui.rec_delete, "Delete"), (ui.rec_cancel, "Later")], &[]);
    ui.rec_dialog.set_escape_id(ui.rec_cancel.get_id());
    { let dialog = ui.rec_dialog; ui.rec_cancel.on_click(move |_| dialog.end_modal(wxdragon::id::ID_CANCEL)); }
    let candidates = Rc::new(RefCell::new(candidates));
    let selected = Rc::new(Cell::new(0usize));
    let hover = Rc::new(Cell::new(None::<usize>));
    let preview = PagePreview::install(ui.rec_preview, Arc::new(render::VectorDocument::default()));
    let show_preview: Rc<dyn Fn()> = { let candidates = Rc::clone(&candidates); let selected = Rc::clone(&selected); let preview = Rc::clone(&preview); let formats = Arc::clone(&formats);
        Rc::new(move || {
            let document = candidates.borrow().get(selected.get()).and_then(|c| crate::file_io::load_document(&c.path, &formats).ok());
            preview.show_document(document.map(|d| preview_layout(&d)).unwrap_or_default());
        }) };
    let list = ui.rec_list;
    list.set_background_style(BackgroundStyle::Paint);
    list.on_erase_background(|_| {});
    { let c = Rc::clone(&candidates); let sel = Rc::clone(&selected); let hov = Rc::clone(&hover);
      list.on_paint(move |_| paint_list(&list, &c.borrow(), sel.get(), hov.get())); }
    let index_at = move |y: i32| (y / list.from_dip_int(CARD_H + 6)).max(0) as usize;
    { let c = Rc::clone(&candidates); let hov = Rc::clone(&hover);
      list.on_mouse_motion(move |event| {
        let WindowEventData::MouseMotion(m) = &event else { return; };
        let Some(p) = m.event.get_position() else { return; };
        let i = Some(index_at(p.y)).filter(|i| *i < c.borrow().len());
        if hov.replace(i) != i { list.refresh(false, None); }
      }); }
    // A second click on the same card within the double-click time restores it.
    let last_click = Rc::new(Cell::new(None::<(usize, std::time::Instant)>));
    { let c = Rc::clone(&candidates); let sel = Rc::clone(&selected); let show = Rc::clone(&show_preview); let dialog = ui.rec_dialog;
      list.on_mouse_left_down(move |event| {
        let WindowEventData::MouseButton(m) = &event else { return; };
        let Some(p) = m.event.get_position() else { return; };
        let i = index_at(p.y);
        if i >= c.borrow().len() { return; }
        let now = std::time::Instant::now();
        if let Some((j, at)) = last_click.replace(Some((i, now))) {
            if j == i && now.duration_since(at).as_millis() < 450 { last_click.set(None); dialog.end_modal(wxdragon::id::ID_OK); return; }
        }
        if sel.replace(i) != i { list.refresh(false, None); show(); }
      }); }
    { let c = Rc::clone(&candidates); let sel = Rc::clone(&selected); let show = Rc::clone(&show_preview); let state = Rc::clone(state); let ui2 = Rc::clone(&ui);
      ui.rec_delete.on_click(move |_| {
        let confirm = MessageDialog::builder(&ui2.rec_dialog, &crate::i18n::text(&("Delete this recovered copy? This cannot be undone.")), &crate::i18n::text(&("Delete recovered copy")))
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconWarning).build();
        if confirm.show_modal() != wxdragon::id::ID_YES { return; }
        let index = sel.get();
        if index >= c.borrow().len() { return; }
        let removed = c.borrow_mut().remove(index);
        state.borrow().recovery.discard(removed);
        if c.borrow().is_empty() { ui2.rec_dialog.end_modal(wxdragon::id::ID_CANCEL); return; }
        sel.set(index.min(c.borrow().len() - 1));
        list.refresh(false, None); show();
      }); }
    show_preview();
    let accepted = chrome.show();
    ui.rec_dialog.destroy();
    if !accepted { return Ok(None); }
    let index = selected.get();
    let candidate = { let mut c = candidates.borrow_mut(); if index >= c.len() { return Ok(None); } c.remove(index) };
    let document = state.borrow_mut().recovery.restore(candidate, &formats)?;
    Ok(Some(document))
}
