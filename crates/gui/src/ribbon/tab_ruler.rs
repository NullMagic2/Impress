//! Page-aligned tab ruler. Mouse gestures commit through the semantic editor.
use super::ruler_geometry::{PageScale, RulerPages};
use super::ruler_indents::{self, Handle, Indents};
use super::*;
use model::{TabAlignment, TabLeader, TabStop};
use wxdragon::{
    BrushStyle, DeviceContext, PenStyle, dc::AutoBufferedPaintDC, widgets::panel::Panel,
    window::BackgroundStyle,
};

const SELECTOR_DIP: i32 = 32;
const MARKER_Y_DIP: i32 = 25;
/// Align the measurement scale with the lower edge of the ruler strip.
const SCALE_BOTTOM_INSET_DIP: i32 = 4;
/// Keep number labels just above the scale baseline.
const LABEL_BASELINE_GAP_DIP: i32 = 2;
const HIT_DIP: i32 = 7;
const SNAP_MM: f32 = 1.0;
const FINE_SNAP_MM: f32 = 0.1;
const MM_TO_PT: f32 = 72.0 / 25.4;
const DOUBLE_CLICK_MS: u128 = 400;
/// Keep centimetre labels legible when zoomed out.
const MIN_LABEL_SPACING_DIP: i32 = 36;
/// Avoid drawing subpixel ruler ticks at low zoom.
const MIN_TICK_SPACING_DIP: i32 = 3;
const BACKGROUND: Colour = Colour::rgb(232, 236, 242);
const PAPER: Colour = Colour::rgb(255, 255, 255);
const INK: Colour = Colour::rgb(57, 70, 88);
const ACCENT: Colour = Colour::rgb(53, 106, 193);
const TYPES: [TabAlignment; 5] = [
    TabAlignment::Left,
    TabAlignment::Center,
    TabAlignment::Right,
    TabAlignment::Decimal,
    TabAlignment::Bar,
];

#[derive(Clone, PartialEq)]
struct Geometry {
    start: f32,
    width: f32,
    pixels_per_mm: f32,
    stops: Option<Vec<TabStop>>,
    default_mm: f32,
    indents: Indents,
    page: PageScale,
    vertical: Vec<PageScale>,
    editable: bool,
}
struct FormatCache {
    revision: u64,
    selection: Option<Selection>,
    tabs: Option<Vec<TabStop>>,
    default_mm: f32,
    indents: Indents,
}

struct Drag {
    geometry: Geometry,
    before: Option<f32>,
    stop: TabStop,
    remove: bool,
    revision: u64,
    selection: Option<Selection>,
}

struct IndentDrag {
    geometry: Geometry,
    handle: Handle,
    original: Indents,
    current: Indents,
    revision: u64,
    selection: Option<Selection>,
}

pub(super) struct TabRuler {
    ribbon: std::rc::Weak<Ribbon>,
    panel: Panel,
    vertical: Panel,
    format_cache: RefCell<Option<FormatCache>>,
    kind: Cell<usize>,
    geometry: RefCell<Option<Geometry>>,
    drag: RefCell<Option<Drag>>,
    indent_drag: RefCell<Option<IndentDrag>>,
    last_click: Cell<Option<(Instant, i32)>>,
}

