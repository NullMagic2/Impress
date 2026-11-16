//! Native document zoom settings, using the same limits as the viewport.
use super::*;
use wxdragon::{
    dialogs::{DialogBuilder, DialogStyle},
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{spinctrl::SpinCtrl, static_text::StaticText},
};

const GAP_DIP: i32 = 12;
const PERCENT_WIDTH_DIP: i32 = 100;
const ZOOM_STEP_PERCENT: i32 = 10;

pub(super) fn show(ribbon: &Ribbon) {
    let window = DialogBuilder::new(&ribbon.ui.main_frame, &crate::i18n::text("Zoom"))
        .with_style(DialogStyle::DefaultDialogStyle)
        .build();
    let dip = |value| window.from_dip_int(value);
    let minimum = (render::ViewportState::MIN_ZOOM * 100.0) as i32;
    let maximum = (render::ViewportState::MAX_ZOOM * 100.0) as i32;
    let initial = (ribbon.state.borrow().viewport.zoom * 100.0).round() as i32;
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let label = StaticText::builder(&window)
        .with_label(&crate::i18n::text("Zoom to:"))
        .build();
    let percent = SpinCtrl::builder(&window)
        .with_range(minimum, maximum)
        .with_initial_value(initial)
        .with_size(Size::new(dip(PERCENT_WIDTH_DIP), -1))
        .build();
    let unit = StaticText::builder(&window).with_label("%").build();
    row.add(
        &label,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        dip(GAP_DIP),
    );
    row.add(
        &percent,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        dip(GAP_DIP),
    );
    row.add(&unit, 0, SizerFlag::AlignCenterVertical, 0);
    root.add_sizer(&row, 0, SizerFlag::All, dip(GAP_DIP));
    let steps = BoxSizer::builder(Orientation::Horizontal).build();
    let out = Button::builder(&window)
        .with_label(&crate::i18n::text("Zoom out"))
        .build();
    let zoom_in = Button::builder(&window)
        .with_label(&crate::i18n::text("Zoom in"))
        .build();
    steps.add(&out, 1, SizerFlag::Right, dip(GAP_DIP));
    steps.add(&zoom_in, 1, SizerFlag::empty(), 0);
    root.add_sizer(
        &steps,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom | SizerFlag::Expand,
        dip(GAP_DIP),
    );
    let selected_fit = Rc::new(Cell::new(None::<(i32, bool)>));
    let update: Rc<dyn Fn()> = Rc::new(move || {
        out.enable(percent.value() > minimum);
        zoom_in.enable(percent.value() < maximum);
    });
    for (button, delta) in [(out, -ZOOM_STEP_PERCENT), (zoom_in, ZOOM_STEP_PERCENT)] {
        let update = Rc::clone(&update);
        let fit = Rc::clone(&selected_fit);
        button.on_click(move |_| {
            fit.set(None);
            percent.set_value((percent.value() + delta).clamp(minimum, maximum));
            update();
        });
    }
    {
        let update = Rc::clone(&update);
        let fit = Rc::clone(&selected_fit);
        percent.on_value_changed(move |_| {
            fit.set(None);
            update();
        });
    }
    let presets = BoxSizer::builder(Orientation::Horizontal).build();
    for (label, mode, value) in [
        ("100%", None, Some(1.0)),
        ("One page", Some(true), ribbon.fit_zoom(true)),
        ("Page width", Some(false), ribbon.fit_zoom(false)),
    ] {
        let button = Button::builder(&window)
            .with_label(&crate::i18n::text(label))
            .build();
        presets.add(&button, 0, SizerFlag::Right, dip(GAP_DIP));
        button.enable(value.is_some());
        let update = Rc::clone(&update);
        let fit = Rc::clone(&selected_fit);
        button.on_click(move |_| {
            if let Some(value) = value {
                let value = (value * 100.0).round() as i32;
                percent.set_value(value);
                fit.set(mode.map(|mode| (value, mode)));
                update();
            }
        });
    }
    root.add_sizer(
        &presets,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        dip(GAP_DIP),
    );
    let footer = BoxSizer::builder(Orientation::Horizontal).build();
    let ok = Button::builder(&window)
        .with_id(ID_OK)
        .with_label(&crate::i18n::text("OK"))
        .build();
    let cancel = Button::builder(&window)
        .with_id(ID_CANCEL)
        .with_label(&crate::i18n::text("Cancel"))
        .build();
    footer.add_stretch_spacer(1);
    footer.add(&ok, 0, SizerFlag::Right, dip(GAP_DIP));
    footer.add(&cancel, 0, SizerFlag::empty(), 0);
    root.add_sizer(&footer, 0, SizerFlag::All | SizerFlag::Expand, dip(GAP_DIP));
    window.set_sizer_and_fit(root, true);
    window.set_escape_id(ID_CANCEL);
    ok.set_default();
    ok.on_click(move |_| window.end_modal(ID_OK));
    cancel.on_click(move |_| window.end_modal(ID_CANCEL));
    update();
    crate::placement::centre(&window);
    percent.set_focus();
    let accepted = window.show_modal() == ID_OK;
    let value = percent.value();
    window.destroy();
    if accepted {
        if let Some((_, mode)) = selected_fit
            .get()
            .filter(|(expected, _)| *expected == value)
        {
            ribbon.fit(mode);
        } else {
            ribbon.zoom_to(value as f32 / 100.0);
        }
    }
    ribbon.focus_document();
}
