# Architecture and source tree

Impress is a Cargo workspace. The three central crates have separate jobs: `core` defines the document data, `editor` changes that data and records undo, and `gui` connects windows and input to those operations. So a DOCX importer can build a document without opening a window, and an editor test can change one without wxDragon. Other crates import `core` as `model`, so it doesn't shadow Rust's built-in `core` crate.

```text
core (semantic data) <--- editor (commands and undo) <--- gui (wxDragon UI)
       ^                         ^                            |
       |                         |                            v
   format adapters          selection state             layout + output
```

Format adapters translate files, `typography` shapes text, `layout` positions content, `render` stores vector page commands, and `output` writes the final pages. These are successive stages, not alternative copies of the document. The whole DOCX/OOXML adapter, with its tests and fixtures, lives in `crates/docx/`. The independent legacy Word adapter lives in `crates/format-doc/`. It statically compiles the native Rust reader in `vendor/rwml/`, passes an in-memory DOCX representation to the existing semantic importer, and writes Word97 binary structures directly. No office executable is invoked.

Code style, naming and reuse rules are in [STYLE_GUIDE.md](STYLE_GUIDE.md).

## Headers and footers

A section owns up to three header streams and three footer streams: default, first page and even pages (`model::HeaderFooterSet` and `model::HeaderFooterVariant`).

1. **core**: `Section::header_footers()` iterates the existing streams. `HeaderFooterSet::for_page()` chooses the stream a page prints, falling back to the default stream as Word does.
2. **editor** (`header_footer.rs`): enter, switch, remove and leave streams, plus insert fields. Each change is one undo step.
3. **layout**: measures the tallest header and footer of each section (`HeaderFooterExtents`). A header or footer taller than its margin pushes the body text away instead of overlapping it. Pages carry a `HeaderFooter` hit region for each band.
4. **gui** (`header_footer_ui.rs`): while a stream is being edited, veils the body text and draws dashed band boundaries, in the variant's colour for the stream holding the caret. The *Header & Footer* tools, a Ribbon page shown in place of Insert like the Table and Picture tools, navigate between streams, insert the page number, page count or date, and remove or close a stream.

## Icons

Every Ribbon, dialog and canvas icon comes from the Yaru icon theme, in LibreOffice's word-processor command set. `tools/import_yaru_icons.py` maps each Impress icon name to a Yaru icon (`ICON_SOURCES`). It copies the sources into `design/icons/color/`, renders 64x64 PNGs into `crates/gui/assets/ribbon/` with every icon's ink scaled to the same size and centred, and regenerates `ribbon/embedded.rs`. The executable embeds the PNGs, and `ribbon/art.rs` scales them for each DPI. The licence notice is `crates/gui/assets/ribbon/YARU_ICONS.md`.

## Source hierarchy

The tree lists every file. The three icon folders hold one file per icon name, so they are summarized; `docs/ribbon-icons.png` shows every raster with its name. `target/` holds Cargo output and is never packaged.

