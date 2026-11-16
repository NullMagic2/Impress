//! Compact font picker with a bounded, owner-painted font-name list.
use super::*;
use wxdragon::{
    BrushStyle, DeviceContext, PenStyle,
    dc::AutoBufferedPaintDC,
    event::{scroll_events::ScrollEvents, window_events::WindowEvents},
    font::{Font, FontFamily, FontStyle, FontWeight},
    geometry::{Point, Size},
    widgets::textctrl::{TextCtrl, TextCtrlStyle},
    widgets::{
        combobox::ComboBox,
        panel::Panel,
        scrollbar::{ScrollBar, ScrollBarStyle},
    },
    window::BackgroundStyle,
};

const FIELD_WIDTH: i32 = 150;
const POPUP_WIDTH: i32 = 270;
const ROW_HEIGHT: i32 = 28;
const MAX_ROWS: usize = 11;
const PAPER: Colour = Colour::rgb(255, 255, 255);
const INK: Colour = Colour::rgb(35, 50, 74);
const EDGE: Colour = Colour::rgb(175, 186, 200);
const HOT: Colour = Colour::rgb(229, 237, 252);
fn rounded_surface(panel: Panel, background: Colour, radius: i32) {
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_paint(move |_| {
        let dc = AutoBufferedPaintDC::new(&panel);
        let size = panel.get_client_size();
        dc.set_background(background);
        dc.clear();
        dc.set_brush(PAPER, BrushStyle::Solid);
        dc.set_pen(EDGE, panel.from_dip_int(1).max(1), PenStyle::Solid);
        dc.draw_rounded_rectangle(
            0,
            0,
            size.width - 1,
            size.height - 1,
            panel.from_dip_int(radius) as f64,
        );
    });
}

thread_local! { static PENDING_FOCUS:RefCell<Option<std::rc::Weak<Ribbon>>>=const{RefCell::new(None)}; }
pub(super) fn poll() {
    let pending = PENDING_FOCUS.with(|slot| slot.borrow_mut().take());
    if let Some(ribbon) = pending.and_then(|owner| owner.upgrade()) {
        ribbon.focus_document();
    }
}
fn apply_family(ribbon: &Ribbon, input: TextCtrl, name: String) {
    input.change_value(&name);
    ribbon.commit_field_value(name, Field::Family);
    // wx destroys the popup's focused entry after this mouse/key handler.
    // Restore the canvas focus after that teardown so typing reaches the page.
    PENDING_FOCUS.with(|slot| *slot.borrow_mut() = Some(ribbon.owner.clone()));
}

pub(super) fn install(ribbon: &Rc<Ribbon>) {
    let panel = ribbon.ui.font_family_combo;
    let input = ribbon.ui.font_family_input;
    panel.set_background_style(BackgroundStyle::Paint);
    panel.set_tooltip(&crate::i18n::text(&("")));
    input.set_tooltip(&crate::i18n::text(&("")));
    crate::gtk_native::style_font_input(&input);
    input.on_set_focus(move |event| {
        let end = input.get_value().chars().count() as i64;
        input.set_selection(end, end);
        event.skip(true);
    });
    panel.on_paint(move |_| paint_field(panel));
    panel.on_size(move |event| {
        layout_input(panel, input);
        panel.refresh(false, None);
        event.skip(true);
    });
    layout_input(panel, input);
    let size_combo = ribbon.ui.font_size_combo;
    size_combo.on_size(move |event| {
        align_allocated_height(panel, size_combo);
        event.skip(true);
    });
    let weak = Rc::downgrade(ribbon);
    // Opening during mouse-down lets the panel's native focus handling steal
    // activation straight back from the new dropdown. Finish the click first.
    panel.on_mouse_left_up(move |_| {
        if let Some(r) = weak.upgrade() {
            show_picker(&r, false);
        }
    });
    let weak = Rc::downgrade(ribbon);
    input.on_text_updated(move |_| {
        if input.has_focus() {
            if let Some(r) = weak.upgrade() {
                show_picker(&r, true);
            }
        }
    });
    let weak = Rc::downgrade(ribbon);
    input.on_enter_pressed(move |_| {
        if let Some(r) = weak.upgrade() {
            let family = input.get_value();
            if !family.trim().is_empty() {
                apply_family(&r, input, family);
            }
        }
    });
    let weak = Rc::downgrade(ribbon);
    input.on_key_down(move |event| {
        if let WindowEventData::Keyboard(key) = &event {
            if key.get_key_code() == Some(wxdragon::keycode::WXK_F4)
                || (key.alt_down() && key.get_key_code() == Some(wxdragon::keycode::WXK_DOWN))
            {
                if let Some(r) = weak.upgrade() {
                    show_picker(&r, false);
                }
                return;
            }
        }
        event.skip(true);
    });
}

