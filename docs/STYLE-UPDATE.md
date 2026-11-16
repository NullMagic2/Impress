# Updating styles from a selection

Right-click a style in the Home gallery, expanded gallery or Styles pane and
choose **Update [style] to Match Selection**. The command copies uniform font
and paragraph formatting from the document selection into that named style.
With a caret, it uses the current typing format and paragraph. Mixed properties
keep the style's existing value. Character styles copy only character formatting.

Redefinition updates all text using the style, including derived styles through
inheritance. It preserves the style's identity, name, base, following style and
other metadata. It does not reassign paragraphs to a different style or alter
text. Explicit local overrides outside the selection remain respected. Undo
restores the previous definition in one step.

## Font-size changes in Modify Style

An explicit size on pasted text can mask the style's font size. When saving a
Modify Style edit, properties changed by the definition are refreshed on the
selected sample using that style. For example, changing Heading 1 from 20 to
14 pt removes an old direct 12 pt size from the selected heading, so the
heading, toolbar and saved document resolve to 14 pt. Other properties, such as
italics or the proofing language, and direct formatting outside the selection
remain unchanged. A derived style with its own size remains independent.

The definition and selected formatting refresh form one undoable transaction.
Native and DOCX round-trip tests verify the resulting sizes and definitions.