pub(super) fn install(ribbon: &Rc<Ribbon>) -> Rc<TabRuler> {
    let panel = ribbon.ui.tab_ruler;
    let ruler = Rc::new(TabRuler {
        ribbon: Rc::downgrade(ribbon),
        panel,
        vertical: ribbon.ui.vertical_ruler,
        format_cache: RefCell::new(None),
        kind: Cell::new(0),
        geometry: RefCell::new(None),
        drag: RefCell::new(None),
        indent_drag: RefCell::new(None),
        last_click: Cell::new(None),
    });
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_erase_background(|_| {});
    let weak = Rc::downgrade(&ruler);
    panel.on_paint(move |_| {
        if let Some(ruler) = weak.upgrade() {
            ruler.paint();
        }
    });
    let weak = Rc::downgrade(&ruler);
    let vertical = ruler.vertical;
    vertical.set_background_style(BackgroundStyle::Paint);
    vertical.on_erase_background(|_| {});
    let vertical_weak = Rc::downgrade(&ruler);
    vertical.on_paint(move |_| {
        if let Some(ruler) = vertical_weak.upgrade() {
            ruler.paint_vertical();
        }
    });
    panel.on_mouse_left_down(move |event| {
        if let (Some(ruler), WindowEventData::MouseButton(mouse)) = (weak.upgrade(), &event) {
            if let Some(point) = mouse.event.get_position() {
                ruler.down(point.x, point.y, mouse.event.event.alt_down());
            }
        }
    });
    let weak = Rc::downgrade(&ruler);
    panel.on_mouse_motion(move |event| {
        if let (Some(ruler), WindowEventData::MouseMotion(mouse)) = (weak.upgrade(), &event) {
            if let Some(point) = mouse.event.get_position() {
                ruler.motion(point.x, point.y, mouse.event.event.alt_down());
            }
        }
    });
    let weak = Rc::downgrade(&ruler);
    panel.on_mouse_left_up(move |event| {
        if let (Some(ruler), WindowEventData::MouseButton(mouse)) = (weak.upgrade(), &event) {
            if let Some(point) = mouse.event.get_position() {
                ruler.motion(point.x, point.y, mouse.event.event.alt_down());
            }
            ruler.finish();
        }
    });
    let weak = Rc::downgrade(&ruler);
    panel.on_mouse_right_up(move |_| {
        if let Some(ruler) = weak.upgrade() {
            ruler.open_dialog();
        }
    });
    let weak = Rc::downgrade(&ruler);
    panel.on_key_down(move |event| {
        let Some(ruler) = weak.upgrade() else {
            return;
        };
        if let WindowEventData::Keyboard(key) = &event {
            match key.get_key_code() {
                Some(WXK_ESCAPE) => {
                    ruler.cancel();
                    if let Some(ribbon) = ruler.ribbon.upgrade() {
                        ribbon.focus_document();
                    }
                    event.skip(false);
                    return;
                }
                Some(WXK_RETURN) | Some(WXK_NUMPAD_ENTER) => {
                    ruler.cancel();
                    ruler.open_dialog();
                    event.skip(false);
                    return;
                }
                _ => {}
            }
        }
        event.skip(true);
    });
    let weak = Rc::downgrade(ribbon);
    let check = ribbon.ui.show_ruler_check;
    check.on_toggled(move |_| {
        if let Some(ribbon) = weak.upgrade() {
            ribbon.ui.tab_ruler.show(check.is_checked());
            ribbon.ui.vertical_ruler.show(check.is_checked());
            ribbon.ui.canvas_container.layout();
        }
    });
    ruler.panel.set_tooltip(&crate::i18n::text(&format!(
        "{} tab. Click to change type; click the ruler to place it.",
        crate::i18n::text(alignment_name(TYPES[0]))
    )));
    ruler.refresh();
    ruler
}

impl TabRuler {
    pub(super) fn refresh(&self) {
        if (self.drag.borrow().is_some() || self.indent_drag.borrow().is_some())
            && !self.panel.has_capture()
        {
            self.cancel();
        }
        let next = self.read_geometry();
        if *self.geometry.borrow() != next {
            self.geometry.replace(next);
            self.panel.refresh(false, None);
            self.vertical.refresh(false, None);
        }
    }

