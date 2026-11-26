//! Discoverable positioning and paragraph anchoring from Insert → Table.
use super::*;
use crate::table_ui::TableOp;
use wxdragon::geometry::Point;
const FIRST: i32 = 24_600;

use super::table_bar::{draw_icon, Icon};
use wxdragon::bitmap::Bitmap;
use std::ffi::{c_char, c_void, CString};

unsafe extern "C" {
    fn wxd_Menu_AppendIcon(menu: *mut c_void, id: i32, label: *const c_char,
                          image: *const c_void, checked: i32, submenu: *mut c_void);
}
#[derive(Clone, Copy)]
enum MenuIcon { Table(Icon), Art(&'static str) }

fn bitmap(icon: MenuIcon, side: u32) -> Option<Bitmap> {
    match icon {
        MenuIcon::Art(name) => Bitmap::from_rgba(&art::raster(name, side, side, false)?, side, side),
        MenuIcon::Table(icon) => {
            let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
            draw_icon(&mut pixmap, icon, 0.0, 0.0, side as f32, true);
            crate::crisp::bitmap(&pixmap)
        }
    }
}
fn append(menu: &mut Menu, id: i32, label: &str, icon: MenuIcon, checked: Option<bool>, submenu: Option<Menu>, side: u32) {
    let label = CString::new(crate::i18n::text(label)).unwrap_or_default();
    let image = bitmap(icon, side);
    // Native menu owns its submenu and copies the bitmap before these wrappers drop.
    unsafe { wxd_Menu_AppendIcon(menu.as_mut_ptr().cast(), FIRST + id, label.as_ptr(),
        image.as_ref().map_or(std::ptr::null(), |image| image.as_const_ptr().cast()),
        checked.map_or(-1, i32::from), submenu.map_or(std::ptr::null_mut(), |menu| menu.into_raw_mut().cast())); }
}

pub(super) fn show(ribbon: &Ribbon) {
    let button = ribbon.ui.insert_table_button;
    let anchor = crate::gtk_native::to_screen(&button, Point::new(0, button.get_size().height));
    show_at(ribbon, anchor);
}

/// Preserve the current cell selection while opening Insert → Table.
fn show_at(ribbon: &Ribbon, anchor: Point) {
    let state = ribbon.state.borrow();
    let facts = crate::table_ui::facts(&state);
    let alignment = state.formatting_summary().alignment;
    drop(state);
    let movable = facts.as_ref().is_some_and(|f| f.movable);
    let position = facts.as_ref().and_then(|f| f.position.as_ref());
    let side = ribbon.ui.main_frame.from_dip_int(20).max(16) as u32;
    let mut text = Menu::builder().build();
    for (id, label, icon, value) in [
        (13, "Align left", "align-left", ParagraphAlignment::Left),
        (14, "Center", "align-center", ParagraphAlignment::Center),
        (15, "Align right", "align-right", ParagraphAlignment::Right),
        (16, "Justify", "align-justify", ParagraphAlignment::Justify),
    ] {
        append(&mut text, id, label, MenuIcon::Art(icon), Some(facts.is_some() && alignment == Some(value)), None, side);
    }
    text.append_separator();
    for (id, label, icon, value) in [
        (10, "Align text to the top of the cell", Icon::AlignTop, 0),
        (11, "Centre text vertically", Icon::AlignCenter, 1),
        (12, "Align text to the bottom of the cell", Icon::AlignBottom, 2),
    ] {
        append(&mut text, id, label, MenuIcon::Table(icon), Some(facts.as_ref().is_some_and(|f|f.align==value)), None, side);
    }
    let mut wrap = Menu::builder().build();
    for (id, label, icon, checked) in [
        (1, "In Line with Text", Icon::Inline, facts.is_some() && position.is_none()),
        (2, "Text around table", Icon::WrapAround, position.is_some_and(|p|p.wrap==model::TableWrap::Around)),
        (3, "Text above and below table", Icon::WrapTopBottom, position.is_some_and(|p|p.wrap==model::TableWrap::TopAndBottom)),
        (9, "In Front of Text", Icon::WrapFront, position.is_some_and(|p|p.wrap==model::TableWrap::InFrontOfText)),
        (17, "Behind Text", Icon::WrapBehind, position.is_some_and(|p|p.wrap==model::TableWrap::BehindText)),
    ] {
        append(&mut wrap, id, label, MenuIcon::Table(icon), Some(checked), None, side);
        wrap.enable_item(FIRST + id, movable);
    }
    let mut menu = Menu::builder().build();
    append(&mut menu, 8, "Insert table…", MenuIcon::Art("table"), None, None, side);
    append(&mut menu, 0, "Table tools…", MenuIcon::Table(Icon::SelTable), None, None, side);
    menu.append_separator();
    append(&mut menu, 20, "Text alignment", MenuIcon::Table(Icon::AlignCenter), None, Some(text), side);
    append(&mut menu, 21, "Text wrapping", MenuIcon::Table(Icon::WrapAround), None, Some(wrap), side);
    menu.append_separator();
    append(&mut menu, 4, "Table position…", MenuIcon::Table(Icon::Position), None, None, side);
    append(&mut menu, 6, "Fit table to page margins", MenuIcon::Table(Icon::Distribute), None, None, side);
    menu.append_separator();
    append(&mut menu, 7, "Repeat header", MenuIcon::Table(Icon::Header), Some(facts.as_ref().is_some_and(|f|f.header)), None, side);
    menu.enable_item(FIRST + 8, facts.is_none());
    for id in [0, 6, 7, 20, 21] { menu.enable_item(FIRST + id, facts.is_some()); }
    menu.enable_item(FIRST + 4, movable);
    ribbon.table_insert_requested.set(false);
    ribbon.ui.main_frame.popup_menu(&mut menu, Some(anchor));
    // Finish native menu cleanup before focusing the keyboard-operated grid.
    if ribbon.table_insert_requested.replace(false) {
        table_picker::show(ribbon);
    }
}

pub(super) fn handle(ribbon: &Ribbon, id: i32) -> bool {
    match id - FIRST {
        0 => ribbon.open_table_tools(),
        8 => ribbon.table_insert_requested.set(true),
        1 => ribbon.run(Action::Table(TableOp::Float(false))),
        2 | 3 | 9 | 17 => {
            let wrap = match id - FIRST { 2 => model::TableWrap::Around, 3 => model::TableWrap::TopAndBottom, 9 => model::TableWrap::InFrontOfText, _ => model::TableWrap::BehindText };
            if let Err(error) = set_wrap(ribbon, wrap) {
                ribbon.state.borrow_mut().action_status = Some(error);
            }
        }
        4 => ribbon.run(Action::TablePosition),
        6 => ribbon.run(Action::Table(TableOp::FitToMargins)),
        7 => ribbon.run(Action::Table(TableOp::HeaderRow)),
        10..=12 => ribbon.run(Action::Table(TableOp::Align((id - FIRST - 10) as u8))),
        13..=16 => ribbon.run(Action::Align(match id - FIRST {
            13 => ParagraphAlignment::Left, 14 => ParagraphAlignment::Center,
            15 => ParagraphAlignment::Right, _ => ParagraphAlignment::Justify,
        })),
        _ => return false,
    }
    true
}

/// A wrapping choice is one editor transaction, including conversion to float.
pub(super) fn float_table(ribbon: &Ribbon, around: bool) -> Result<(), String> {
    set_wrap(ribbon, if around { model::TableWrap::Around } else { model::TableWrap::TopAndBottom })
}

fn set_wrap(ribbon: &Ribbon, wrap: model::TableWrap) -> Result<(), String> {
    let id = crate::table_ui::context(&ribbon.state.borrow()).ok_or("Place the caret in a table first")?.table_id;
    if !crate::table_ui::facts(&ribbon.state.borrow()).is_some_and(|f|f.movable) {
        return Err("Only tables in the main text can be moved.".into());
    }
    ribbon.edit_table(&id, |e| e.set_table_wrap(&id, wrap))
}
