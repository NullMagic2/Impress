//! One native, nonmodal swatch grid for ribbon, table and dialog color controls.
use super::*;
use std::ffi::{c_char, c_void, CStr, CString};
use wxdragon::geometry::Point;
use wxdragon::widgets::BitmapComboBox;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Text,
    Highlight,
    TableText,
    CellFill,
}

type Callback = unsafe extern "C" fn(*mut c_void, i32, *const c_char) -> i32;
unsafe extern "C" {
    fn wxd_ColourPalette_Show(
        parent: *mut c_void,
        x: i32,
        y: i32,
        selected: *const c_char,
        empty: i32,
        custom: *const c_char,
        labels: *const c_char,
        names: *const c_char,
        add_icon: *const c_void,
        add_disabled: *const c_void,
        delete_icon: *const c_void,
        delete_disabled: *const c_void,
        callback: Callback,
        context: *mut c_void,
    );
    fn wxd_ColourPalette_BindCombo(combo: *mut c_void, callback: Callback, context: *mut c_void);
    fn wxd_ColourPalette_RefreshCombo(combo: *mut c_void);
    fn wxd_ColourPalette_ComboAnchor(combo: *mut c_void, x: *mut i32, y: *mut i32);
}
struct Handler(Box<dyn Fn(i32, &str) -> Result<(), String>>);
// C++ owns the handler until Closed, exactly once, including parent teardown.
unsafe extern "C" fn callback(context: *mut c_void, action: i32, color: *const c_char) -> i32 {
    if action == 3 {
        drop(unsafe { Box::from_raw(context.cast::<Handler>()) });
        return 1;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let handler = unsafe { &*context.cast::<Handler>() };
        let value = if color.is_null() {
            ""
        } else {
            unsafe { CStr::from_ptr(color) }.to_str().unwrap_or("")
        };
        (handler.0)(action, value)
    }));
    match result {
        Ok(Ok(())) => 1,
        Ok(Err(error)) => {
            eprintln!("Color palette: {error}");
            0
        }
        Err(_) => {
            eprintln!("Color palette callback failed");
            0
        }
    }
}
fn popup(
    parent: &impl WxWidget,
    anchor: Point,
    selected: Option<&str>,
    empty: bool,
    apply: impl Fn(Option<String>) -> Result<(), String> + 'static,
) {
    let selected =
        CString::new(selected.filter(|s| *s != "none").unwrap_or("")).unwrap_or_default();
    let custom = CString::new(custom_colours::load().join("\n")).unwrap_or_default();
    let labels = CString::new(
        [
            if empty { "No Color" } else { "Automatic" },
            "Standard colors",
            "Custom colors",
            "Add…",
            "Delete",
            "Colors",
        ]
        .map(crate::i18n::text)
        .join("\n"),
    )
    .unwrap_or_default();
    let names = CString::new(
        styles::FONT_COLOURS
            .map(|(name, _)| crate::i18n::text(name))
            .join("\n"),
    )
    .unwrap_or_default();
    let handler = Box::new(Handler(Box::new(move |action, color| match action {
        0 => apply((!color.is_empty()).then(|| color.to_owned())),
        1 => custom_colours::add(color),
        2 => custom_colours::remove(color),
        _ => Ok(()),
    })));
    let add_icon = art::load("add", 18, 18);
    let delete_icon = art::load("trash", 18, 18);
    let bundle = |art: &Option<art::ButtonArt>, disabled| art.as_ref().map_or(std::ptr::null(), |art| {
        if disabled { art.disabled.as_ptr().cast() } else { art.normal.as_ptr().cast() }
    });
    unsafe {
        wxd_ColourPalette_Show(
            parent.handle_ptr().cast(),
            anchor.x,
            anchor.y,
            selected.as_ptr(),
            i32::from(empty),
            custom.as_ptr(),
            labels.as_ptr(),
            names.as_ptr(),
            bundle(&add_icon, false),
            bundle(&add_icon, true),
            bundle(&delete_icon, false),
            bundle(&delete_icon, true),
            callback,
            Box::into_raw(handler).cast(),
        );
    }
}
fn table_target(state: &CanvasState) -> Option<(NodeId, Vec<NodeId>)> {
    if let Some(objects_ui::SelectedObject::Cells { table_id, cell_ids }) = &state.selected_object {
        return Some((table_id.clone(), cell_ids.clone()));
    }
    let context = state.editor.table_cell_context()?;
    Some((context.table_id, vec![context.cell_id]))
}
fn current(ribbon: &Ribbon, kind: Kind) -> Option<String> {
    let state = ribbon.state.borrow();
    match kind {
        Kind::Text => state.formatting_summary().character.color,
        Kind::Highlight => state.formatting_summary().character.highlight,
        Kind::TableText => {
            let (table, cells) = table_target(&state)?;
            state
                .editor
                .table_formatting_summary(&table, &cells)
                .character
                .color
        }
        Kind::CellFill => {
            let (id, ids) = table_target(&state)?;
            let table = state.editor.table_by_id(&id)?;
            let mut cells = table
                .rows
                .iter()
                .flat_map(|r| &r.cells)
                .filter(|c| ids.contains(&c.id));
            let fill = cells.next()?.shading.clone();
            cells.all(|c| c.shading == fill).then_some(fill).flatten()
        }
    }
}
pub(super) fn show(ribbon: &Ribbon, kind: Kind, anchor: Point) {
    let selected = current(ribbon, kind);
    let empty = matches!(kind, Kind::Highlight | Kind::CellFill);
    let owner = ribbon.owner.clone();
    ribbon.focus_document();
    popup(
        &ribbon.ui.main_frame,
        anchor,
        selected.as_deref(),
        empty,
        move |color| {
            let Some(ribbon) = owner.upgrade() else {
                return Ok(());
            };
            let color = color.unwrap_or_else(|| if empty { "none" } else { "#000000" }.into());
            let patch = if kind == Kind::Highlight {
                CharacterProperties {
                    highlight: Some(color.clone()),
                    ..Default::default()
                }
            } else {
                CharacterProperties {
                    color: Some(color.clone()),
                    ..Default::default()
                }
            };
            let result = if matches!(kind, Kind::TableText | Kind::CellFill) {
                let target = table_target(&ribbon.state.borrow());
                if let Some((table, cells)) = target {
                    ribbon.edit_without_focus(|editor| {
                        if kind == Kind::CellFill {
                            editor.set_table_cell_fill(&table, &cells, &color)
                        } else {
                            editor.with_table_formatting(&table, &cells, |editor| {
                                editor.apply_character_properties(&patch)
                            })
                        }
                    })
                } else {
                    Ok(())
                }
            } else {
                ribbon.edit_canvas(|state| {
                    state.apply_formatting(|editor| editor.apply_character_properties(&patch))
                })
            };
            if let Err(error) = &result {
                ribbon.state.borrow_mut().action_status = Some(error.clone());
            }
            result
        },
    );
}
/// Preserve the existing dialog draft/read APIs; replace just the native picker
/// presentation with a swatch button in the same sizer slot.
pub(super) fn bind_combo(combo: BitmapComboBox, parent: Dialog, changed: impl Fn() + 'static) {
    let changed: Rc<dyn Fn()> = Rc::new(changed);
    let handler = Box::new(Handler(Box::new(move |action, _| {
        if action != 4 {
            return Ok(());
        }
        let current = styles::colour_value(&combo.get_value());
        let empty = crate::i18n::source(&combo.get_string(0)) == styles::NO_COLOUR;
        let (mut x, mut y) = (0, 0);
        unsafe {
            wxd_ColourPalette_ComboAnchor(combo.handle_ptr().cast(), &mut x, &mut y);
        }
        let changed = changed.clone();
        popup(
            &parent,
            Point::new(x, y),
            current.as_deref(),
            empty,
            move |color| {
                let none = if empty {
                    styles::NO_COLOUR
                } else {
                    styles::AUTOMATIC
                };
                let label = color
                    .as_deref()
                    .map(|hex| styles::colour_label(Some(hex)))
                    .unwrap_or_else(|| none.into());
                let label = crate::i18n::text(&label);
                let index = (0..combo.get_count())
                    .find(|&i| combo.get_string(i) == label)
                    .unwrap_or_else(|| {
                        let index = combo.get_count();
                        combo.append(
                            &label,
                            styles::colour_swatch(
                                &parent,
                                color.as_deref(),
                                empty && color.is_none(),
                            )
                            .as_ref(),
                        );
                        index
                    });
                combo.set_selection(index as i32);
                refresh_combo(combo);
                changed();
                Ok(())
            },
        );
        Ok(())
    })));
    unsafe {
        wxd_ColourPalette_BindCombo(
            combo.handle_ptr().cast(),
            callback,
            Box::into_raw(handler).cast(),
        );
    }
}
pub(super) fn refresh_combo(combo: BitmapComboBox) {
    unsafe {
        wxd_ColourPalette_RefreshCombo(combo.handle_ptr().cast());
    }
}