    fn read_geometry(&self) -> Option<Geometry> {
        let ribbon = self.ribbon.upgrade()?;
        let state = ribbon.state.borrow();
        let origin = wxdragon::geometry::Point::new(0, 0);
        let canvas = crate::gtk_native::to_screen(&ribbon.ui.paper_canvas, origin);
        let horizontal = crate::gtk_native::to_screen(&self.panel, origin);
        let vertical = crate::gtk_native::to_screen(&self.vertical, origin);
        let screen = crate::display_scale::window_scale(&ribbon.ui.paper_canvas);
        let pages = RulerPages::new(
            &state.vector_document,
            &state.viewport,
            screen,
            render::Point::new(
                (canvas.x - horizontal.x) as f32,
                (canvas.y - vertical.y) as f32,
            ),
        )?;
        let page_scale = pages.primary().clone();
        let page = &state.vector_document.pages[page_scale.index];
        // A hidden/offscreen caret must not move the scale to another sheet or
        // expose handles that would edit a paragraph outside the visible page.
        let caret = state.overlay.caret.clone().or_else(|| {
            let focus = state.editor.focus_position()?;
            let selection = render::SourceSelection::collapsed(render::SourcePosition {
                node_id: focus.node_id.0,
                byte_offset: focus.byte_offset,
            });
            render::overlay_for_selection_with_caret_line(
                &state.vector_document,
                Some(&selection),
                1.0,
                state.caret_visual_line,
            )
            .caret
        });
        let editable = caret
            .as_ref()
            .is_some_and(|caret| caret.page_index == page_scale.index);
        let scale = pages.pixels_per_point;
        let mut start = page_scale.margins.x;
        let mut width = page_scale.margins.width;
        if editable {
            if let Some(context) = state.editor.table_cell_context() {
                if let Some(cell) = page
                    .object_regions
                    .iter()
                    .find(|region| region.node_id == context.cell_id.0)
                {
                    let padding =
                        model::TABLE_TEXT_PADDING_PT.min(((cell.rect.width - 1.0) * 0.5).max(0.0));
                    start = page_scale.paper.x + (cell.rect.x + padding) * scale;
                    width = (cell.rect.width - 2.0 * padding).max(1.0) * scale;
                }
            } else if !state.editor.is_editing_header_footer() {
                if let Some(section) = state.editor.section_properties_at_caret() {
                    let relative = caret.as_ref()?.rect.x / MM_TO_PT - section.margin_left_mm;
                    if let Some(band) = section.column_bands().into_iter().find(|band| {
                        relative >= band.left_mm - 0.1
                            && relative <= band.left_mm + band.width_mm + 0.1
                    }) {
                        start += band.left_mm * MM_TO_PT * scale;
                        width = band.width_mm * MM_TO_PT * scale;
                    }
                }
            }
        }
        let mut format_cache = self.format_cache.borrow_mut();
        if format_cache.as_ref().is_none_or(|cache| {
            cache.revision != state.editor.revision || cache.selection != state.editor.selection
        }) {
            *format_cache = Some(FormatCache {
                revision: state.editor.revision,
                selection: state.editor.selection.clone(),
                tabs: state.editor.formatting_summary().tabs,
                indents: Indents::from_properties(&state.editor.paragraph_properties_at_caret()?),
                default_mm: state
                    .editor
                    .document
                    .styles
                    .resolve_paragraph(Some(&model::StyleId::from(model::STYLE_NORMAL)))
                    .default_tab_mm
                    .unwrap_or(model::DEFAULT_TAB_INTERVAL_MM),
            });
        }
        let cache = format_cache.as_ref()?;
        Some(Geometry {
            start,
            width,
            pixels_per_mm: MM_TO_PT * scale,
            stops: cache.tabs.clone(),
            default_mm: cache.default_mm,
            indents: cache.indents,
            page: page_scale,
            vertical: pages.pages,
            editable,
        })
    }

    fn open_dialog(&self) {
        if let Some(ribbon) = self.ribbon.upgrade() {
            if let Err(error) = ribbon.tabs_dialog() {
                ribbon.state.borrow_mut().action_status = Some(error);
            }
        }
    }

