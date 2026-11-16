use editor::CommandId;

/// One command vocabulary backs Ribbon buttons and keyboard shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppCommand {
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PastePlain,
    SelectAll,
    Bold,
    Italic,
    Underline,
    StyleNormal,
    StyleHeading1,
    StyleTitle,
    InsertInlineEquation,
    /// An equation at the caret, made in the equation editor pane.
    InsertEquation,
}

impl AppCommand {
    pub fn editor_command(self) -> Option<CommandId> {
        match self {
            Self::Undo => Some(CommandId::Undo),
            Self::Redo => Some(CommandId::Redo),
            Self::SelectAll => Some(CommandId::SelectAll),
            Self::Bold => Some(CommandId::Bold),
            Self::Italic => Some(CommandId::Italic),
            Self::Underline => Some(CommandId::Underline),
            Self::StyleNormal => Some(CommandId::StyleNormal),
            Self::StyleHeading1 => Some(CommandId::StyleHeading1),
            Self::StyleTitle => Some(CommandId::StyleTitle),
            _ => None,
        }
    }
}
