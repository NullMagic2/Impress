//! Native section-break commands shared by Insert and Layout.
use super::*;
use wxdragon::geometry::Point;

/// IDs reserved for the four section insertion commands.
const FIRST_CHOICE_ID: i32 = 32_600;
/// Layout follows Insert in the fixed eight-page ribbon.
const LAYOUT_TAB: usize = 3;

struct Choice {
    start: SectionStart,
    label: &'static str,
    help: &'static str,
}

const CHOICES: [Choice; 4] = [
    Choice {
        start: SectionStart::NewPage,
        label: "Next page",
        help: "Start the new section on the next page",
    },
    Choice {
        start: SectionStart::Continuous,
        label: "Continuous",
        help: "Start the new section on the same page",
    },
    Choice {
        start: SectionStart::EvenPage,
        label: "Even page",
        help: "Start the new section on the next even-numbered page",
    },
    Choice {
        start: SectionStart::OddPage,
        label: "Odd page",
        help: "Start the new section on the next odd-numbered page",
    },
];

pub(super) fn show(ribbon: &Ribbon) {
    let mut builder = Menu::builder();
    for (index, choice) in CHOICES.iter().enumerate() {
        builder = builder.append_item(
            FIRST_CHOICE_ID + index as i32,
            &crate::i18n::text(choice.label),
            &crate::i18n::text(choice.help),
        );
    }
    let mut menu = builder.build();
    let button = if ribbon.tab_index.get() == LAYOUT_TAB {
        ribbon.ui.breaks_button
    } else {
        ribbon.ui.section_break_button
    };
    let anchor = crate::gtk_native::to_screen(&button, Point::new(0, button.get_size().height));
    ribbon.ui.main_frame.popup_menu(&mut menu, Some(anchor));
}

/// Consume only this menu's IDs; selecting a command is one undoable edit.
pub(super) fn handle(ribbon: &Ribbon, id: i32) -> bool {
    let Some(choice) = id
        .checked_sub(FIRST_CHOICE_ID)
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| CHOICES.get(index))
    else {
        return false;
    };
    if let Err(error) = ribbon.edit(|editor| editor.insert_section_break(choice.start)) {
        ribbon.state.borrow_mut().action_status = Some(error);
    }
    ribbon.last_key.borrow_mut().take();
    true
}
