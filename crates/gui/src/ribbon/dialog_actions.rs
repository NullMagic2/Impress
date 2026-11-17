//! Shared sizing and platform order for native dialog actions.
use super::*;
use wxdragon::sizers::{BoxSizer, SizerFlag};

/// Minimum width for short captions. Long translations keep their natural width.
pub(super) const WIDTH_DIP: i32 = 96;
const GAP_DIP: i32 = 12;

pub(super) fn size(button: Button) {
    let width = button
        .from_dip_int(WIDTH_DIP)
        .max(button.get_best_size().width);
    button.set_min_size(Size::new(width, -1));
}

/// Add a centered pair without replacing the caller's IDs or event handlers.
pub(super) fn pair(row: &BoxSizer, accept: Button, cancel: Button) {
    let width = accept
        .from_dip_int(WIDTH_DIP)
        .max(accept.get_best_size().width)
        .max(cancel.get_best_size().width);
    for button in [accept, cancel] {
        button.set_min_size(Size::new(width, -1));
    }
    let [first, last] = if cfg!(windows) {
        [accept, cancel]
    } else {
        [cancel, accept]
    };
    row.add_stretch_spacer(1);
    row.add(&first, 0, SizerFlag::Right, first.from_dip_int(GAP_DIP));
    row.add(&last, 0, SizerFlag::empty(), 0);
    row.add_stretch_spacer(1);
}

pub(super) fn heading(label: &wxdragon::widgets::StaticText) {
    if let Some(mut font) = label.get_font() {
        font.make_bold();
        label.set_font(&font);
    }
}

pub(super) fn divider(window: &Dialog, root: &BoxSizer, inset: i32) {
    let line = wxdragon::widgets::StaticLine::builder(window).build();
    root.add(
        &line,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        inset,
    );
}
