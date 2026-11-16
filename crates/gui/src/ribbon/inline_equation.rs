//! Inline equations are edited in place: a compact box in the text line, over
//! the equation (or at the caret for a new one), holding its Typst math. The
//! equation is rendered when the box is confirmed: Enter, Tab, its ✓ button,
//! or a click anywhere outside it. Escape leaves the document as it was. An
//! emptied box removes the equation; source Typst cannot read keeps the box
//! open (or, closed by a click outside, leaves the document unchanged) and
//! says why. The box's equation button opens the full equation editor.
use super::*;
use editor::equation_input;
use model::{Equation, EquationDisplay};
use wxdragon::{
    event::{EventType, WxEvtHandler},
    geometry::{Point, Rect, Size},
    keycode::{WXK_ESCAPE, WXK_TAB},
    widgets::textctrl::{TextCtrl, TextCtrlStyle},
};

const HEIGHT: i32 = 34;
const MIN_WIDTH: i32 = 280;
const BUTTON: i32 = 26;
const PAD: i32 = 4;

/// What the box edits.
#[derive(Clone)]
enum Target {
    /// A new equation, at this caret.
    New(Option<editor::Selection>),
    Existing(NodeId),
}

/// The box's place: the equation's (or the caret's) rectangle in view pixels.
fn target_rect(state: &CanvasState, target: &Target) -> Option<render::Rect> {
    let scale = state.viewport.points_to_pixels();
    let (page, rect) = match target {
        Target::Existing(id) => { let (page, region) = objects_ui::region_for(state, id)?; (page, region.rect) }
        Target::New(_) => { let caret = state.overlay.caret.as_ref()?; (caret.page_index, caret.rect) }
    };
    let page_rect = state.viewport.page_rect_px(&state.vector_document, page)?;
    Some(render::Rect::new(page_rect.x + rect.x * scale, page_rect.y + rect.y * scale, rect.width * scale, rect.height * scale))
}

/// View pixels (physical) to screen coordinates (logical on GTK).
fn to_screen(canvas: &impl WxWidget, rect: render::Rect) -> Rect {
    let scale = crate::display_scale::window_scale(canvas).max(0.01);
    let origin = crate::gtk_native::to_screen(canvas, Point::new((rect.x / scale) as i32, (rect.y / scale) as i32));
    Rect::new(origin.x, origin.y, (rect.width / scale) as i32, (rect.height / scale) as i32)
}