    fn down(&self, x: i32, y: i32, fine: bool) {
        if x < self.panel.from_dip_int(SELECTOR_DIP) {
            self.last_click.set(None);
            self.kind.set((self.kind.get() + 1) % TYPES.len());
            self.panel.set_tooltip(&crate::i18n::text(
                &(&format!(
                    "{} tab. Click to change type; click the ruler to place it.",
                    crate::i18n::text(alignment_name(TYPES[self.kind.get()]))
                )),
            ));
            self.panel.refresh(false, None);
            return;
        }
        if let Some(geometry) = self.geometry.borrow().clone().filter(|g| g.editable) {
            let width = geometry.width / geometry.pixels_per_mm;
            if let Some(handle) = ruler_indents::hit(
                &self.panel,
                geometry.indents,
                geometry.start,
                geometry.pixels_per_mm,
                width,
                x,
                y,
            ) {
                if let Some(ribbon) = self.ribbon.upgrade() {
                    let state = ribbon.state.borrow();
                    let original = geometry.indents;
                    self.indent_drag.replace(Some(IndentDrag {
                        geometry,
                        handle,
                        original,
                        current: original,
                        revision: state.editor.revision,
                        selection: state.editor.selection.clone(),
                    }));
                    drop(state);
                    self.last_click.set(None);
                    self.panel.set_focus();
                    self.panel.capture_mouse();
                    self.panel.refresh(false, None);
                }
                return;
            }
        }
        let now = Instant::now();
        if self
            .last_click
            .replace(Some((now, x)))
            .is_some_and(|(previous, previous_x)| {
                now.duration_since(previous).as_millis() < DOUBLE_CLICK_MS
                    && (previous_x - x).abs() < self.panel.from_dip_int(HIT_DIP)
            })
        {
            self.last_click.set(None);
            self.open_dialog();
            return;
        }
        let Some(geometry) = self.geometry.borrow().clone().filter(|g| g.editable) else {
            return;
        };
        if (x as f32) < geometry.start || x as f32 > geometry.start + geometry.width {
            return;
        }
        let existing = geometry
            .stops
            .iter()
            .flatten()
            .min_by(|a, b| {
                (a.position_mm * geometry.pixels_per_mm + geometry.start - x as f32)
                    .abs()
                    .total_cmp(
                        &(b.position_mm * geometry.pixels_per_mm + geometry.start - x as f32).abs(),
                    )
            })
            .filter(|stop| {
                (stop.position_mm * geometry.pixels_per_mm + geometry.start - x as f32).abs()
                    < self.panel.from_dip_int(HIT_DIP) as f32
            })
            .cloned();
        let stop = existing.clone().unwrap_or(TabStop {
            position_mm: position(&geometry, x, fine),
            alignment: TYPES[self.kind.get()],
            leader: TabLeader::None,
        });
        let Some(ribbon) = self.ribbon.upgrade() else {
            return;
        };
        let state = ribbon.state.borrow();
        self.drag.replace(Some(Drag {
            geometry,
            before: existing.map(|stop| stop.position_mm),
            stop,
            remove: false,
            revision: state.editor.revision,
            selection: state.editor.selection.clone(),
        }));
        drop(state);
        self.panel.set_focus();
        self.panel.capture_mouse();
        self.panel.refresh(false, None);
    }

    fn motion(&self, x: i32, y: i32, fine: bool) {
        {
            let mut slot = self.indent_drag.borrow_mut();
            if let Some(drag) = slot.as_mut() {
                let position = position(&drag.geometry, x, fine);
                let width = drag.geometry.width / drag.geometry.pixels_per_mm;
                drag.current = drag.original.moved(drag.handle, position, width);
                self.panel.set_tooltip(&crate::i18n::text(&format!(
                    "{} · {:.1} mm",
                    crate::i18n::text(drag.handle.label()),
                    drag.current.position(drag.handle, width)
                )));
                self.panel.refresh(false, None);
                return;
            }
        }
        let mut slot = self.drag.borrow_mut();
        let Some(drag) = slot.as_mut() else {
            if let Some(geometry) = self.geometry.borrow().as_ref().filter(|g| g.editable) {
                let handle = ruler_indents::hit(
                    &self.panel,
                    geometry.indents,
                    geometry.start,
                    geometry.pixels_per_mm,
                    geometry.width / geometry.pixels_per_mm,
                    x,
                    y,
                );
                let label = handle.map_or_else(
                    || {
                        format!(
                            "{} tab. Click to change type; click the ruler to place it.",
                            crate::i18n::text(alignment_name(TYPES[self.kind.get()]))
                        )
                    },
                    |handle| crate::i18n::text(handle.label()),
                );
                self.panel.set_tooltip(&crate::i18n::text(&label));
            }
            return;
        };
        drag.remove = y < -self.panel.from_dip_int(HIT_DIP)
            || y > self.panel.get_client_size().height + self.panel.from_dip_int(HIT_DIP);
        drag.stop.position_mm = position(&drag.geometry, x, fine);
        self.panel.set_tooltip(&crate::i18n::text(
            &(&if drag.remove {
                "Release to remove this tab stop".into()
            } else {
                format!(
                    "{} tab · {:.1} mm",
                    crate::i18n::text(alignment_name(drag.stop.alignment)),
                    drag.stop.position_mm
                )
            }),
        ));
        self.panel.refresh(false, None);
    }

