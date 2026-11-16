# Paragraph formatting and multiline text

Plain-text insertion (including clipboard paste and IME commits) treats LF,
CRLF and CR as paragraph boundaries, preserving blank paragraphs and trailing
newlines. Insertion remains one undoable edit and retains surrounding text,
run formatting and the active selection for Undo.

Earlier builds could store several visible paragraphs as newline characters
inside one text run. The editor repairs these blocks on opening, including
table cells, headers, footers, notes and text boxes. Existing styles, direct
formatting and resources are retained; reference ranges are rebased to the new
paragraph identities. Explicit soft line-break objects remain in their original
paragraph. The original file is only updated when the user saves it.

Applying a paragraph style excludes a following paragraph when a nonempty
selection ends at byte zero of that paragraph. This applies in both selection
directions. Paragraph alignment, lists, tabs and formatting summaries share the
same target boundary. A collapsed caret still formats its current paragraph;
selected empty paragraphs between the endpoints remain included.

The raw text spans retain the selected paragraph separator for copying and
pasting. Applying a style leaves shared style definitions unchanged, and undo
and redo restore the document and selection. Regressions cover the Greek
title/body case, legacy multiline blocks, pasted text, blank paragraphs,
selection direction, clipboard text, references, formatting summaries, undo/redo,
real multi-paragraph selections and collapsed carets.
