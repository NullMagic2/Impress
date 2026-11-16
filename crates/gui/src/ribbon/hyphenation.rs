//! Hyphenation (the Hyphenation menu, as the Typography one): automatic
//! hyphenation on or off for the selected paragraphs or the whole document,
//! as one undo step. Words break by the patterns of each paragraph's text
//! language.
use super::*;

/// Whether the paragraph at the caret hyphenates, with its style resolved.
pub(super) fn hyphenates_at_caret(editor: &EditorState) -> bool {
    let Some(position) = editor.focus_position() else { return false; };
    model::document_paragraphs(&editor.document).into_iter().find(|p| p.id == position.node_id)
        .and_then(|p| editor.document.styles.resolve_paragraph_with_direct(p.style.as_ref(), &p.properties).hyphenation)
        .unwrap_or(false)
}

/// Turns automatic hyphenation on or off for the selected paragraphs, or
/// for the whole document.
pub(super) fn apply(ribbon: &Ribbon, on: bool, everywhere: bool) -> Result<(), String> {
    let properties = ParagraphProperties { hyphenation: Some(on), ..Default::default() };
    ribbon.edit(|e| {
        if !everywhere { return e.apply_paragraph_properties(&properties); }
        // The whole body, then the caret back where it was.
        let kept = e.selection.clone();
        e.select_all();
        let result = e.apply_paragraph_properties(&properties);
        e.selection = kept;
        result
    })
}
