# Formatting after Enter

Enter carries the current character formatting into a new blank paragraph:
font family, size, character style, emphasis, language and other direct run
properties. The blank keeps a semantic empty text run for its paragraph mark,
so moving away/back, repeated Enter, Undo/Redo and reopening do not reset it.
Inherited paragraph/character style sizes remain linked to their style; they
are not copied into fixed direct font-size overrides.

Splitting inside existing text retains the paragraph's current style and
paragraph properties. At the end, an explicitly configured following paragraph
style still applies (for example Heading 1 -> Normal); the new paragraph then
uses that style's formatting instead of stale heading typing overrides.
Without a different following style, the current style and direct formatting
continue. The same split implementation is used in body text, tables,
headers/footers, text boxes and notes.

Empty-line layout and caret metrics use the retained size. Native files retain
the empty run; DOCX writes its properties as the standard paragraph mark
w:pPr/w:rPr and restores them on import. No visible placeholder character is
inserted into the document.