    fn finish(&self) {
        let indent_drag = self.indent_drag.borrow_mut().take();
        if let Some(drag) = indent_drag {
            if self.panel.has_capture() {
                self.panel.release_mouse();
            }
            if let Some(ribbon) = self.ribbon.upgrade() {
                let unchanged = {
                    let state = ribbon.state.borrow();
                    state.editor.revision == drag.revision
                        && state.editor.selection == drag.selection
                };
                if unchanged && drag.current != drag.original {
                    if let Err(error) = ribbon.edit(|editor| {
                        editor.apply_paragraph_properties(&drag.current.patch(drag.handle))
                    }) {
                        ribbon.state.borrow_mut().action_status = Some(error);
                    }
                } else {
                    ribbon.focus_document();
                }
            }
            self.panel.refresh(false, None);
            return;
        }
        let Some(drag) = self.drag.borrow_mut().take() else {
            if let Some(ribbon) = self.ribbon.upgrade() {
                ribbon.focus_document();
            }
            return;
        };
        if self.panel.has_capture() {
            self.panel.release_mouse();
        }
        let Some(ribbon) = self.ribbon.upgrade() else {
            return;
        };
        let unchanged = {
            let state = ribbon.state.borrow();
            state.editor.revision == drag.revision && state.editor.selection == drag.selection
        };
        if unchanged && !(drag.before.is_none() && drag.remove) {
            if let Err(error) = ribbon.edit(|editor| {
                editor.edit_tab_stop(drag.before, (!drag.remove).then_some(drag.stop))
            }) {
                ribbon.state.borrow_mut().action_status = Some(error);
            }
        }
        self.refresh();
        self.panel.refresh(false, None);
    }

    fn cancel(&self) {
        self.drag.borrow_mut().take();
        self.indent_drag.borrow_mut().take();
        if self.panel.has_capture() {
            self.panel.release_mouse();
        }
        self.panel.refresh(false, None);
    }

    fn paint_vertical(&self) {
        let dc = AutoBufferedPaintDC::new(&self.vertical);
        dc.set_background(BACKGROUND);
        dc.clear();
        dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
        if let Some(font) = self.vertical.get_font() {
            dc.set_font(&font);
        }
        let size = self.vertical.get_client_size();
        let d = |value| self.vertical.from_dip_int(value);
        if let Some(geometry) = self.geometry.borrow().as_ref() {
            for page in &geometry.vertical {
                dc.set_brush(PAPER, BrushStyle::Solid);
                dc.set_pen(PAPER, 1, PenStyle::Solid);
                let top = page.margins.y.max(0.0);
                let bottom = page.margins.max_y().min(size.height as f32);
                if bottom > top {
                    dc.draw_rectangle(
                        0,
                        top.round() as i32,
                        size.width - 1,
                        (bottom - top).round() as i32,
                    );
                }
                dc.set_pen(INK, 1, PenStyle::Solid);
                dc.set_text_foreground(INK);
                let first = ((page.paper.y.max(0.0) - page.margins.y) / geometry.pixels_per_mm)
                    .ceil() as i32;
                let last = ((page.paper.max_y().min(size.height as f32) - page.margins.y)
                    / geometry.pixels_per_mm)
                    .floor() as i32;
                let labels =
                    measurement_step(d(MIN_LABEL_SPACING_DIP) as f32 / geometry.pixels_per_mm, 10);
                let ticks =
                    measurement_step(d(MIN_TICK_SPACING_DIP) as f32 / geometry.pixels_per_mm, 1);
                for mm in first..=last {
                    if mm % ticks != 0 {
                        continue;
                    }
                    let y = (page.margins.y + mm as f32 * geometry.pixels_per_mm).round() as i32;
                    let tick = if mm % 10 == 0 {
                        8
                    } else if mm % 5 == 0 {
                        5
                    } else {
                        2
                    };
                    dc.draw_line(size.width - d(2), y, size.width - d(2 + tick), y);
                    if mm % labels == 0 {
                        let label = (mm / 10).to_string();
                        let extent = dc.get_text_extent(&crate::i18n::text(&(&label)));
                        dc.draw_text(
                            &crate::i18n::text(&(&label)),
                            (size.width - d(11) - extent.0).max(0),
                            y - extent.1 / 2,
                        );
                    }
                }
            }
        }
        dc.set_pen(Colour::rgb(197, 207, 220), 1, PenStyle::Solid);
        dc.draw_line(size.width - 1, 0, size.width - 1, size.height);
    }

