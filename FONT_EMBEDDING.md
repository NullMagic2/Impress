# Optional font embedding

The Save / Save As file dialog includes an **Embed fonts** checkbox for native
`.impress` and `.docx` files. The checkbox starts with the last successful save's
choice, remembered across restarts. It is disabled for Typst and RTF. Cancel
cancels the save without changing the preference. Saving an existing document
uses the remembered choice directly; use Save As to change it. There is no
separate font-options dialog.

The preference lives in `save-options.json` in Impress's existing data directory
(next to a writable portable executable, or in the user's data folder for a
system installation). Failure to write the preference is reported after saving.

Embedding uses complete OpenType faces, including the used regular/bold/italic/
bold-italic variants, inherited formatting, headers, footers, tables, text boxes,
notes, list markers and script fallback fonts. Shared payloads are stored once.
Fonts with restricted, preview/print-only, or bitmap-only embedding permission
cannot be embedded in an editable document; Impress reports the affected family
and lets the user save again without embedding. Equations continue to use the
application's bundled Typst fonts.

Native files carry payloads as resources with document-local family/style bindings.
The optional metadata is additive; documents without embedded fonts retain their
existing schema and package format. Font collections are extracted into complete
standalone faces. Opening a document makes its fonts available to its layout,
without installing fonts or changing another document's font catalogue.

DOCX writes `word/fontTable.xml`, its relationships, Word's `embedRegular`,
`embedBold`, `embedItalic` and `embedBoldItalic` elements, `.odttf` payloads,
content types and embedding settings. Payloads use ECMA-376's GUID/XOR font
obfuscation. Import reverses that operation and restores the family/style bindings.
Saving with embedding disabled removes the payloads and old embedding declarations.
Word's font table permits one face per family/style; the requested face takes
priority and script fallback faces are also included under their actual families.

Explicit saves collect fonts on the background worker from a frozen document
snapshot. Collection does not mutate the editor, undo history or recovery policy.

## Older LibreOffice versions on Windows

Some older Windows LibreOffice releases inspect embedded fonts using
`CreateScalableFontResource`, which supports TrueType outlines only. Valid
OpenType/CFF payloads can consequently appear by name while Windows substitutes
another face. Linux LibreOffice does not use that Windows API.

DOCX export now includes genuine legacy style-linked family names as well as
requested aliases. This lets a face such as Neo Aegean Compact Bold register
under its own Windows family without duplicating its embedded payload.

When a genuine TrueType release of an embedded CFF face is already available in
the font catalogue, or beside its installed font file with the same basename,
DOCX export prefers that release. Its name/version, character mapping, horizontal
and vertical metrics, glyph count, units and OpenType shaping tables must match,
and its editable embedding permissions are checked separately. Font outlines
are never converted, fixtures are never used as export fallbacks, and a missing
or mismatched counterpart leaves the original font intact. Native Impress files
continue to preserve their original font data.

The reported Colonization document contains Neo Aegean CFF fonts and renders
with those actual faces in isolated LibreOffice 26.2 on Linux. An isolated Wine
probe of the older Windows API rejects its OTF release and accepts the vendor's
TTF release. Wine is not a native Windows acceptance test; a final check on the
recipient's LibreOffice installation remains necessary, especially when its
version is unknown.

References: [LibreOffice 7.6 Windows loader](https://github.com/LibreOffice/core/blob/libreoffice-7.6.7.2/vcl/win/gdi/salfont.cxx)
and [Microsoft API limitations](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-createscalablefontresourcew).

## Validation

Regression tests cover native and DOCX round trips, separate font variants,
GUID byte order, full payload preservation, shared aliases, disabling embedding,
font permissions, TTC extraction/checksum, document isolation, layout with embedded
fonts in body text/headers/notes/parallel table cells, and preference persistence.

Commands:

```sh
cargo test -p typography -p docx -p format-native -p layout --offline
cargo check -p gui --offline
cargo test -p gui --bin Impress --offline
```

On the development host, GUI compilation needed
`BINDGEN_EXTRA_CLANG_ARGS='-isystem /usr/lib/gcc/x86_64-linux-gnu/15/include'`;
linking GUI tests needed temporary `libjpeg.so` and `libtiff.so` symlinks to the
installed versioned libraries via `LIBRARY_PATH`. These are host setup workarounds,
not changes to the shipped build configuration.

Microsoft Word itself was not available on this host. A final interoperability
check should open a saved DOCX in Word on a computer without the source fonts.

## Font families and searching

The font catalogue recognizes typographic family names, legacy style-linked
families, and WWS names. Exact Bold/Italic faces take priority over width fallback,
so a family such as Neo Aegean can use its Compact Bold face for Bold. Its
`Neo Aegean Compact` and `Neo Aegean Compact Light` families are selectable too.
Regression tests use all six actual OTF and TTF faces from Neo Aegean 0.9, including Greek
shaping. The fonts are test fixtures, not automatically installed or bundled into
the application executable.

The Ribbon font dropdown uses rounded corners and the surrounding Ribbon colors.
Its search field has no label and leaves clearance for all four border edges.
The family field follows the size selector's allocated height to align the widgets. Typing filters names without automatically selecting
a result; arrow keys select a result and Enter applies it. Enter also accepts an
exact family name. Escape closes the dropdown. Embedded fonts appear in the list.
Selection restores document focus after the popup closes so typing reaches the
page. Font identities also include their catalogue, preventing outline cache
collisions between documents. Tests verify visible Latin and Greek page pixels
for all twelve supplied font files.

## Justified text

Wrapped justified lines distribute their remaining width across interior spaces,
including lines requiring more expansion than the line-break optimizer's search
tolerance. The last line keeps natural spacing. A regression using Neo Aegean
and the reported Greek sentence checks five widths, margin alignment, last-line
spacing and complete source-byte preservation.

## Editing responsiveness

Double-clicking text selects its Unicode word. Greek accents and combining
characters remain intact. Native double-clicks are routed to the canvas on Linux
and the Windows input bridge. Object/header double-click actions remain available.

Backspace/Delete find neighboring grapheme boundaries near the caret. Timer
repeat deletions batch one paragraph mutation and undo/review/anchor update per
tick. A regression verifies four deletions in a long paragraph require one
revision and that undo restores all text.

Linux paints its persistent cairo frame directly, avoiding a second full-window
bitmap allocation. Previously rendered tiles stay visible while replacements
are pending, preventing blank-paper flashes when rasterization exhausts its
frame budget. Replacement tiles are still processed on subsequent frames; the
completed image is checked against a fresh render. Zoom changes clear old tiles.

## Saving and font decoding

Saving no longer opens an Export compatibility confirmation dialog. Font resources
are handled by the typography decoder rather than reported as unsupported images.
Regression tests export/import embedded Neo Aegean OTF and TTF through native
and DOCX files, check their original bytes, resolve a document-only family alias,
shape Greek without missing glyphs, and confirm visible glyph bounds.