/// Opens the box on the equation `id`, or for a new equation at the caret.
pub(super) fn show(ribbon: &Ribbon, id: Option<NodeId>) {
    let (target, source, place) = {
        let state = ribbon.state.borrow();
        let target = match id { Some(id) => Target::Existing(id), None => Target::New(state.editor.selection.clone()) };
        let source = match &target {
            Target::Existing(id) => match state.editor.equation_by_id(id) {
                Some(equation) => equation_input::format(&equation.root),
                None => return,
            },
            Target::New(_) => String::new(),
        };
        let place = target_rect(&state, &target).map(|rect| to_screen(&ribbon.ui.paper_canvas, rect));
        (target, source, place)
    };
    let frame = ribbon.ui.main_frame;
    let dip = |v: i32| frame.from_dip_int(v);
    let width = dip(MIN_WIDTH).max(place.map_or(0, |p| p.width + dip(2 * BUTTON + 4 * PAD)));
    let size = Size::new(width, dip(HEIGHT));
    let popup = dropdown::create(&frame, "Inline equation", size);
    // The controls sit on a white surface inside the dropdown's frame.
    let surface = wxdragon::widgets::panel::Panel::builder(&popup).with_pos(dropdown::origin(&popup)).with_size(size).build();
    surface.set_background_color(Colour::rgb(255, 255, 255));
    let inside = Point::new(0, 0);
    let field = TextCtrl::builder(&surface).with_value(&source).with_style(TextCtrlStyle::ProcessEnter)
        .with_pos(Point::new(inside.x + dip(PAD), inside.y + dip(PAD)))
        .with_size(Size::new(width - dip(2 * BUTTON + 4 * PAD), dip(HEIGHT - 2 * PAD))).build();
    let points = field.get_font().map_or(10, |font| font.get_point_size());
    if let Some(font) = wxdragon::font::Font::new_with_details(points, wxdragon::font::FontFamily::Teletype as i32,
        wxdragon::font::FontStyle::Normal as i32, wxdragon::font::FontWeight::Normal as i32, false, "") { field.set_font(&font); }
    field.set_tooltip(&crate::i18n::text(&("Typst math, such as x^2 + 1/2 or sqrt(a) · Enter to finish, Esc to cancel")));
    // ✓ and the full editor, painted at the right of the field.
    let tools = wxdragon::widgets::panel::Panel::builder(&surface)
        .with_pos(Point::new(width - dip(2 * BUTTON + 2 * PAD), dip((HEIGHT - BUTTON) / 2)))
        .with_size(Size::new(dip(2 * BUTTON + PAD), dip(BUTTON))).build();
    tools.set_background_style(wxdragon::window::BackgroundStyle::Paint);
    let hover = Rc::new(Cell::new(None::<usize>));
    { let hover = Rc::clone(&hover); tools.on_paint(move |_| paint_tools(tools, hover.get())); }
    { let hover = Rc::clone(&hover); tools.on_mouse_motion(move |event| {
        let WindowEventData::MouseMotion(m) = &event else { return; };
        let slot = m.event.get_position().and_then(|p| tool_at(tools, p));
        if hover.replace(slot) != slot {
            tools.set_tooltip(&crate::i18n::text(&(match slot { Some(0) => "Insert the equation (Enter)", Some(_) => "Open in the equation editor", None => "" })));
            tools.refresh(false, None);
        }
    }); }
    { let hover = Rc::clone(&hover); tools.on_mouse_leave(move |_| { if hover.take().is_some() { tools.refresh(false, None); } }); }

    let weak = ribbon.owner.clone();
    let finish = {
        let (weak, target) = (weak.clone(), target.clone());
        // Commits and closes; false (box kept open) when Typst cannot read it.
        move |open_editor: bool| -> bool {
            let Some(ribbon) = weak.upgrade() else { return true; };
            match commit(&ribbon, &target, &field.get_value()) {
                Ok(made) => {
                    dropdown::dismiss(popup);
                    if open_editor {
                        let id = made.or_else(|| match &target { Target::Existing(id) => Some(id.clone()), Target::New(_) => None });
                        if let Err(error) = ribbon.open_equation(id, EquationDisplay::Inline) { ribbon.state.borrow_mut().action_status = Some(error); }
                    } else { ribbon.focus_document(); }
                    true
                }
                Err(error) => {
                    field.set_tooltip(&crate::i18n::text(&(&format!("{error}\nEnter to finish, Esc to cancel"))));
                    ribbon.state.borrow_mut().action_status = Some(format!("Equation: {error}"));
                    field.set_focus();
                    false
                }
            }
        }
    };
    let finish = Rc::new(finish);
    { let finish = Rc::clone(&finish); field.on_text_enter(move |_| { finish(false); }); }
    { let finish = Rc::clone(&finish); tools.on_mouse_left_up(move |event| {
        let WindowEventData::MouseButton(m) = &event else { return; };
        if let Some(slot) = m.event.get_position().and_then(|p| tool_at(tools, p)) { finish(slot == 1); }
    }); }
    {
        let (finish, weak) = (Rc::clone(&finish), weak.clone());
        popup.bind_internal(EventType::CHAR_HOOK, move |event| {
            match event.get_key_code() {
                Some(WXK_ESCAPE) => {
                    dropdown::dismiss(popup);
                    if let Some(ribbon) = weak.upgrade() { ribbon.focus_document(); }
                }
                Some(WXK_TAB) => { finish(false); }
                _ => event.skip(true),
            }
        });
    }
    // A click outside renders the equation; if Typst cannot read the source,
    // the document keeps what it had.
    {
        let (weak, target) = (weak.clone(), target.clone());
        dropdown::on_leave(move || {
            let Some(ribbon) = weak.upgrade() else { return; };
            if let Err(error) = commit(&ribbon, &target, &field.get_value()) {
                ribbon.state.borrow_mut().action_status = Some(format!("Equation not changed: {error}"));
            }
        });
    }
    match place {
        Some(place) => {
            let at = Point::new(place.x - dip(PAD), place.y + place.height / 2 - dip(HEIGHT) / 2);
            dropdown::show_at_point(popup, at, &field);
        }
        None => dropdown::show(popup, &ribbon.ui.insert_inline_equation_button, &field),
    }
    field.set_insertion_point_end();
}

