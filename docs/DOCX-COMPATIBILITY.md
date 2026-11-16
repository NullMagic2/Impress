# DOCX compatibility

Impress reads and writes DOCX directly in Rust. Opening, editing and saving does not require Microsoft Office or LibreOffice. `docs/docx_progress.html` separates implemented capabilities from their verification; passing tests does not increase implementation percentages.

## Theme fonts

Theme major/minor collections resolve to actual font families for ASCII, high ANSI, East Asian and complex-script text. The importer reads the theme/settings relationship targets, supplemental script fonts, `themeFontLang`, regional run languages, font hints and RTL/complex-script selectors. Theme selectors take precedence over explicit names in the same property element. Direct properties and editable paragraph/character style inheritance remain sparse.

The model keeps resolved regional slots plus theme provenance. Layout splits shaping at font boundaries without splitting grapheme clusters, including cached field results. Font embedding collects the actual regional faces used. A manual font choice replaces inherited theme slots. DOCX export preserves theme relationships and updates edited style definitions while retaining unsupported style metadata. A missing or invalid theme produces an explicit compatibility diagnostic and uses available explicit/default fonts. Actual glyph availability still depends on installed or embedded fonts.

## Content controls and custom XML

Content controls retain typed properties and live UTF-8 ranges in body, table, header/footer and note content. Inline, block, row and cell wrappers are rebuilt during export. Plain text, rich text, list choices, combo boxes, check boxes, dates, pictures and groups retain their semantic type, aliases, tags and lock settings. Unknown control metadata is retained with a diagnostic for unsupported constraints.

Bindings resolve genuine XPath 1.0 expressions, including namespaces, predicates and attribute/text nodes, against the custom XML store identified by its datastore UUID. Editing a bound value updates the actual XML and its peer controls in one undo transaction. Initial stale cached text is refreshed from XML. Bindings must select exactly one value-bearing node. Invalid/missing stores, undeclared prefixes, ambiguous selections, oversized XML/XPath and excessive expression nesting return errors; external XML is not fetched.

Editor transactions enforce deletion/content locks, single-line text constraints, list membership and check box/date values. Failed validation restores document content, control metadata and XML without adding undo history. Right-click a control to change its value or remove an unlocked wrapper. Dates have a native calendar picker and preserve `fullDate`, display pattern, language and calendar metadata. Gregorian numeric and month/day-name display patterns are evaluated; other calendars retain a compatibility diagnostic.

Whole-paragraph inline rich controls can become block controls when Enter splits their paragraph. An inline control occupying only part of a paragraph requires a line break or its own paragraph; Impress rejects a split that would otherwise wrap unrelated prefix/suffix text. Temporary controls discard their wrapper after the first successful content edit, with undo support. Repeating-section behavior and custom XML schema validation are not implemented. Their metadata is retained and unsupported control constraints are reported. Picture bindings decode and validate bounded PNG, JPEG, GIF and BMP data before replacing the controlled image resource. XML-to-picture refresh, picture-to-XML edits and peer propagation preserve geometry, anchors and effects in the same undo transaction. Base OOXML rich-text bindings use scalar plain text; the later Word XML-subtree extension remains outside this scope.

Control wrappers, bindings and constraints are DOCX/native features. Keep a DOCX or Impress copy when exporting to a format without structured document tags.

## Pagination and revisions

Mixed inline page/column breaks remain semantic inline objects at their exact UTF-8 positions. Page breaks start a new page and column breaks advance the current column, including unequal-width columns. Standalone break paragraphs remain supported.

Word revisions keep individual authors, dates, original values and live anchors. Imported text insertions/deletions, linked moves, character and paragraph properties, paragraph-mark insertions/deletions, section properties, table/grid/row/cell properties, row/cell insertions/deletions and vertical-merge revisions have editable semantic snapshots. A deleted paragraph mark joins text in the editor; rejecting it restores the boundary. Tracked moves link their source and destination and resolve atomically, including moves within a paragraph and separate author/date metadata at each end.

Review → Word revisions offers navigation, acceptance and rejection. Rejection checks for conflicting newer edits, participates in undo/redo and preserves content-control anchors. DOCX export writes standard Word revision markup and before-properties. Annotation IDs are unique; a markup-compatibility-ignorable Impress group attribute keeps a formatting edit spanning multiple emitted runs together when reopened in Impress. Word does not require this extension to display or review the revisions.

Snapshots are bounded to 2,000 changes and 32 MiB of history, with a 4 MiB per-effect import limit; oversized histories are rejected explicitly. Native chronological transaction audit history remains a separate feature. Accepted/rejected native audit events, arbitrary unsupported Word properties and complete keystroke provenance are not reconstructed as Word revisions. Keep the native file for that audit trail.

## Picture crop shapes

Standard DrawingML picture preset geometry and adjustment values survive DOCX/native round trips. The renderer evaluates all 187 presets, and Picture tools exposes 176 filled crop choices with previews. Crop fractions, original image bytes and solid RGB outline colour/width are retained. Screen, PDF and print use the same clipped picture scene. Custom/freeform geometry, theme-based or dashed/compound outlines and 3D effects remain outside this implementation. See [PICTURE-CROPS.md](PICTURE-CROPS.md).

## Verification

Focused regressions cover regional shaping/inheritance/export, actual XML mutation and peer synchronization, atomic failed edits, typed check/date values, multiline controls, row/cell/block scope, SDT/revision/hyperlink nesting, malformed bindings and inline-to-block promotion. These checks validate behavior and are not implementation credit. Compatibility diagnostics remain available for features beyond the implemented model.

Release 0.8.6-43 passed all 183 workspace Rust tests, including 16 content-control, four theme-font and 12 tracked-change regressions. Both release converters passed combined DOCX → Impress → DOCX → Impress cycles checking theme resolution, bound XML, exact breaks, five revision authors, linked moves and unequal columns. Windows checks used isolated Wine. Linux execve tracing with an empty executable search path recorded only the converter process. Interface checks passed 64 tests with one skipped and two unchanged baseline source-assertion failures.