fn layout_input(panel: Panel, input: wxdragon::widgets::textctrl::TextCtrl) {
    let size = panel.get_client_size();
    let dip = |v| panel.from_dip_int(v);
    let height = (input.get_text_extent("Ag").height + dip(2)).min((size.height - dip(2)).max(1));
    input.set_size_with_pos(
        dip(7),
        (size.height - height).max(0) / 2,
        (size.width - dip(29)).max(dip(20)),
        height,
    );
}

fn paint_field(panel: Panel) {
    let dc = AutoBufferedPaintDC::new(&panel);
    let size = panel.get_client_size();
    let dip = |v| panel.from_dip_int(v);
    dc.set_background(
        panel
            .get_parent()
            .map_or(PAPER, |parent| parent.get_background_color()),
    );
    dc.clear();
    dc.set_brush(Colour::rgb(255, 255, 255), BrushStyle::Solid);
    dc.set_pen(Colour::rgb(175, 186, 200), dip(1).max(1), PenStyle::Solid);
    dc.draw_rounded_rectangle(
        0,
        0,
        size.width - 1,
        size.height - 1,
        panel.from_dip_int(6) as f64,
    );
    let x = size.width - dip(13);
    let y = size.height / 2;
    dc.set_pen(Colour::rgb(68, 81, 102), dip(1).max(1), PenStyle::Solid);
    dc.draw_line(x - dip(4), y - dip(2), x, y + dip(2));
    dc.draw_line(x, y + dip(2), x + dip(4), y - dip(2));
}

pub(super) fn align_with_size(panel: Panel, size_combo: ComboBox) {
    // Native editable combo boxes may clamp their height. Match the resulting
    // control, rather than forcing a larger minimum that only the panel obeys.
    // The width is the one generated.rs gave the size box (room for "10.5"
    // beside GTK's dropdown button).
    let width = size_combo.get_min_size().width;
    size_combo.set_min_size(Size::new(width, -1));
    let height = size_combo.get_best_size().height.max(1);
    size_combo.set_min_size(Size::new(width, height));
    panel.set_min_size(Size::new(panel.from_dip_int(FIELD_WIDTH), height));
    panel.set_size(Size::new(
        panel.get_size().width.max(panel.from_dip_int(FIELD_WIDTH)),
        height,
    ));
}

// GTK may allocate a native combo slightly taller than its reported best size.
// Follow that final allocation so both controls and their button rows line up.
fn align_allocated_height(panel: Panel, size_combo: ComboBox) {
    let height = size_combo.get_size().height.max(1);
    let minimum = Size::new(panel.from_dip_int(FIELD_WIDTH), height);
    if panel.get_min_size() != minimum {
        panel.set_min_size(minimum);
        if let Some(parent) = panel.get_parent() { parent.layout(); }
    }
    if panel.get_size().height != height {
        panel.set_size(Size::new(panel.get_size().width, height));
    }
}

fn layout_search(header: Panel, search: TextCtrl) {
    let size = header.get_client_size();
    let x = header.from_dip_int(8);
    let padding = header.from_dip_int(4);
    let height = (search.get_text_extent("Ag").height + header.from_dip_int(2))
        .min((size.height - 2 * padding).max(1));
    search.set_size_with_pos(x, (size.height - height) / 2,
        (size.width - 2 * x).max(1), height);
}

