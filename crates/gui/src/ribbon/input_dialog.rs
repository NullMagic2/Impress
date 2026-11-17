//! Common native layout for choice lists and short or multiline entry dialogs.
use super::*;
use wxdragon::{
    dialogs::{DialogBuilder, DialogStyle},
    id::ID_CANCEL,
    sizers::{BoxSizer, Orientation, SizerFlag},
    widgets::{
        StaticText, TextCtrl,
        listbox::{ListBox, ListBoxStyle},
    },
};

const MARGIN_DIP: i32 = 18;
const FIELD_WIDTH_DIP: i32 = 380;
const MULTILINE_HEIGHT_DIP: i32 = 112;

struct Prompt {
    window: Dialog,
    root: BoxSizer,
}

impl Prompt {
    fn new(parent: &impl WxWidget, title: &str, message: &str) -> Self {
        let window = DialogBuilder::new(parent, &crate::i18n::text(title))
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let root = BoxSizer::builder(Orientation::Vertical).build();
        let message = crate::i18n::text(message);
        // A revision or comment can contain many paragraphs. Keep it readable
        // and scrollable without pushing the action buttons below the display.
        if message.lines().count() > 5 || message.chars().count() > 320 {
            let details = TextCtrl::builder(&window)
                .with_style(TextCtrlStyle::ReadOnly | TextCtrlStyle::MultiLine)
                .with_size(window.from_dip(Size::new(FIELD_WIDTH_DIP, MULTILINE_HEIGHT_DIP)))
                .with_value(&message)
                .build();
            root.add(
                &details,
                1,
                SizerFlag::All | SizerFlag::Expand,
                window.from_dip_int(MARGIN_DIP),
            );
        } else if !message.is_empty() {
            let label = StaticText::builder(&window)
                .with_label(&message.replace('&', "&&"))
                .build();
            label.wrap(window.from_dip_int(FIELD_WIDTH_DIP));
            root.add(
                &label,
                0,
                SizerFlag::All | SizerFlag::Expand,
                window.from_dip_int(MARGIN_DIP),
            );
        }
        Self { window, root }
    }

    fn show(self, field: &impl WxWidget) -> bool {
        let window = self.window;
        let margin = window.from_dip_int(MARGIN_DIP);
        self.root.add(
            field,
            1,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom | SizerFlag::Expand,
            margin,
        );
        dialog_actions::divider(&window, &self.root, margin);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let ok = Button::builder(&window)
            .with_id(ID_OK)
            .with_label(&crate::i18n::text("OK"))
            .build();
        let cancel = Button::builder(&window)
            .with_id(ID_CANCEL)
            .with_label(&crate::i18n::text("Cancel"))
            .build();
        dialog_actions::pair(&row, ok, cancel);
        self.root
            .add_sizer(&row, 0, SizerFlag::All | SizerFlag::Expand, margin);
        window.set_sizer_and_fit(self.root, true);
        ok.set_default();
        window.set_escape_id(ID_CANCEL);
        ok.on_click(move |_| window.end_modal(ID_OK));
        cancel.on_click(move |_| window.end_modal(ID_CANCEL));
        crate::placement::centre(&window);
        field.set_focus();
        window.show_modal() == ID_OK
    }
}

pub(super) fn text(
    parent: &impl WxWidget,
    title: &str,
    message: &str,
    initial: &str,
    multiline: bool,
) -> Option<String> {
    let prompt = Prompt::new(parent, title, message);
    let window = prompt.window;
    let height = if multiline { MULTILINE_HEIGHT_DIP } else { -1 };
    let style = if multiline {
        TextCtrlStyle::MultiLine
    } else {
        TextCtrlStyle::Default
    };
    let field = TextCtrl::builder(&window)
        .with_value(initial)
        .with_style(style)
        .with_size(window.from_dip(Size::new(FIELD_WIDTH_DIP, height)))
        .build();
    field.set_name(&crate::i18n::text(title));
    field.select_all();
    let result = prompt.show(&field).then(|| field.get_value());
    window.destroy();
    result
}

pub(super) fn choice(
    parent: &impl WxWidget,
    title: &str,
    message: &str,
    items: &[String],
) -> Option<usize> {
    let prompt = Prompt::new(parent, title, message);
    let window = prompt.window;
    let rows = items.len().clamp(3, 9) as i32;
    let field = ListBox::builder(&window)
        .with_style(ListBoxStyle::Default | ListBoxStyle::HorizontalScrollbar)
        .with_size(window.from_dip(Size::new(FIELD_WIDTH_DIP, rows * 24)))
        .build();
    field.set_name(&crate::i18n::text(title));
    for item in items {
        field.append(item);
    }
    if !items.is_empty() {
        field.set_selection(0, true);
    }
    field.on_item_double_clicked(move |_| window.end_modal(ID_OK));
    let accepted = prompt.show(&field);
    let result = field
        .get_selection()
        .filter(|_| accepted)
        .map(|index| index as usize);
    window.destroy();
    result
}