/// The tool under `p`: 0 is ✓, 1 the full editor.
fn tool_at(tools: wxdragon::widgets::panel::Panel, p: Point) -> Option<usize> {
    let (side, gap) = (tools.from_dip_int(BUTTON), tools.from_dip_int(PAD));
    if p.y < 0 || p.y >= side || p.x < 0 { return None; }
    if p.x < side { Some(0) } else if p.x >= side + gap && p.x < 2 * side + gap { Some(1) } else { None }
}

fn paint_tools(tools: wxdragon::widgets::panel::Panel, hover: Option<usize>) {
    use tiny_skia::{FillRule, Paint, PathBuilder, Stroke, Transform};
    let dc = wxdragon::dc::AutoBufferedPaintDC::new(&tools);
    let size = tools.get_client_size();
    let Some((mut pix, canvas)) = crate::crisp::Canvas::new(&tools, size.width, size.height) else { return; };
    pix.fill(tiny_skia::Color::WHITE);
    let d = |v: i32| tools.from_dip_int(v) as f32;
    let paint = |rgb: (u8, u8, u8)| { let mut p = Paint::default(); p.set_color_rgba8(rgb.0, rgb.1, rgb.2, 255); p.anti_alias = true; p };
    let slot_x = |slot: usize| slot as f32 * (d(BUTTON) + d(PAD));
    if let Some(slot) = hover {
        let mut p = PathBuilder::new();
        let (x, side, r) = (slot_x(slot), d(BUTTON), d(4));
        p.move_to(x + r, 0.0); p.line_to(x + side - r, 0.0); p.quad_to(x + side, 0.0, x + side, r);
        p.line_to(x + side, side - r); p.quad_to(x + side, side, x + side - r, side);
        p.line_to(x + r, side); p.quad_to(x, side, x, side - r); p.line_to(x, r); p.quad_to(x, 0.0, x + r, 0.0); p.close();
        if let Some(path) = p.finish() { pix.fill_path(&path, &paint((232, 239, 253)), FillRule::Winding, crate::crisp::transform(), None); }
    }
    // ✓ in the accent colour.
    let mut check = PathBuilder::new();
    let (x, s) = (slot_x(0), d(BUTTON));
    check.move_to(x + 0.27 * s, 0.52 * s); check.line_to(x + 0.43 * s, 0.68 * s); check.line_to(x + 0.74 * s, 0.33 * s);
    if let Some(path) = check.finish() {
        let stroke = Stroke { width: d(2).max(1.5), line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
        pix.stroke_path(&path, &paint((43, 111, 232)), &stroke, crate::crisp::transform(), None);
    }
    // The equation icon, rasterized at the screen's resolution.
    let k = canvas.scale();
    let side = (d(18) * k).round().max(1.0) as u32;
    if let Some(rgba) = art::raster("equation", side, side, false) {
        if let Some(mut icon) = tiny_skia::Pixmap::new(side, side) {
            for (t, s) in icon.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(rgba.as_chunks::<4>().0) {
                let a = u16::from(s[3]);
                let pm = |c: u8| ((u16::from(c) * a + 127) / 255) as u8;
                *t = [pm(s[0]), pm(s[1]), pm(s[2]), s[3]];
            }
            let left = ((slot_x(1) + (d(BUTTON) - d(18)) * 0.5) * k).round() as i32;
            let top = ((d(BUTTON) - d(18)) * 0.5 * k).round() as i32;
            pix.draw_pixmap(left, top, icon.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
        }
    }
    canvas.present(&dc, &pix, 0, 0);
}

/// Puts the box's source into the document: the new equation's id, if one
/// was made.
fn commit(ribbon: &Ribbon, target: &Target, source: &str) -> Result<Option<NodeId>, String> {
    let source = source.trim();
    if source.is_empty() {
        if let Target::Existing(id) = target { ribbon.edit(|editor| editor.delete_equation(id))?; }
        return Ok(None);
    }
    let root = equation_input::parse(source)?;
    let mut equation = Equation::inline(root);
    equation.display = EquationDisplay::Inline;
    match target {
        Target::Existing(id) => {
            let unchanged = ribbon.state.borrow().editor.equation_by_id(id).is_some_and(|e| e.root == equation.root);
            if !unchanged { ribbon.edit(|editor| editor.replace_equation(id, equation))?; }
            Ok(None)
        }
        Target::New(selection) => {
            let id = equation.id.clone();
            let selection = selection.clone();
            ribbon.edit(move |editor| {
                if selection.is_some() { editor.selection = selection; }
                editor.insert_inline_equation(equation)
            })?;
            Ok(Some(id))
        }
    }
}