#[derive(Default)]
struct Picker {
    families: Vec<String>,
    matches: Vec<usize>,
    first: usize,
    selected: Option<usize>,
    hovered: Option<usize>,
}
impl Picker {
    fn filter(&mut self, query: &str) {
        self.matches = matching_indices(&self.families, query);
        self.first = 0;
        self.selected = None;
        self.hovered = None;
    }
    fn name(&self) -> Option<&String> {
        self.selected
            .and_then(|index| self.matches.get(index))
            .and_then(|index| self.families.get(*index))
    }
    fn scroll_limit(&self, visible: usize) -> usize {
        self.matches.len().saturating_sub(visible)
    }
    fn navigate(&mut self, key: i32, visible: usize) -> bool {
        use wxdragon::keycode::*;
        if self.matches.is_empty() {
            return matches!(key, WXK_UP | WXK_DOWN | WXK_PAGEUP | WXK_PAGEDOWN);
        }
        let index = self.selected.unwrap_or(self.first);
        let next = if self.selected.is_none()
            && matches!(key, WXK_UP | WXK_DOWN | WXK_PAGEUP | WXK_PAGEDOWN)
        {
            if key == WXK_UP {
                self.matches.len() - 1
            } else {
                self.first.min(self.matches.len() - 1)
            }
        } else {
            match key {
                WXK_UP => index.saturating_sub(1),
                WXK_DOWN => (index + 1).min(self.matches.len() - 1),
                WXK_PAGEUP => index.saturating_sub(visible),
                WXK_PAGEDOWN => (index + visible).min(self.matches.len() - 1),
                _ => return false,
            }
        };
        self.hovered = None;
        self.selected = Some(next);
        if next < self.first {
            self.first = next;
        }
        if next >= self.first + visible {
            self.first = next + 1 - visible;
        }
        true
    }
}
/// Prefix matches come first; every typed word must occur in the family name.
fn matching_indices(families: &[String], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let words = query.split_whitespace().collect::<Vec<_>>();
    let mut matches = families
        .iter()
        .enumerate()
        .filter(|(_, name)| {
            let name = name.to_lowercase();
            words.iter().all(|word| name.contains(word))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    matches.sort_by_key(|index| !families[*index].to_lowercase().starts_with(&query));
    matches
}
fn update_bar(bar: ScrollBar, picker: &Picker, visible: usize) {
    let count = picker.matches.len();
    bar.set_scrollbar(
        picker.first as i32,
        visible.min(count).max(1) as i32,
        count.max(1) as i32,
        visible as i32,
        true,
    );
    bar.show(count > visible);
}
fn show_picker(ribbon: &Ribbon, search_typed: bool) {
    let mut families = {
        let state = ribbon.state.borrow();
        let mut families =
            typography::FontCatalog::for_resources(&state.editor.document.resources).family_names();
        for resource in state.editor.document.resources.values() {
            for face in &resource.font_faces {
                if !families
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&face.family))
                {
                    families.push(face.family.clone());
                }
            }
        }
        families
    };
    families.sort_by_key(|name| name.to_lowercase());
    if families.is_empty() {
        return;
    }
    let field = ribbon.ui.font_family_combo;
    let input = ribbon.ui.font_family_input;
    let typed = input.get_value();
    let visible = families.len().min(MAX_ROWS);
    let mut state = Picker {
        families,
        ..Default::default()
    };
    state.filter(if search_typed { &typed } else { "" });
    if !search_typed {
        let active = state
            .matches
            .iter()
            .position(|index| state.families[*index].eq_ignore_ascii_case(typed.trim()))
            .unwrap_or(0);
        state.first = active.saturating_sub(3).min(state.scroll_limit(visible));
    }
    let end = typed.chars().count() as i64;
    input.set_selection(end, end);
    let picker = Rc::new(RefCell::new(state));
    let width = field.from_dip_int(POPUP_WIDTH);
    let search_height = field.from_dip_int(36);
    let height = field.from_dip_int(ROW_HEIGHT * visible as i32 + 4);
    let scroll_width = field.from_dip_int(16);
    let popup = dropdown::create(
        &ribbon.ui.main_frame,
        "Fonts",
        Size::new(width, height + search_height + field.from_dip_int(12)),
    );
    let inside = dropdown::origin(&popup);
    rounded_surface(
        popup,
        popup
            .get_parent()
            .map_or(PAPER, |parent| parent.get_background_color()),
        6,
    );
    let inset = field.from_dip_int(6);
    let header = Panel::builder(&popup)
        .with_pos(Point::new(inside.x + inset, inside.y + inset))
        .with_size(Size::new(
            width - 2 * inset,
            search_height - field.from_dip_int(4),
        ))
        .build();
    rounded_surface(header, PAPER, 6);
    let search = TextCtrl::builder(&header)
        .with_style(TextCtrlStyle::from_bits_retain(
            TextCtrlStyle::ProcessEnter.bits()
                | wxdragon::widgets::panel::PanelStyle::BorderNone.bits(),
        ))
        .with_value(if search_typed { &typed } else { "" })
        .with_pos(Point::new(field.from_dip_int(8), field.from_dip_int(4)))
        .with_size(Size::new(
            width - 2 * inset - field.from_dip_int(16),
            search_height - field.from_dip_int(8),
        ))
        .build();
    search.set_background_color(PAPER);
    search.set_foreground_color(INK);
    crate::gtk_native::style_font_input(&search);
    // Keep the native entry entirely inside the painted rounded frame. Its old
    // height extended to the header's bottom edge and erased the bottom border.
    layout_search(header, search);
    header.on_size(move |event| {
        layout_search(header, search);
        header.refresh(false, None);
        event.skip(true);
    });
    let surface = Panel::builder(&popup)
        .with_pos(Point::new(
            inside.x + inset,
            inside.y + search_height + inset,
        ))
        .with_size(Size::new(width - scroll_width - 2 * inset, height))
        .build();
    surface.set_background_style(BackgroundStyle::Paint);
    let bar = ScrollBar::builder(&popup)
        .with_style(ScrollBarStyle::Vertical)
        .build();
    bar.set_size_with_pos(
        inside.x + width - scroll_width - inset,
        inside.y + search_height + inset,
        scroll_width,
        height,
    );
    update_bar(bar, &picker.borrow(), visible);
    {
        let picker = picker.clone();
        surface.on_paint(move |_| paint_list(surface, &picker.borrow(), visible));
    }
    {
        let picker = picker.clone();
        search.on_text_updated(move |_| {
            picker.borrow_mut().filter(&search.get_value());
            update_bar(bar, &picker.borrow(), visible);
            surface.refresh(false, None);
        });
    }
    {
        let picker = picker.clone();
        let weak = ribbon.owner.clone();
        surface.on_mouse_left_down(move |event| {
            if let Some(point) = mouse_position(&event) {
                let row = point.y / surface.from_dip_int(ROW_HEIGHT);
                let name = {
                    let state = picker.borrow();
                    if row < 0 || (row as usize) >= visible {
                        None
                    } else {
                        state
                            .matches
                            .get(state.first + row as usize)
                            .map(|index| state.families[*index].clone())
                    }
                };
                if let Some(name) = name {
                    dropdown::dismiss(popup);
                    if let Some(ribbon) = weak.upgrade() {
                        apply_family(&ribbon, input, name);
                    }
                }
            }
        });
    }
    {
        let picker = picker.clone();
        surface.on_mouse_motion(move |event| {
            let mut state = picker.borrow_mut();
            let next = mouse_position(&event)
                .and_then(|p| {
                    let row = p.y / surface.from_dip_int(ROW_HEIGHT);
                    (row >= 0 && (row as usize) < visible).then_some(state.first + row as usize)
                })
                .filter(|index| *index < state.matches.len());
            if state.hovered != next {
                state.hovered = next;
                surface.refresh(false, None);
            }
        });
    }
    {
        let picker = picker.clone();
        surface.on_mouse_wheel(move |event| {
            let (rotation, delta) = match &event {
                WindowEventData::MouseButton(m) => {
                    (m.event.get_wheel_rotation(), m.event.get_wheel_delta())
                }
                WindowEventData::MouseMotion(m) => {
                    (m.event.get_wheel_rotation(), m.event.get_wheel_delta())
                }
                _ => (0, 0),
            };
            if rotation != 0 {
                let mut state = picker.borrow_mut();
                let steps = (rotation.abs() / delta.max(1)).max(1) as usize * 3;
                state.first = if rotation > 0 {
                    state.first.saturating_sub(steps)
                } else {
                    (state.first + steps).min(state.scroll_limit(visible))
                };
                update_bar(bar, &state, visible);
                surface.refresh(false, None);
            }
        });
    }
    {
        let picker = picker.clone();
        bar.on_thumb_track(move |event| {
            if let Some(position) = event.get_position() {
                let mut state = picker.borrow_mut();
                state.first = (position.max(0) as usize).min(state.scroll_limit(visible));
                surface.refresh(false, None);
            }
        });
    }
    for (event_type, step) in [
        (wxdragon::event::ScrollEventType::LineUp, -1),
        (wxdragon::event::ScrollEventType::LineDown, 1),
        (wxdragon::event::ScrollEventType::PageUp, -(visible as i32)),
        (wxdragon::event::ScrollEventType::PageDown, visible as i32),
    ] {
        let picker = picker.clone();
        ScrollEvents::bind_category_event(&bar, event_type, move |_| {
            let mut state = picker.borrow_mut();
            state.first =
                (state.first as i32 + step).clamp(0, state.scroll_limit(visible) as i32) as usize;
            update_bar(bar, &state, visible);
            surface.refresh(false, None);
        });
    }
    {
        let picker = picker.clone();
        let weak = ribbon.owner.clone();
        dropdown::bind_keys(popup, input, move |key| {
            if key == wxdragon::keycode::WXK_RETURN {
                let name = {
                    let state = picker.borrow();
                    state.name().cloned().or_else(|| {
                        state
                            .families
                            .iter()
                            .find(|family| family.eq_ignore_ascii_case(search.get_value().trim()))
                            .cloned()
                    })
                };
                if let Some(name) = name {
                    dropdown::dismiss(popup);
                    if let Some(ribbon) = weak.upgrade() {
                        apply_family(&ribbon, input, name);
                    }
                }
                return true;
            }
            let mut state = picker.borrow_mut();
            if state.navigate(key, visible) {
                update_bar(bar, &state, visible);
                surface.refresh(false, None);
                return true;
            }
            false
        });
    }
    dropdown::show(popup, &field, &search);
    if search_typed {
        search.set_selection(typed.chars().count() as i64, typed.chars().count() as i64);
    }
}

