//! Read-only document statistics and application information.
use super::*;
use wxdragon::{
    dialogs::{
        DialogBuilder,
        about_dialog::{AboutDialogInfo, show_about_box},
    },
    sizers::{BoxSizer, FlexGridSizer, Orientation, SizerFlag},
    widgets::StaticText,
};

const MARGIN_DIP: i32 = 18;

pub(super) fn about(parent: &Frame) {
    let mut info = AboutDialogInfo::new();
    info.set_name("Impress");
    info.set_version(env!("CARGO_PKG_VERSION"));
    let description = crate::i18n::text(
        "\n\nDocument editing, styles, DOCX, Typst, RTF, PDF output, and native Windows printing.",
    );
    info.set_description(description.trim());
    info.set_website("https://github.com/NullMagic2/Impress");
    if let Some(icon) = art::bitmap_from_png(include_bytes!("../../assets/ribbon/logo.png"), 64) {
        info.set_icon(&icon);
    }
    show_about_box(&info, Some(parent));
}

pub(super) fn word_count(parent: &Frame, stats: navigation::TextStats) {
    let window = DialogBuilder::new(parent, &crate::i18n::text("Word count")).build();
    let margin = window.from_dip_int(MARGIN_DIP);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let heading = StaticText::builder(&window)
        .with_label(&crate::i18n::text("Body text"))
        .build();
    dialog_actions::heading(&heading);
    root.add(&heading, 0, SizerFlag::All, margin);
    let metrics = FlexGridSizer::builder(3, 2)
        .with_gap(window.from_dip(Size::new(24, 8)))
        .build();
    for (label, value) in [
        ("Words", stats.words),
        ("Unicode characters, including spaces", stats.characters),
        ("Paragraphs", stats.paragraphs),
    ] {
        let label = StaticText::builder(&window)
            .with_label(&crate::i18n::text(label))
            .build();
        let value = StaticText::builder(&window)
            .with_label(&value.to_string())
            .build();
        metrics.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
        metrics.add(&value, 0, SizerFlag::AlignRight, 0);
    }
    root.add_sizer(&metrics, 0, SizerFlag::Left | SizerFlag::Right, margin);
    let note = StaticText::builder(&window)
        .with_label(&crate::i18n::text(
            "Equations and image contents are not counted as text.",
        ))
        .build();
    note.wrap(window.from_dip_int(320));
    root.add(&note, 0, SizerFlag::All, margin);
    dialog_actions::divider(&window, &root, margin);
    let close = Button::builder(&window)
        .with_id(ID_OK)
        .with_label(&crate::i18n::text("Close"))
        .build();
    dialog_actions::size(close);
    root.add(
        &close,
        0,
        SizerFlag::AlignCenterHorizontal | SizerFlag::All,
        margin,
    );
    close.set_default();
    close.on_click(move |_| window.end_modal(ID_OK));
    window.set_escape_id(ID_OK);
    window.set_sizer_and_fit(root, true);
    crate::placement::centre(&window);
    window.show_modal();
    window.destroy();
}