    fn paint(&self) {
        let dc = AutoBufferedPaintDC::new(&self.panel);
        dc.set_background(BACKGROUND);
        dc.clear();
        dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
        if let Some(font) = self.panel.get_font() {
            dc.set_font(&font);
        }
        let size = self.panel.get_client_size();
        let d = |value| self.panel.from_dip_int(value);
        let geometry = self.geometry.borrow();
        if let Some(geometry) = geometry.as_ref() {
            dc.set_brush(PAPER, BrushStyle::Solid);
            dc.set_pen(PAPER, 1, PenStyle::Solid);
            dc.draw_rectangle(
                geometry.page.margins.x.round() as i32,
                0,
                geometry.page.margins.width.round() as i32,
                size.height - d(2),
            );
            let max_mm = (geometry.width / geometry.pixels_per_mm)
                .min(model::MAX_TAB_POSITION_MM)
                .floor() as i32;
            dc.set_text_foreground(INK);
            dc.set_pen(INK, 1, PenStyle::Solid);
            let min_tick_mm = ((geometry.page.paper.x.max(d(SELECTOR_DIP) as f32)
                - geometry.page.margins.x)
                / geometry.pixels_per_mm)
                .ceil() as i32;
            let max_tick_mm = ((geometry.page.paper.max_x().min(size.width as f32)
                - geometry.page.margins.x)
                / geometry.pixels_per_mm)
                .floor() as i32;
            let labels =
                measurement_step(d(MIN_LABEL_SPACING_DIP) as f32 / geometry.pixels_per_mm, 10);
            let ticks =
                measurement_step(d(MIN_TICK_SPACING_DIP) as f32 / geometry.pixels_per_mm, 1);
            let tick_baseline = size.height - d(SCALE_BOTTOM_INSET_DIP);
            let label_height = dc.get_text_extent(&crate::i18n::text(&("0123456789"))).1;
            let label_top = (tick_baseline - label_height - d(LABEL_BASELINE_GAP_DIP)).max(0);
            for mm in min_tick_mm..=max_tick_mm {
                if mm % ticks != 0 {
                    continue;
                }
                let x =
                    (geometry.page.margins.x + mm as f32 * geometry.pixels_per_mm).round() as i32;
                if x < d(SELECTOR_DIP) || x >= size.width {
                    continue;
                }
                let tick = if mm % 10 == 0 {
                    8
                } else if mm % 5 == 0 {
                    5
                } else {
                    2
                };
                dc.draw_line(x, tick_baseline, x, tick_baseline - d(tick));
                if mm % labels == 0 {
                    dc.draw_text(
                        &crate::i18n::text(&(&(mm / 10).to_string())),
                        x + d(2),
                        label_top,
                    );
                }
            }
            if geometry.editable {
                let drag = self.drag.borrow();
                let last_custom = geometry
                    .stops
                    .iter()
                    .flatten()
                    .filter(|stop| stop.alignment != TabAlignment::Bar)
                    .map(|stop| stop.position_mm)
                    .fold(0.0_f32, f32::max);
                let interval = geometry
                    .default_mm
                    .clamp(model::MIN_TAB_INTERVAL_MM, model::MAX_TAB_INTERVAL_MM);
                dc.set_pen(Colour::rgb(166, 175, 188), 1, PenStyle::Solid);
                let mut mm = ((last_custom / interval).floor() + 1.0) * interval;
                while mm <= max_mm as f32 {
                    let x = (geometry.start + mm * geometry.pixels_per_mm).round() as i32;
                    dc.draw_line(x, d(29), x, d(32));
                    mm += interval;
                }
                for stop in geometry.stops.iter().flatten() {
                    if drag
                        .as_ref()
                        .is_some_and(|drag| drag.before == Some(stop.position_mm))
                    {
                        continue;
                    }
                    marker(
                        &dc,
                        &self.panel,
                        (geometry.start + stop.position_mm * geometry.pixels_per_mm).round() as i32,
                        stop.alignment,
                        INK,
                    );
                }
                if let Some(drag) = drag.as_ref().filter(|drag| !drag.remove) {
                    marker(
                        &dc,
                        &self.panel,
                        (geometry.start + drag.stop.position_mm * geometry.pixels_per_mm).round()
                            as i32,
                        drag.stop.alignment,
                        ACCENT,
                    );
                }
                let indent_drag = self.indent_drag.borrow();
                let indents = indent_drag
                    .as_ref()
                    .map_or(geometry.indents, |drag| drag.current);
                ruler_indents::paint(
                    &dc,
                    &self.panel,
                    indents,
                    geometry.start,
                    geometry.pixels_per_mm,
                    geometry.width / geometry.pixels_per_mm,
                    indent_drag.as_ref().map(|drag| drag.handle),
                );
            }
        }
        dc.set_pen(Colour::rgb(197, 207, 220), 1, PenStyle::Solid);
        dc.draw_line(0, size.height - 1, size.width, size.height - 1);
        dc.set_brush(BACKGROUND, BrushStyle::Solid);
        dc.set_pen(Colour::rgb(197, 207, 220), 1, PenStyle::Solid);
        dc.draw_rectangle(0, 0, d(SELECTOR_DIP), size.height);
        marker(
            &dc,
            &self.panel,
            d(SELECTOR_DIP / 2),
            TYPES[self.kind.get()],
            INK,
        );
    }
}