```text
.
|-- .gitignore                       Ignored build output, editor backups, Python caches.
|-- ARCHITECTURE.md                  This source map, data flow and ownership rules.
|-- STYLE_GUIDE.md                   Readability and reuse rules for constants, types and modules.
|-- README.md                        Build instructions and current scope.
|-- HISTORY.md                       Change history and validation notes.
|-- LICENSE.md                       MIT licence for the Impress source code.
|-- SOURCE_MANIFEST.sha256           SHA-256 of every packaged source file.
|-- Cargo.toml                       Workspace members, shared dependencies, build profiles.
|-- Cargo.lock                       Locked dependency versions.
|-- build.bat / build.ps1            Windows release (or debug) build.
|-- check.bat / check.ps1            Workspace checks and Rust tests.
|-- clean.bat / clean.ps1            Remove build output.
|-- crates/                          Rust workspace members.
|   |-- core/                        Document schema; no GUI or editor dependency.
|   |   |-- Cargo.toml               Package manifest (imported as `model`).
|   |   `-- src/
|   |       |-- lib.rs               Public model exports.
|   |       |-- document.rs          Sections, header/footer streams and variants, blocks, paragraphs, text boxes, objects, resources, page numbering.
|   |       |-- style.rs             Style definitions and inherited properties.
|   |       |-- equation.rs          Editable equation nodes and structures.
|   |       |-- ids.rs               Stable node and resource identifiers.
|   |       |-- inline_split.rs      Splits paragraph content at a text offset (inline objects count as one marker).
|   |       |-- review.rs            Comments, revisions, footnotes and review records.
|   |       |-- revision_payload.rs  Shared history payload accounting.
|   |       |-- shared.rs            Copy-on-write document branch storage.
|   |       |-- symbol_fonts.rs      Maps Symbol/Wingdings private-use characters (U+F020-F0FF) to Unicode for bullets.
|   |       `-- text_diff.rs         Text-change representation and diff helpers.
|   |-- editor/                      Commands that transform core documents.
|   |   |-- Cargo.toml               Package manifest.
|   |   |-- src/
|   |   |   |-- lib.rs               EditorState, caret, selection, undo/redo, inline-object insertion, recursive paragraph lookup.
|   |   |   |-- header_footer.rs     Enter, switch, remove and leave header/footer streams; stream context.
|   |   |   |-- formatting.rs        Direct text and paragraph formatting, pictures, tables, sections.
|   |   |   |-- styles.rs            Create, apply, modify and remove styles.
|   |   |   |-- sections.rs          Section breaks and cross-section deletion.
|   |   |   |-- columns.rs           Word-style column presets and layouts applied to the document, a section or from the caret on.
|   |   |   |-- objects.rs           Inline objects, equations and object selection.
|   |   |   |-- floating_pictures.rs Anchored picture editing.
|   |   |   |-- picture_rotation.rs  Rotates a picture as a new image file, one undo step, from the in-session original.
|   |   |   |-- equation_input.rs    Equation insertion and editing commands.
|   |   |   |-- note_editing.rs      Footnote and endnote operations, next/previous note, reference lookup.
|   |   |   |-- contents_sync.rs     Contents/index synchronization after edits.
|   |   |   |-- search.rs            Find and replace over document content.
|   |   |   |-- review.rs            Comments, tracked changes, revision journal, bookmarks and references.
|   |   |   |-- anchor_maps.rs       Keeps reference anchors stable after text edits.
|   |   |   `-- clipboard.rs         Semantic clipboard: payloads, copy/paste, HTML, RTF and bitmap interchange.
|   |   `-- tests/
|   |       `-- search.rs            Search and replace.
|   |-- format/                      Common file-format interface and safe I/O.
|   |   |-- Cargo.toml               Package manifest.
|   |   `-- src/
|   |       |-- lib.rs               FormatRegistry and the DocumentFormat trait.
|   |       |-- atomic.rs            Atomic file replacement when saving.
|   |       `-- limits.rs            Import size and resource limits.
|   |-- format-native/               Native .impress ZIP reader and writer.
|   |   |-- Cargo.toml               Package manifest.
|   |   |-- src/lib.rs               Versioned package and semantic JSON I/O.
|   |   `-- tests/roundtrip.rs       Native save/load round trips.
|   |-- format-doc/                  Native Word 97–2003 reader/writer and standalone conversion CLI.
|   |-- docx/                        Self-contained DOCX/OOXML format crate.
|   |   |-- Cargo.toml               Package manifest and test dependencies.
|   |   |-- src/
|   |   |   |-- lib.rs               DocumentFormat adapter and public compatibility API.
|   |   |   |-- import.rs            DOCX XML to core nodes and styles, text boxes, OLE objects, picture effects.
|   |   |   |-- import_review.rs     Comments and revisions import.
|   |   |   |-- package.rs           Core nodes to DOCX XML and ZIP members, headers/footers, sections.
|   |   |   |-- package_review.rs    Review data export.
|   |   |   |-- compatibility.rs     Unsupported-feature reporting.
|   |   |   |-- ligatures.rs         OOXML ligature setting translation.
|   |   |   `-- omml.rs              Office Math equation translation.
|   |   `-- tests/
|   |       |-- roundtrip.rs            DOCX import/export round trips.
|   |       |-- equation_roundtrip.rs   OMML equation round trips.
|   |       `-- fixtures/
|   |           |-- README.md           What the fixture covers.
|   |           `-- mixed.docx          Mixed-content DOCX sample.
|   |-- format-rtf/                  RTF adapter.
|   |   |-- Cargo.toml               Package manifest.
|   |   |-- src/
|   |   |   |-- lib.rs               Adapter entry point.
|   |   |   |-- lexer.rs             RTF tokenization.
|   |   |   |-- reader.rs            RTF to core document.
|   |   |   `-- writer.rs            Core document to RTF.
|   |   `-- tests/roundtrip.rs       Unicode and escaping round trips.
|   |-- format-pdf/                  PDF conversion, as Word's: an editable document that keeps as much of the PDF as it can.
|   |   |-- Cargo.toml               Package manifest.
|   |   |-- src/
|   |   |   |-- lib.rs               Entry points (analyze, pieces, to_document), options and the PDF format.
|   |   |   |-- extract.rs           Styled text chunks (font, bold, italic, colour), pictures and web links per page.
|   |   |   |-- structure.rs         Running headers/footers, column bands and reading order, tables, lists, alignment, headings.
|   |   |   |-- build.rs             The document: styles from the PDF's fonts, runs, lists, tables, pictures, column sections, header/footer.
|   |   |   |-- drawings.rs          Vector charts and diagrams found among painted shapes and rendered as pictures.
|   |   |   |-- ocr.rs               OCR of scans: Tesseract when installed, else the embedded ocrs models; never strips accents.
|   |   |   |-- render.rs            Page rendering with hayro, in parallel, and PNG encoding.
|   |   |   `-- scan.rs              Scanned pages without a text layer rendered whole as page pictures.
|   |   |-- assets/ocr/              ocrs text detection and recognition models (CC BY-SA 4.0; OCR_MODELS.md).
|   |   `-- examples/pdf_preview.rs  Prints how a PDF converts (--stats, --document).
|   |-- format-typst/                Structured Typst interchange.
|   |   |-- Cargo.toml               Package manifest.
|   |   `-- src/
|   |       |-- lib.rs               Adapter entry point.
|   |       |-- structured.rs        Structured source import/export.
|   |       |-- text_export.rs       Styled text to Typst source.
|   |       `-- equation.rs          Equation source translation.
|   |-- typography/                  Font and line-composition primitives.
|   |   |-- Cargo.toml               Package manifest.
|   |   |-- src/
|   |   |   |-- lib.rs               Public shaping/composition API.
|   |   |   |-- fonts.rs             Font lookup and face selection.
|   |   |   |-- shaping.rs           OpenType glyph shaping.
|   |   |   |-- optical_kerning.rs   Optical pair adjustment.
|   |   |   |-- microtype.rs         Margin protrusion and expansion.
|   |   |   |-- bidi.rs              Bidirectional text runs.
|   |   |   |-- break_lines.rs       Line-break choices.
|   |   |   |-- hyphen.rs            Hyphenation support.
|   |   |   `-- render.rs            Positioned glyph-run data.
|   |   `-- examples/kerning_probe.rs  Prints shaped positions for a supplied font.
|   |-- typst-engine/                Typst compiler for equations and source views.
|   |   |-- Cargo.toml               Package manifest.
|   |   |-- src/lib.rs               Compile Typst and convert pages to vectors/SVG.
|   |   `-- tests/generated_source.rs  Runs generated source through the pinned engine.
|   |-- layout/                      Semantic document to paginated geometry.
|   |   |-- Cargo.toml               Package manifest.
|   |   |-- src/
|   |   |   |-- lib.rs               Page layout, paragraph/table preparation, header/footer extents and bands.
|   |   |   |-- scheduler.rs         Latest-revision worker coordination.
|   |   |   |-- preparation_cache.rs Reuse of unchanged prepared content.
|   |   |   |-- incremental.rs       Incremental-layout cache and invalidation.
|   |   |   |-- line_fitting.rs      Fitting shaped lines to widths.
|   |   |   |-- floating.rs          Anchored picture/text-box placement and text wrapping.
|   |   |   |-- object_scene.rs      Typed picture, text-box and embedded-object hit regions.
|   |   |   |-- references.rs        Footnotes, fields, headings and reference placement.
|   |   |   `-- bounds_tests.rs      Layout bounds regression tests.
|   |   `-- examples/
|   |       |-- bounds_bench.rs      Damage-bounds timing on the build machine.
|   |       |-- incremental_bench.rs Full versus incremental layout timing.
|   |       `-- typography_specimen.rs  Renders a typography specimen.
|   |-- render/                      Platform-independent vector page scene.
|   |   |-- Cargo.toml               Package manifest.
|   |   `-- src/
|   |       |-- lib.rs               Scene API and module exports.
|   |       |-- scene.rs             Draw commands, pages, hit regions, overlay chrome shapes (lines, dashes, icons).
|   |       |-- geometry.rs          Paths, transforms, rectangles, points.
|   |       |-- glyphs.rs            Glyph outline extraction.
|   |       |-- raster.rs            Image decoding, caching, colour conversion.
|   |       |-- color.rs             Scene colour values.
|   |       |-- viewport.rs          Page-to-viewport transforms.
|   |       |-- overlay.rs           Caret and selection overlay geometry.
|   |       |-- navigation.rs        Hit testing and page navigation.
|   |       `-- search_marks.rs      Search-highlight geometry.
|   |-- output/                      Final page export; not an editor model.
|   |   |-- Cargo.toml               Package manifest.
|   |   |-- src/
|   |   |   |-- lib.rs               Output API and page-range selection.
|   |   |   |-- pdf.rs               Vector PDF writing.
|   |   |   |-- raster.rs            Raster bands for printing/output.
|   |   |   `-- links.rs             Link annotation collection.
|   |   `-- examples/smoke_pdf.rs    Writes a sample PDF.
|   `-- gui/                         Desktop executable and platform integration.
|       |-- Cargo.toml               Package manifest (binary `impress`).
|       |-- build.rs                 Windows executable manifest and icon resources.
|       |-- assets/
|       |   |-- app_icon.rc          Windows resource script for the application icon.
|       |   |-- impress.ico          Application icon.
|       |   `-- ribbon/
|       |       |-- *.png            182 Yaru icon rasters (64x64), the logo and three style previews.
|       |       |-- manifest.json    Name and size of every raster (generated).
|       |       `-- YARU_ICONS.md    Origin and licence of the Yaru artwork.
|       |-- ui/
|       |   |-- main.xrc             Main window and Ribbon (generated).
|       |   |-- ribbon-manifest.json Button, size and group data behind main.xrc (generated).
|       |   |-- find_dialog.xrc      Find and Replace dialog.
|       |   |-- citation_dialog.xrc  Citation manager dialog.
|       |   |-- columns_dialog.xrc   Columns dialog (generated).
|       |   |-- layout_dialog.xrc    Picture Layout dialog (generated).
|       |   |-- style_editor.xrc     Modify Style dialog.
|       |   |-- format_*.xrc         Format dialogs: font, paragraph, tabs, borders, frame, language, numbering, shortcut, text effects.
|       |   |-- pdf_export.xrc / pdf_import.xrc / print_dialog.xrc  Output dialogs.
|       |   |-- recovery_dialog.xrc  Recover Documents dialog.
|       |   |-- note_dialog.xrc      Footnotes and Endnotes dialog (generated).
|       |   `-- table_position.xrc   Table Position dialog.
|       `-- src/
|           |-- main.rs              Startup, canvas state, wxDragon event loop and keyboard handling.
|           |-- commands.rs          GUI command identifiers dispatched to editor operations.
|           |-- document_defaults.rs New-document defaults.
|           |-- file_io.rs           Open/save orchestration via the format registry.
|           |-- background_io.rs     Asynchronous document file jobs.
|           |-- documents.rs         The documents open in a window: the one shown and the parked stack.
|           |-- title_bar.rs         Linux: the top strip as the window's title bar (client-side decorations).
|           |-- gtk_native.rs        Linux GTK calls: own title bar, true screen positions, mouse presses, title-bar colours from libadwaita.
|           |-- recovery.rs          Autosave and recovery sessions.
|           |-- clipboard_jobs.rs    Background clipboard conversions.
|           |-- system_clipboard.rs  OS clipboard read/write; semantic work is in editor.
|           |-- output_ui.rs         PDF/print jobs and their status.
|           |-- native_print.rs      Native print dialog and Windows spooler.
|           |-- native_input.rs      Windows IME bridge for the canvas.
|           |-- placement.rs         Centres dialogs on the Impress window.
|           |-- wx_canvas.rs         Document painting at physical resolution, cached page tiles, overlay and chrome drawing.
|           |-- display_scale.rs     Physical pixels per logical pixel (GTK HiDPI); used by the canvas and button faces.
|           |-- crisp.rs             Canvas for custom-drawn controls: shapes and icons at the screen's resolution.
|           |-- slot_ui.rs           An equation slot selected on the page (blue box) and typed into.
|           |-- header_footer_ui.rs  Header/footer editing chrome, variant colours, field buttons.
|           |-- table_ui.rs          On-page table editing and its chrome.
|           |-- objects_ui.rs        Picture/equation selection, resize/rotate/free-move drags and header/footer entry by double-click.
|           |-- picture_chrome.rs    On-page picture selection chrome (outline, 8 handles, rotate handle, delete ×), hit testing, rotation maths.
|           |-- footnote_ui.rs       Footnote reference tooltips and double-click jumps between reference and note.
|           |-- equation_ui.rs       Equation editing controls.
|           |-- equation_editor/
|           |   |-- mod.rs           Visual equation editor window.
|           |   |-- tree.rs          Structural equation editing model.
|           |   |-- layout.rs        Math box layout.
|           |   |-- slots.rs         Slots of an equation on the page: the one under a click, filling it.
|           |   `-- palette.rs       Palette contents and `\name` commands.
|           |-- wheel.rs             Mouse wheel and zoom input.
|           |-- spelling.rs          Spell-check integration.
|           |-- inline_spelling.rs   Inline spelling marks.
|           `-- ribbon/              Ribbon, Styles pane and dialogs.
|               |-- mod.rs           Ribbon command routing, context tools and status bar.
|               |-- generated.rs     Generated button specs and sizes (from build_ribbon_ui.py).
|               |-- embedded.rs      Generated icon dispatch table (from embed_ribbon_pngs.py).
|               |-- art.rs           Icon decoding, area-resampled DPI bundles, disabled tint, canvas icons.
|               |-- face.rs          Shared button face painting.
|               |-- skin.rs          Button faces with logical-size DPI bundles and state updates.
|               |-- bitmap_button.rs Native bitmap button setup.
|               |-- group_relief.rs  Ribbon group outlines and caption stripes.
|               |-- tab_strip.rs     Self-painted Ribbon tabs.
|               |-- dropdown.rs      Shared modeless dropdown placement and dismissal.
|               |-- font_preview.rs  Ribbon font field and system-font picker.
|               |-- style_gallery.rs In-Ribbon style gallery.
|               |-- gallery_popup.rs Expanded paragraph-style gallery.
|               |-- styles.rs        Modify Style dialog and docked Styles pane.
|               |-- format_dialogs.rs Format dialogs and draft values.
|               |-- find_dialog.rs   Find and Replace dialog controller.
|               |-- search_ui.rs     Search commands.
|               |-- review_ui.rs     Review commands (bookmarks, contents, tracking, spelling).
|               |-- comments_pane.rs Comments pane: threads as cards, writing and replying, page highlights.
|               |-- citation_ui.rs   Citation dialog controller.
|               |-- picture_bar.rs   Picture tools page (painted, like the Table tools): wrapping styles, position, size, arrange, close.
|               |-- picture_layout.rs Picture layout rules and pictures; Position gallery; Wrap Text menu of the on-page Layout Options button.
|               |-- bar_paint.rs     Drawing shared by the painted Table and Picture tools pages.
|               |-- doc_switcher.rs  Title-bar document switcher and its list of open documents.
|               |-- hyphenation.rs   Hyphenation menu: automatic hyphenation for the selection or the document.
|               |-- typst_highlight.rs  Syntax colours for the Typst source view.
|               |-- layout_dialog.rs Picture Layout dialog (Position, Text Wrapping, Size).
|               |-- table_bar.rs     Table bar shown above a table.
|               |-- table_picker.rs  Insert > Table size grid.
|               |-- symbol_picker.rs Insert > Symbol character grid (categories, preview, name, code point).
|               |-- inline_equation.rs In-place inline equation box: Typst source in the line, rendered on confirm.
|               |-- symbol_table.rs  Generated symbol categories and Unicode names (from build_symbol_table.py).
|               |-- table_position.rs Table Position dialog controller.
|               |-- column_art.rs    Page pictures of column layouts, shared by the Columns menu and dialog.
|               |-- columns_menu.rs  Layout > Columns menu: presets with pictures and More Columns.
|               |-- columns_dialog.rs Columns dialog controller: presets, widths, spacing, line between, apply to.
|               |-- output_dialogs.rs Export PDF, Import PDF and Print dialogs.
|               |-- recovery_dialog.rs Recover Documents dialog.
|               |-- note_dialog.rs   Footnotes and Endnotes dialog: note list, preview, actions, insert form.
|               `-- navigation.rs    Navigation pane controls.
|-- design/                          HTML visual references, layout and colour tokens.
|   |-- README.md                    How the design sources relate to the generators.
|   |-- interface-reference.html     Ribbon/interface reference with an inline Yaru sprite.
|   |-- interface-layout.json        Native Ribbon layout description.
|   |-- interface-tokens.json        Shared interface colours.
|   |-- format-dialogs.html          Generated XRC dialog preview.
|   `-- icons/
|       |-- color/*.svg, *.png       Yaru source for every icon name: SVG, or PNG for system-theme icons (generated).
|       |-- sprite.svg               All icons as `<symbol id="i-name">` (generated).
|       `-- dialog-launcher.svg      Group-caption dialog launcher glyph.
|-- packaging/
|   `-- linux/
|       |-- Impress.desktop.in       Launcher template; its name matches the app ID `Impress` so the dock finds the icon.
|       `-- install.sh               Installs or removes the launcher and the scalable logo icon (~/.local by default).
|-- docs/
|   |-- EQUATIONS.md                 Equation model and renderer notes.
|   |-- TYPOGRAPHY.md                Text shaping and layout notes.
|   `-- ribbon-icons.png             Labelled overview of every icon raster (generated).
|-- vendor/
|   |-- pdf-extract/                 pdf-extract 0.12.1 (MIT), patched to report fonts, colours and pictures; see IMPRESS_CHANGES.md.
|   `-- wxdragon-sys/                wxdragon-sys 0.9.22 (MIT OR Apache-2.0); its build fixes wxWidgets' Wayland pointer handling; see IMPRESS_CHANGES.md.
|-- test-support/                    Inputs shared by Rust tests.
|   `-- ligature_font.rs             Builds a tiny original OpenType face in memory.
|-- tools/                           Generators and development checks (Python 3 + Pillow).
|   |-- import_yaru_icons.py         Yaru icon import: SVGs, PNGs, sprite, overview, dispatch table.
|   |-- render_mock_icons/           Small resvg program that rasterizes SVG icons.
|   |   |-- Cargo.toml / Cargo.lock  Standalone manifest (not a workspace member).
|   |   `-- src/main.rs              SVG folder to 64x64 PNGs with a transparent inset and fitted, centred ink.
|   |-- embed_ribbon_pngs.py         PNG manifest and Rust dispatch generation.
|   |-- embed_style_dialog_icons.py  Embeds Styles-toolbar PNGs in the HTML reference.
|   |-- build_ribbon_ui.py           Ribbon XRC and Rust bindings generator.
|   |-- build_symbol_table.py        Insert > Symbol categories and names (ribbon/symbol_table.rs).
|   |-- adopt_mockup.py              Turns interface-layout.json into the visible native Ribbon.
|   |-- build_style_dialog.py        Modify Style XRC generator.
|   |-- build_format_dialogs.py      Format-dialog XRC generator.
|   |-- build_output_dialogs.py      Export/Import PDF, Print, Table Position and Recover Documents XRC generator.
|   |-- build_note_dialog.py         Footnotes and Endnotes XRC generator.
|   |-- build_columns_dialog.py      Columns XRC generator.
|   |-- build_layout_dialog.py       Picture Layout dialog XRC generator.
|   |-- build_citation_dialog.py     Citation XRC generator.
|   |-- dialog_kit.py                Shared dialog-generation helpers.
|   |-- xrc_preview.py               XRC-to-HTML dialog preview.
|   |-- xrc_sizer_checks.py          XRC sizer alignment rules.
|   |-- sync_clipboard_reference.py  HTML clipboard reference synchronization.
|   |-- ligature_fixture_oracle.py   Python transcription of the ligature test font.
|   |-- validate_ribbon.py           Ribbon resource and static checks.
|   `-- tests/                       Interface, asset, metrics and source-contract checks (`test_*.py`, `support.py`).
`-- target/                          Cargo build cache; never packaged.
```

Each crate's unit tests are in `crates/<crate>/tests/unit/`, mirroring `src/`, and are included by the source file they test; integration tests are the other files in `crates/<crate>/tests/`.

## Ownership rules

- `core` defines one persistent document representation. DOCX XML nodes, editor selections and layout page commands are temporary data for their own stages.
- `editor` depends on `core` and returns changed documents. It never creates wxDragon controls. `gui` reads OS events and calls those editor operations.
- GUI clipboard code talks to the operating system, and editor clipboard code translates semantic blocks. GUI file dialogs choose paths, and format crates parse and write bytes.
- wxDragon owns the GUI controls and event loop. Ribbon faces are rendered into wxDragon bitmaps and assigned to wxDragon buttons.
- `render` is the vector scene used by the GUI and by `output`. It is not a second document format. Overlay chrome (selection handles, header/footer bands) is drawn in view pixels on top of the cached page tiles and never enters the document.