fn mouse_position(event: &WindowEventData) -> Option<Point> {
    match event {
        WindowEventData::MouseButton(m) => m.event.get_position(),
        WindowEventData::MouseMotion(m) => m.event.get_position(),
        _ => None,
    }
}

fn paint_list(surface: Panel, picker: &Picker, visible: usize) {
    let dc = AutoBufferedPaintDC::new(&surface);
    let size = surface.get_client_size();
    let row_height = surface.from_dip_int(ROW_HEIGHT);
    let dip = |v| surface.from_dip_int(v);
    dc.set_background(Colour::rgb(255, 255, 255));
    dc.clear();
    if picker.matches.is_empty() {
        dc.set_text_foreground(Colour::rgb(107, 119, 138));
        dc.draw_text(&crate::i18n::text("No fonts found"), dip(9), dip(7));
    }
    for row in 0..visible {
        let index = picker.first + row;
        let Some(name) = picker
            .matches
            .get(index)
            .and_then(|index| picker.families.get(*index))
        else {
            break;
        };
        let y = row as i32 * row_height;
        if picker.selected == Some(index) || picker.hovered == Some(index) {
            dc.set_brush(Colour::rgb(229, 237, 252), BrushStyle::Solid);
            dc.set_pen(Colour::rgb(181, 200, 238), dip(1).max(1), PenStyle::Solid);
            dc.draw_rectangle(0, y, size.width, row_height);
        }
        if let Some(font) = Font::new_with_details(
            11,
            FontFamily::Default as i32,
            FontStyle::Normal as i32,
            FontWeight::Normal as i32,
            false,
            name,
        ) {
            dc.set_font(&font);
        }
        dc.set_text_foreground(Colour::rgb(35, 50, 74));
        let mut label = name.clone();
        while dc.get_text_extent(&label).0 > size.width - dip(18) && label.chars().count() > 2 {
            label.pop();
        }
        if label != *name {
            label.pop();
            label.push('…');
        }
        let text_height = dc.get_text_extent(&label).1;
        dc.draw_text(&label, dip(9), y + (row_height - text_height) / 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typing_finds_prefix_and_substring_without_changing_fonts() {
        let families = vec![
            "Aegean Legacy".into(),
            "Neo Aegean".into(),
            "Neo Aegean Compact".into(),
            "Neo Aegean Compact Light".into(),
            "Noto Sans".into(),
        ];
        assert_eq!(matching_indices(&families, "NEO"), vec![1, 2, 3]);
        assert_eq!(matching_indices(&families, "aegean"), vec![0, 1, 2, 3]);
        assert_eq!(matching_indices(&families, "compact light"), vec![3]);
        assert!(matching_indices(&families, "unavailable").is_empty());
        let mut picker = Picker {
            families,
            ..Default::default()
        };
        picker.filter("neo");
        assert!(picker.name().is_none());
        assert!(picker.navigate(wxdragon::keycode::WXK_DOWN, 2));
        assert_eq!(picker.name().map(String::as_str), Some("Neo Aegean"));
        assert!(picker.navigate(wxdragon::keycode::WXK_DOWN, 2));
        assert_eq!(
            picker.name().map(String::as_str),
            Some("Neo Aegean Compact")
        );
        picker.filter("unavailable");
        assert!(picker.name().is_none());
        assert_eq!(picker.scroll_limit(11), 0);
    }
}