fn measurement_step(minimum_mm: f32, base: i32) -> i32 {
    let mut magnitude = base;
    loop {
        for factor in [1, 2, 5] {
            let step = magnitude * factor;
            if step as f32 >= minimum_mm {
                return step;
            }
        }
        magnitude *= 10;
    }
}

fn position(geometry: &Geometry, x: i32, fine: bool) -> f32 {
    let step = if fine { FINE_SNAP_MM } else { SNAP_MM };
    (((x as f32 - geometry.start) / geometry.pixels_per_mm / step).round() * step).clamp(
        0.0,
        (geometry.width / geometry.pixels_per_mm).min(model::MAX_TAB_POSITION_MM),
    )
}

fn alignment_name(alignment: TabAlignment) -> &'static str {
    match alignment {
        TabAlignment::Left => "Left",
        TabAlignment::Center => "Center",
        TabAlignment::Right => "Right",
        TabAlignment::Decimal => "Decimal",
        TabAlignment::Bar => "Bar",
    }
}

fn marker(
    dc: &AutoBufferedPaintDC,
    panel: &Panel,
    x: i32,
    alignment: TabAlignment,
    colour: Colour,
) {
    let y = panel.from_dip_int(MARKER_Y_DIP);
    let unit = panel.from_dip_int(5);
    dc.set_pen(colour, panel.from_dip_int(2).max(1), PenStyle::Solid);
    dc.draw_line(x, y - unit, x, y + unit);
    match alignment {
        TabAlignment::Left => dc.draw_line(x, y + unit, x + unit, y + unit),
        TabAlignment::Right => dc.draw_line(x - unit, y + unit, x, y + unit),
        TabAlignment::Center | TabAlignment::Decimal => {
            dc.draw_line(x - unit, y + unit, x + unit, y + unit)
        }
        TabAlignment::Bar => {}
    }
    if alignment == TabAlignment::Decimal {
        dc.set_brush(colour, BrushStyle::Solid);
        dc.draw_circle(
            x + unit + panel.from_dip_int(2),
            y + unit,
            panel.from_dip_int(1).max(1),
        );
    }
}
