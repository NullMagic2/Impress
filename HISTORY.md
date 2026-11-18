## 0.8.6-68 — Opening documents with full-page pictures

- Open DOCX files containing pictures as tall as the printable area. If an element prevents normal pagination, the editor can show a simplified view while preserving the original document.
- Accept local file URIs on launch and show opening and pagination messages in the status bar.

## 0.8.6-67 — Table of contents menu

- Open References → Contents to insert or update the table, customize its title and heading levels, or toggle automatic updates. Manual refreshes respect that choice.

## 0.8.6-66 — Dialog layout

- Size column preset captions to the interface font and show units in the table headings. Update the scroll range when the column count changes.
- Give Find and Replace, Manage Styles, and the formatting dialogs room for their controls and keep gallery captions readable on smaller displays.

## 0.8.6-65 — Recent files keyboard focus

- Keep editing the document when Recent files settings opens. Click the settings window to edit the history limit.

## 0.8.6-64 — Native dialog actions and recent-file preferences

- Type the recent-file count beside an illustrated document pocket. Impress remembers the count when history is disabled and re-enabled.
- Use native dialog action buttons, consistent spacing, and separated footers across the editing, layout, and output dialogs. Review prompts support resizable lists and long text.

## 0.8.6-63 — Recent files dialog layout

- Lay the Recent files dialog out top to bottom: a “Remember recently opened files” checkbox with a left-aligned “Number of files” spinbox beneath it.
- Keep Cancel/OK centered in the platform's order, at natural width instead of oversized, with a wider gap above them and consistent margins.

## 0.8.6-62 — Dialog button alignment

- Center the Recent files dialog buttons, give them matching padded widths, and increase the gap between them.

## 0.8.6-61 — Recent files settings dialog

- Replace the text prompt with a native dialog containing one checkbox-and-spinbox row: “Remember up to [number] recently opened files.”
- Keep file-count limits, remembered settings and Cancel/Escape behavior, using native themed controls.

## 0.8.6-60 — Hyperlinks and clipboard interoperability

- Follow document hyperlinks with Ctrl+click, with a URL tooltip and a hand pointer while Ctrl is held. Ordinary clicks keep editing the text; internal bookmark links navigate within the document.
- Keep Cut, Copy and Paste available in the spelling context menu as well as the ordinary page menu.
- Exchange Unicode text, HTML, RTF and PNG images through the native GTK clipboard on Linux, preserving Impress formatting for internal transfers. Read other applications' clipboard data asynchronously and let clipboard managers retain copied content.
- Keep the Windows clipboard owner valid when the user changes applications while Copy is being prepared.
- Verify 61 GUI tests, 42 interface resource checks and native external-application clipboard and hyperlink workflows on Linux.

## 0.8.6-59 — Recent files and table margin snapping

- Add File → Recent with a remembered list of five successful opens or saves by default, a configurable count, and a clear-history command.
- Use the dedicated Yaru Recent Files icon in a full-height dropdown aligned with the File ribbon.
- Gently snap table moves, corner resizes and the outer column edge to the page margins shown by the rulers, with a live guide and matching saved geometry.
- Publish separate Mint 22.x, Ubuntu 24.04/26.04 LTS and Debian 13 packages with native dependency metadata, checksums and installation/startup evidence.
- Verify 194 Rust tests, 42 UI resource checks, native recent-file workflows and the saved geometry of snapped tables.

## 0.8.6-58 — Page-aligned rulers

- Align ruler zero and margin shading with the visible sheet using the same coordinates as the document canvas.
- Restart the vertical scale on each page, leave page gaps blank, and preserve alignment while scrolling, zooming and using display scaling.
- Keep paragraph and table indent handles independent of the page scale; hide handles for an offscreen focused page.

## 0.8.6-56–57 — DOCX tables and overset text

- Fix pagination of tall rows whose cells have different line origins, allowing the reported DOCX to open and render.
- Wrap long cached field values within table cells while preserving their semantic editing positions.
- Correct cell-edge clipping so glyphs cannot paint into neighboring cells.

## 0.8.6-55 — Paragraph formatting

- Preserve direct typing formatting when splitting paragraphs, including empty paragraph marks and DOCX round trips.
- Respect the configured following-paragraph style at the end of a styled paragraph.

## 0.8.6-53–54 — Clipboard and keyboard layouts

- Queue paste behind an in-progress copy or cut, with document and selection guards.
- Resolve command shortcuts across Greek and other non-Latin keyboard layouts while preserving Latin-layout mappings and AltGr text input.

## 0.8.6-50–52 — Proofing, selection and styles

- Correct Greek proofing-language resolution and avoid English fallback for a missing Greek dictionary.
- Normalize legacy multiline paragraphs and keep paragraph-style application within the selected paragraphs.
- Add Update Style to Match Selection to style context menus and correct style-size updates with direct-formatting overrides.

### Other work included in the current source

- Optional, remembered font embedding for native and DOCX saves, integrated into the save dialog.
- Font-family handling and Neo Aegean compatibility, with a searchable, themed font picker.
- Improvements to text justification, typing/deletion responsiveness and word selection.
- Native Word document conversion, table editing, picture controls and equation editing; see the feature-specific guides in `docs/` for details.

## 0.8.6-37 — Decorative headers and footers

- Add Simple rule, Accent rule, Double rule, Shaded band, and Framed designs alongside the six plain layouts. Open on Accent rule.
- Store rules, shading, frames and accent text as editable document formatting; put the separator above footers and below headers.
- Restore paragraph borders and shading in nested layout streams, including headers and footers, using the body renderer's shared decoration helper.
- Preview the actual template stream through the document layout and output renderer. Show only the header/footer strip, with the document's fonts and text width.
- Translate design names and preview errors into all five interface languages.

## 0.8.6-36 — Header and footer templates

- Open a native template gallery from Insert > Header or Footer, with a preview of only the selected header/footer strip and a compact Layout dropdown.
- Offer Blank, Centered, Three columns, Title and page number, Page X of Y, and Title and date.
- Insert editable paragraphs, width-aware tabs and live page-number/page-count fields; dates retain the existing fixed-date behavior.
- Select the initial placeholder for typing. Apply to the current section and preserve a selected first/even stream.
- Provide Edit header/footer for existing content and confirm before replacing it. Insertion is one undoable edit.
- Translate the gallery and placeholders into all five interface languages.

## 0.8.6-35 — Export progress and cancellation

- Remove Cancel from the File tab's Output group and the interface reference.
- Show a modeless export popup with completed-page progress and Cancel. Escape and closing the popup request cancellation; the window closes when the worker finishes.
- Keep temporary-file output and the final cancellation check so canceling does not replace an existing destination file.
- Update popup text when the interface language changes. Preserve Windows background-print cancellation in its own progress popup; Linux retains the native print dialog.

## 0.8.6-34 — Ruler, style and zoom controls

- Return typing focus to the document after using the ruler's tab-type selector, and distinguish Decimal tabs with a dot. Keep tab placement, dragging, removal and undo behavior.
- Paint icon-only Modify Style toolbar buttons without clipped tooltip text. Measure wrapped summary text so all lines remain visible. Preview the sample at its own font size and preserve bottom padding.
- Change a style's base without freezing the previous base's inherited font as an override. Preserve explicit formatting and disable the base dropdown when no compatible non-cyclic style exists.
- Use User in the dictionary Add dialog's Install for dropdown, translated in all interface languages. Keep Installed by User in the installation inventory.
- Enable View → Zoom with percentage entry, zoom in/out and 100%/One page/Page width presets. Share the viewport's 20–800% limits, disable limit buttons, and fit the current page.

## 0.8.6-33 — Interface languages

- Add English, Greek, Spanish, Brazilian Portuguese and European Portuguese interface catalogs, with a saved language preference and immediate switching.
- Put the flag menu on the right of the title toolbar, with a 20 DIP gap before the collapse arrow. Use bundled flag images on Linux and Windows.
- Translate ribbon captions, tooltips, menus, dialogs, status messages and built-in style labels while preserving document text, style identities, filenames and proofing-language tags.
- Rename the local dictionary installation location to Installed by User. Preserve existing installation receipts and removal checks.
- Recompute translated ribbon caption widths and retain stable enum values when translated choices are selected.

## 0.8.6-32 — Dictionary and style management

- Add Spelling → Manage dictionaries with Add and Delete. Show only dictionaries installed through Impress, with Language and Location columns.
- Keep installation records in the account data folder across application upgrades and reinstalls. Delete rechecks the matching installation before removing anything; existing system dictionaries are never adopted.
- Share the closable progress window for installation and removal. Use PolicyKit and constrained package removal on Ubuntu/Debian, and elevated Windows servicing for system dictionaries.
- Add Manage Styles… at the bottom of the Styles pane. Create paragraph or character styles, modify formatting and inheritance, delete unused styles, and configure document defaults through the existing style editor.

## 0.8.6-31 — Ruler marker cleanup

- Remove the extra horizontal dash from the decimal-tab marker while preserving its position and alignment behavior.

## 0.8.6-30 — Horizontal ruler alignment

- Lower the horizontal ruler's measurement scale within its existing widget.
- Anchor measurement ticks to the lower edge and place labels using their measured text height, keeping the scale aligned with tab markers at different display sizes.

## 0.8.6-29 — Rulers and paragraph tabs

- Add horizontal and vertical rulers throughout the editing area, aligned with page margins, zoom and scrolling. View → Ruler toggles both.
- Add ruler tab insertion, dragging and drag-off removal with one-step undo/redo. Remove the ribbon Tabs button and its action handler.
- Open exact tab settings from the ruler, with Left, Center, Right, Decimal and Bar alignment, all six leaders, Set/Clear/Clear All and document default spacing.
- Preserve selected-paragraph boundaries, mixed settings, style inheritance and native/DOCX persistence; export inherited tab clears and DOCX automatic tab spacing.
- Bar stops draw vertical rules on each line without consuming a Tab key press. Ctrl+Tab inserts tabs in tables and lists.
- Center the dictionary scope prompt and remove both scope explanation lines and the installation window closing hint.

## 0.8.6-28 — Dictionary installation

- Offer local or system-wide installation when a missing language is selected.
- Show asynchronous installation progress with a Close button and a command to reopen progress/results.
- Use authenticated APT `.deb` packages on Ubuntu/Debian; extract private dictionaries locally or elevate system APT through PolicyKit.
- Use PowerShell for checksum-pinned private dictionary downloads and elevated native DISM for Windows system dictionaries.
- Bundle Hunspell for private dictionaries, preserve upstream notices, and refresh spelling after successful installation.

## 0.8.6-27 — Spelling review dialog

- Replace sequential choice boxes and completion popups with one resizable native Spelling dialog.
- Show sentence context, the current word, its language, suggestions, and an editable replacement.
- Add Change, Change All, Ignore Once, Ignore All, and Add to Dictionary with selection and per-language boundaries.
- Keep progress and completion inside the dialog; support double-click suggestions, Close and Escape.
- Remove dictionary installation popups and command messages from the application workflow.
- Preserve the twenty named language choices, bottom-bar controls, and Linux/Windows providers.

# Build 0.8.6-26 — twenty proofing languages

The bottom Spelling → Language menu always lists twenty named choices on Linux and Windows, including Portuguese (Portugal), Portuguese (Brazil), and Greek. Missing dictionaries are marked and selecting one shows platform-specific installation instructions. Language changes still affect only the selected text or future typing. European Portuguese and Brazilian Portuguese require their own matching dictionary; neither is substituted for the other. Neutral Greek dictionaries are recognized for Greek text. The Linux package installs all twenty dictionary packages as dependencies. The status bar still contains only the Spelling button, with its toggle inside the menu.

# Build 0.8.6-25 — bottom spellchecker menu

Navigation and Zoom buttons now use the status-bar background instead of white rectangles. The Language control now lives in the bottom status bar as Spelling, with an installed-dictionary language picker, a check-as-you-type toggle with no language code or On/Off label beside it. Checking starts enabled and its default toggle is remembered. The language picker affects selected text or future typing; each text run is checked in its own language. Check selection reviews only selected text, while Check document explicitly reviews everything. F7 and Review → Spelling use the current selection when present. Linux uses Enchant 2 and installed dictionaries; Windows uses its native spelling provider. Red underlines use the existing source geometry overlay. Right-click suggestions replace the word in one undoable edit or add it to the personal dictionary. F7 and Review → Spelling open the correction session directly. Missing providers or dictionaries give installation guidance. Personal dictionary storage creates its parent directory on first use.

# History

## 0.8.6 — Equations in Typst, live equation editing

- Bumped all workspace packages to 0.8.6.
- The equation pane's Source field is Typst math: it shows the equation as `$x$` (inline) or `$ x $` (display) and accepts Typst typed into it (`a/b`, `sum_(k=1)^n k`, `alpha -> beta`, `mat(1, 2; 3, 4)`, `binom(n, k)`, …). It is parsed with Typst's own parser (`format_typst::parse_math_source`); the former linear syntax is gone. Code (`#…`) and calls of non-math functions are refused.
- Typing `$…$` in the text turns valid Typst math into an equation when the closing `$` is typed (`$ x $` with inner spaces gives a display equation). "$5 and $10" and `\$` stay text; Ctrl+Z gives the source back.
- `/` typed in the Source field makes a stacked fraction, so fractions nest one above the other; the generated Typst only uses the parentheses Typst needs (`a / b`, `x^2`, `(x + 1) / 2`).
- The equation pane edits the equation in the document live: the Insert/Update and Cancel buttons and the subtitle are gone, and a × (or Escape) closes the pane, as in the Styles pane. Each change is committed once Typst has laid it out, so the document keeps the last equation that renders; edits while the pane is open are one undo step. An open pane no longer blocks closing the window, opening or switching documents.
- Closing the window ends Impress again. The main frame was never the app's top window, so wxWidgets kept running while the hidden dialogs loaded at startup existed: the window vanished but the process stayed.
- Fractions in inline equations read at text size, as LaTeX's `\dfrac`: the outermost fraction's numerator and denominator are full size instead of Typst's inline 70% (50% for a fraction within it, now 70%). Fractions in exponents and subscripts stay small, and the Typst source is unchanged.
- Tests moved out of the source files: each crate's unit tests are in `crates/<crate>/tests/unit/` (mirroring `src/`, still with access to private items), and the Python checks in `tools/tests/`. The style guide states the rule. All 534 Rust tests and the Python checks give the same results as before.
- The page follows the equation pane at every keystroke: unfinished slots show as □ placeholders and a `\name` being typed as its text, instead of the page waiting for a finished equation (and showing a stale one meanwhile). Each change is laid out once: the preview's layout is kept when it is committed, and the pause before layout is 60 ms.
- Selecting with the mouse past the end of a paragraph's short last line selects that line. The nearest text was chosen by straight-line distance, so the long line above won; now the line at the pointer's height is chosen first (`render/src/viewport.rs`, `nearest_region`).
- Recovery copies and the spelling dictionary go to the user's data folder (`~/.local/share/impress`, `%LOCALAPPDATA%\Impress`) when Impress cannot write next to its executable, as in a system install; a portable copy keeps them next to the executable as before.
- Linux release as a Debian package for Ubuntu 26.04 (`impress_0.8.6-1_amd64.deb`): Impress in `/usr/lib/impress`, an `impress` command, the launcher and the icon.
- Smoother on HiDPI screens (measured at 200 %: a Ribbon tab switch shows the new page in about 11 ms instead of 85–95 ms). A caret blink repaints only the caret, not the whole page view; a repaint where only the caret, selection or handles changed restores just that area of the cached page image instead of copying all of it; wxWidgets no longer sends an update-UI event to every window after every event (Impress uses none; `wxd_UpdateUIEvent_SetMode` in `vendor/wxdragon-sys`); the document switcher, tab strip and group captions measure their text once per scale instead of on every tick or paint, and the tab strip redraws its background only when the active or hovered tab changes.
- The test suite is 100 tests (from 539): old, duplicate and release-named ("alpha…", "buildfix…") tests were removed, the kept ones live in files named after their feature, and fixtures used only by removed tests are gone. The style guide now asks to propose a test, with the reason, instead of adding one.
- The Python source and resource checks in `tools/tests` pass again (they had failed since before 0.8.5, 65 failures and 2 errors). None found a fault in Impress: they still described replaced designs (the Win32 button painter, XRC tab labels and divider panels, 22-pixel group captions, the style buttons as gallery items) and pinned totals of buttons, icons and sizers. They now check the current design and compare totals between the generators' outputs instead. The fontTools-based ligature oracle is skipped when fontTools is not installed, as it already was without HarfBuzz.
- On Wayland, Impress no longer vanishes when a pointer device goes away (a mouse unplugged, a remote or virtual input device stopping): wxWidgets 3.3.2 bound `wl_seat` at version 2 but freed the pointer with `wl_pointer_release` (version 3), and GNOME ended the connection over the invalid request. `vendor/wxdragon-sys` patches the downloaded wxWidgets source before building it.
- Maximizing and restoring the window no longer stalls (about 0.3 s before). Every Ribbon button asked for the screen's scale factor on each resize through a new device context, and creating one on a GTK control looks up the theme's colours; the factor now comes from GTK directly (`gtk_native::scale_factor`). On Linux the page view is kept in one cairo surface that takes only the areas that changed, instead of a wxBitmap whose whole image GTK converted on every paint, and the containers the Ribbon and page view cover no longer fill their whole area before their children paint over it. At 200 % a maximize or restore now takes one frame of about 40–50 ms, most of it GTK's own work for a 3200 × 2000 window.
- Insert > Symbol opens a character grid below the button, laid out like the Windows Character Map, instead of a list of 19 symbols: eight categories (Common, Latin, Greek, Math, Arrows, Currency, Punctuation, Shapes; 965 characters), the character under the pointer or chosen with the arrow keys shown large with its Unicode name and code point, the wheel and Page Up/Down to scroll, a click or Enter to insert. The names come from a table generated by `tools/build_symbol_table.py` (`ribbon/symbol_table.rs`), so Impress carries no Unicode name database.
- Equation and Inline are different commands, both inserting at the caret. Every equation button opened the equation pane for an inline equation, including the one whose tip promised a display equation. Insert > Equation (Ctrl+Shift+M) opens the equation pane; Insert > Inline (Ctrl+M) opens a compact box in the text line at the caret, where the equation is typed as Typst math; it is rendered when the box is confirmed (Enter, Tab, its ✓, or a click anywhere outside it), and Escape leaves the document as it was. Clicking an inline equation opens the same box over it with its source; emptying it removes the equation, and the box's equation button opens the full editor (`ribbon/inline_equation.rs`; dropdowns can now commit when left, `dropdown::on_leave`). The two shortcuts the tips named now work. A new-equation command always makes a new equation: with the caret just after one, it used to open that one and add to it. Equations are on the Insert tab only (Home's Equation group is gone). A display equation, on its own centred line, is made by typing `$ x $` with spaces inside (`EditorState::insert_display_equation`).
- An untitled document holding only an unfinished equation (its boxes never filled in) is blank again for document recovery, which no longer offers to restore it. An equation counts as content only for something besides empty slots (`EquationNode::has_content`), and a recovery copy's stored "has content" is checked again on start, so copies left by earlier builds are cleared too.
- Equations are filled in on the page. A click on a slot of an equation (an empty box, a numerator, an exponent…) selects it with a blue box, as a selected table cell is shown; typing replaces what it holds and the equation follows each key (typed as in the equation pane, so `1/2` makes a fraction), Backspace erases, Enter, a click elsewhere or a caret key finishes, Tab finishes and selects the next empty box, and Escape puts the slot back. The box is fitted to the glyphs Typst drew for the slot. While the equation pane is open, the slot its cursor is in has the same box on the page, so a click on a box there shows it selected too. One undo step per slot; the equation pane does not open (`slot_ui.rs`; the slot is found by laying the formula out as the pane does, `equation_editor/slots.rs`). With the pane open, a click on the equation it edits puts the pane's cursor in the slot clicked instead. Boxes kept in the document as placeholder symbols open as empty slots again, not as literal □ characters.
- Empty slots in the equation editor are easier to click: a click near the dashed square, not only on it, puts the cursor in it. A script, limit or matrix cell is drawn small, and a click just beside its square went to the formula around it.
- The Typst source pane (Typst and Split views) closes with the panes' red cross at the right of its header, which returns to the Graphical view. The cross takes its pane's colour.
- Comments have a pane, as in Word, in place of a chain of choice and text dialogs. Review > Comment opens it to write a comment on the selection (the word at the caret if nothing is selected); the threads show as cards in document order with author, a relative time ("5 minutes ago"), text and replies, resolved ones dimmed; the commented text is highlighted on the page, the current comment's more strongly. A selected card selects its text and offers Reply, Edit, Resolve or Reopen, and Delete; the field at the bottom posts with Ctrl+Enter or Post and cancels with Escape. Review > Delete, Resolve, Previous and Next work on the current comment (the one selected, or the caret's) and are enabled only when they apply; moving the caret into commented text selects its card. The author is the document's review author, else the account's full name. The small thread previews drawn inside the page's right margin are gone (`ribbon/comments_pane.rs`).
- Hyphenation is a menu on its Ribbon button, as Typography is (same width and caret, beside it): for the selected paragraphs (the caret's setting checked) or the whole document, Hyphenate automatically or Don't hyphenate, applied when chosen. The Hyphenation dialog is gone (`ribbon/hyphenation.rs`).
- Dropdowns share one frame and shading (`ribbon/dropdown.rs`): a blue-grey border around every dropdown (Symbol, Table, the expanded style gallery, fonts, documents, Columns, Wrap Text, Position, the right-click menu), and the same light band behind their headers and footers. Most had no edge and blended into the white page and panes below them; the three that drew their own border used three different colours.
- Panes close with the same red cross, slightly larger (20 DIP instead of 16): the Styles pane, the Equation pane (which had a grey ×) and the expanded style gallery (a text ×). `ribbon::close_mark` draws it everywhere.
- Custom-drawn controls paint at the screen's resolution (`crisp.rs`): the Equation pane, the Table and Picture tools pages, the tab strip, group dialog launchers, the context menu, the table picker, the Columns menu and dialog, the picture Wrap and Position menus, the Picture Layout and Table Position dialogs and the Recover Documents list. At 200 % they drew a logical-size image that was stretched, so their icons and edges were blurred next to the Ribbon's.
- A click just past the end of a nested part of an equation (under a root, in an exponent, a numerator) puts the cursor at that part's end, so typing goes on inside it: a click right after "27" in √(3+27) used to land after the root, and an 8 typed there made √(3+27)8 instead of √(3+278).
- The page always catches up with the equation pane: a live preview whose layout was replaced by another one (typing in the document meanwhile) was never committed, so the page could keep the equation one keystroke behind the pane. A lost preview is now laid out again, and an idle pane brings the document up to date. The blue box of a slot is fitted with a margin from its height only, so a wide numerator's box no longer takes in the denominator.
- A square root's bar is no longer cut off inside a fraction (or wherever it rose above the line): Typst sized each equation from the text's cap height, and the bar of a radical in a numerator lay above that, outside what was drawn. Equations are now sized by their ink (`top-edge`/`bottom-edge: "bounds"`).
- Clicking elsewhere on the page clears the blue box of an equation slot, also while the equation pane is open (its box shows only while the pane is in use).
- Inline equations sit on the text's baseline. Each equation is typeset as a small Typst page, and a page frame has no baseline of its own: Typst reports its bottom edge, so every inline equation was raised by its depth plus the page margin, plainly so beside a fraction (the `+ 5` after ¾ sat low against it). The baseline now comes from the equation's line inside the page (`typst-engine`, `content_baseline`).
- Buttons no longer animate their hover state (`button { transition: none; }` in Impress's GTK style). The theme faded it over 200 ms, so moving or resizing the window under the pointer, or just hovering a Ribbon button, repainted the Ribbon on every frame for that long: about 11 repaints per maximize, now one.
- Ribbon pages prepared in the background after start-up pause the window for 22–40 ms each instead of 45–95 ms. Their button faces are drawn once, after the page is laid out: drawn before, layout trimmed most small buttons by a pixel or two and every face was drawn a second time. The face rasterizer works out its rounded outline only at the corners (pixel-identical output, 2–3 times faster), and the per-pixel colour conversions are written so the size-optimized build vectorizes them (the page view's conversion to cairo's format is about 6 times faster). tiny-skia, which fills and composes every page frame, is built for speed.
- The generated Typst source is indented with tabs, shown 4 characters wide in the Typst view.
- The main bar of a fraction over or under another fraction is clearly longer than the inner bars, in the equation editor and on the page.

## 0.8.5 — Several documents per window, Impress's own title bar

- Bumped all workspace packages to 0.8.5. The changes since 0.8.3 are listed under 0.8.4 below.
- Split view toggles: pressing Split again returns to the document alone. The document is on the left and its Typst source on the right.
- A hairline under the Ribbon separates it from the workspace, so its white no longer runs into the page.
- The Typst source is readable code, as a person would write it: the document's defaults are set once at the top (from what most paragraphs look like), each paragraph and table states only how it differs, text is escaped Typst markup with `*bold*` and `_italic_`, blocks and table cells are indented, and table rows sit on one line. Files are about a third of their former size and compile with Typst as before.
- The Typst source view is syntax-highlighted in the system's monospaced font: comments, strings, numbers, keywords, functions, argument names and constants each have their colour, while the document's own text stays plain. Long sources colour in batches, so the window never stalls.
- In Split view the handle between the document and its source can be dragged, like the Styles pane's; the split keeps its proportions when the window is resized.
- `packaging/linux/install.sh` copies the binary to one fixed place, `~/.local/lib/impress/Impress`, and the launcher runs that copy. Installing a new version no longer changes the launcher's path, so apps that keep launchers in memory (the ChatGPT desktop app's "Open with") still start the version installed last.
- A table row that breaks across pages is closed on both pages, as in Word: the first part keeps the cells' bottom padding and borders at the foot of the page, and the rest starts with their top padding and borders on the next. Before, the table was left open at the break and its text touched the page edge of the cell.
- On Wayland, the window no longer shows GTK's own title bar above Impress's, nor a see-through margin: the empty title bar that replaces GTK's is now shown with no height instead of hidden, which GTK on Wayland ignored.
- Selecting text in a table row that breaks across pages highlights only what is selected. A cell paragraph that starts on the next page was ranked after the next cell's text on the first page, so a selection ending in it also lit up that cell (the Glossary next to a Translation). Layout now records each paragraph's place in the document (`VectorDocument::text_order`), and selection highlights follow it instead of page order.
- A mouse selection that starts in a table cell broken across pages runs on into the cell's part on the next page, so the whole cell can be selected; with the pointer in the page margin or between pages it stays in that cell's text (`ViewportState::hit_test_in_cell`). Moving into another cell still selects whole cells, as in Word.
- Dragging a selection past the top or bottom of the page view scrolls the view, faster the further out the pointer is, and the selection follows.
- Shift+click extends the selection from where it began to the click, as in Word and Windows text boxes: another Shift+click moves the end again (shrinking or reversing the selection), and holding the button after Shift+click goes on extending. From one table cell into another of the same table, Shift+click selects the rectangle of cells; in the same cell, or outside the table, it extends the text. wxDragon cannot report modifiers on mouse events (its ShiftDown works on key events only), so Shift is read from GTK's button event.
- The same limitation broke Ctrl+wheel zoom and Shift+wheel sideways scrolling on Linux; the wheel now reads Ctrl and Shift from GTK's scroll event.
- Right-clicking the page opens a menu with Cut, Copy, Paste and Select All, with their shortcuts, drawn inside the window like the Ribbon's dropdowns (`ribbon/context_menu.rs`). Cut and Copy are greyed out without a selection. A right-click outside the selection first moves the caret there; inside the selection or on selected cells, the selection is kept. Arrow keys, Enter and the letters T, C, P and A work in the menu.
- Documents dragged onto the window open in it, each added to the window's list (one already open is shown instead); several dropped together open one after another. Only the drop is handled: wxDragon passes enter/over results to wxWidgets with mismatched numbers (its "Copy" arrives as "None"), which refused every drag.

## 0.8.4 — PDF conversion: OCR, drawings and columns

- Bumped all workspace packages to 0.8.4.
- The previews in Recover and Import PDF are miniatures of the document as laid out, like the Print and Export PDF previews, instead of its text in a plain font; all four are drawn at the screen's resolution.
- The equation editor stays open while the document is edited, and after Insert or Apply. Clicking, typing and the whole Ribbon work as usual; while the document is being edited it is shown without the draft (so the caret stays true), and changing the equation shows the draft on the page again. Insert puts a new equation at the caret, wherever it now is, and the editor stays on it, so further changes update it; an existing equation is updated wherever it now is. Opening another equation keeps the current one's changes first. Saving and printing work with the editor open; New, Open, Recover and Import PDF wait until the equation is applied or cancelled.
- Opening a document always adds it to the window's list; a blank Untitled document is no longer replaced. Only the blank document Impress starts with gives way to a file named on the command line (opened from the file manager).
- The title bar takes the colours of GNOME's own title bars, read at run time from the stylesheets built into the installed libadwaita (the library GNOME's apps draw with) for the current light or dark style and accent: background and text, and the quieter shade while Impress is in the background. It follows a change of style, accent or libadwaita without a restart; without libadwaita it takes the GTK theme's header bar. On a dark title bar the quick-access icons are drawn as light glyphs.
- Dropdowns (style gallery, columns, Insert Table, wrap and position menus, fonts, the document list) are drawn inside the window instead of as separate windows. On Wayland a separate window cannot be placed, so GNOME centred them over the window; now they open exactly below their button, without the desktop's drop shadow, and close on Escape, on a press outside them or when another window is activated (`gui/src/gtk_native.rs`).
- On Linux, Impress draws its own title bar, as GNOME apps and Word do: the strip with the logo, quick-access buttons and document switcher replaces the desktop's title bar, so the switcher sits in the window's title bar. GTK still draws the window's shadow and resize edges. Dragging the strip moves the window, double-clicking it maximizes or restores, and it ends with minimize, maximize and close buttons. Windows keeps its native title bar. New file: `gui/src/title_bar.rs`.
- Layout > Hyphenation opens a dialog like Word's instead of a list to pick from: Automatically hyphenate, for the selected paragraphs or the whole document, as one undo step (`ribbon/hyphenation_dialog.rs`, `tools/build_hyphenation_dialog.py`). (0.8.6: replaced by the Hyphenation menu, below.)
- Hyphenation is on by default for Word files too, as for new documents; paragraphs a Word file marks as not hyphenated keep their words whole.
- Several documents per window. The document name, centred in the title bar, is the switcher: the name of the document shown, a dot for unsaved changes and a small arrow. Clicking it lists the window's documents in the order they were opened, with their folders; a row shows its document and × closes it. Open, New, Import PDF and Recover add a document to the list instead of replacing the one shown, so they no longer ask to save first; opening a file that is already open shows it. Each document keeps its own undo, file, recovery snapshot, zoom and scroll. Alt+1…9 shows a document and Ctrl+W closes it, asking first when it has unsaved changes; closing the window asks about each unsaved document in turn. Launching Impress again still opens a separate window. New files: `gui/src/documents.rs`, `gui/src/ribbon/doc_switcher.rs`.
- Styles show their real colours. The style gallery painted every bold style blue and the others grey whatever their colour, so a style looked blue in the gallery and came out black. The inline and expanded galleries now paint each style in its own colour. The gallery's scroll arrows and frame are drawn at the screen's resolution (they were stretched and blurry on HiDPI screens). The built-in headings are coloured in the blue the gallery always showed (#336EC1), so they look the same in the gallery and the document, and new documents also have Heading 2, Heading 3, Subtitle and Quote; a Word file's own styles of those names replace them.
- Yes/no questions are real message boxes instead of a list to pick from: replacing an existing file in Save As (Replace / Cancel) and exporting with known limitations (Export anyway / Cancel). The safe button is the default, so Enter never discards or overwrites.
- PDF conversion reads scanned pages (OCR). Impress uses Tesseract when it is installed, with every installed language, so accented text keeps its accents; pages are read in parallel. Without Tesseract it uses the ocrs models built into Impress. ocrs, like a Tesseract with English alone, knows no accented letters, so its text is kept only when the document reads as English: a scan in Portuguese or another accented language stays a picture, and the status bar says which Tesseract language to install (for Portuguese, `tesseract-ocr-por`). Accents are never stripped. OCR and page rendering are compiled for speed rather than size (`opt-level = 3` for the ocrs, rten, hayro and vello crates), which reads a page about twice as fast; a dense page takes about 1.5 s. File > Import PDF previews scans as pictures and reads them only when you click Import, so the dialog opens at once.
- PDF conversion keeps vector drawings. Charts, diagrams and logos drawn with shapes rather than embedded images become pictures, with the labels written over them; table rules, cell shading and frames around text stay as they are. Drawings are rendered in parallel, so a 662-page report full of charts converts in about 9 seconds.
- Page columns are recognised only around running prose, so code listings, number tables and hex dumps are no longer taken for columns.
- PDF conversion keeps page columns as real column sections: each run of columns becomes a continuous section with the PDF's number of columns and gap, and a column the PDF ended early keeps a column break. File > Import PDF has a "Keep page columns" option.
- Layout: the last page of a section in columns is balanced, as in Word, so columns between full-width text end level instead of the first column filling to the bottom. Text after a section in columns starts below its lowest column; it could start beside the end of the last column, over the first column's text.
- Pictures are selected like tables. A selected picture shows a blue outline with square handles: corners resize in proportion (Shift unlocks), the squares on the top, bottom, left and right edges stretch one way. A round arrow above it rotates it (Shift snaps to 15°), and a × beside it deletes it. Drag a picture to move it anywhere on the page; it floats where it is dropped with text wrapping around it, as one undo step (Shift-drag moves it within the text). The Picture tools (Align, Wrap Text, Delete, Close) replace the Insert tab while a picture is selected; the red Close button or Esc deselects it and returns to the tab shown before, as leaving a header or footer now does too. Rotation is stored by saving the turned picture as a new image (each turn in a session starts from the original, so it does not blur). New files: `gui/src/picture_chrome.rs`, `editor/src/picture_rotation.rs`.
- Picture tools take the Ribbon like the Table tools: one painted page in place of Insert while a picture is selected, with Wrap Text (six large buttons, the current style highlighted), Position (gallery, Move with Text, Fix Position on Page, More Options…), Size (width and height steppers in millimetres, Lock aspect ratio), Arrange (align, Rotate ▾, Delete) and a red Close Picture Tools. It shares its drawing with the Table tools (`ribbon/bar_paint.rs`, `ribbon/picture_bar.rs`). Its buttons use Yaru icons like the rest of the Ribbon (wrapping styles, Position, Move with Text, Fix Position on Page, More Options, alignment, Rotate, Lock aspect ratio, Delete); only the steppers and the red Close are drawn.
- Picture layout works like Word's, replacing the list dialog. Wrap Text offers each style with its picture: In Line with Text, Square, Tight, Top and Bottom (new for pictures, in the model, layout and DOCX), Behind Text and In Front of Text, then Move with Text / Fix Position on Page and More Layout Options…. The new Position button opens a gallery: in line with the text, or one of nine places on the page with text wrapping. A Layout Options button beside a selected picture opens the same menu there. More Layout Options… opens a Layout dialog with Word's tabs: Position (alignment or absolute position, from the page, margin or paragraph; move with text; lock anchor), Text Wrapping (style, distance from text) and Size (height, width, lock aspect ratio); OK is one undo step. A picture that starts to float stays where it was, anchored to the paragraph it starts beside (anchoring to the paragraph below made layout unable to settle). Double-clicking a picture selects it, as in Word, instead of opening a dialog. As in Word, a click on text lying over a Behind Text picture places the cursor in the text; the picture is picked where no text covers it.
- Tabbed dialogs are no longer as tall as all their tabs together: only the first tab starts visible, so the window is fitted to the largest tab instead of the stacked pages.
- Fixed a crash of the layout worker ("RefCell already borrowed") with tables of four or more rows: their rows are prepared in parallel, and while waiting the thread could be given another block to prepare, which borrowed the preparation cache a second time. The cache no longer holds its borrow while preparing.
- Smooth scrolling: a wheel notch glides over a few frames instead of jumping the page, and quick notches add up.
- PDF reading: fill-and-stroke and even-odd fill operators now end their path (the vendored pdf-extract ignored them and merged later shapes into it).

## 0.8.3 — Yaru icons, header and footer tools

- Bumped all workspace packages to 0.8.3.
- Every Ribbon, dialog and canvas icon is now from the Yaru icon theme, in its LibreOffice word-processor command set: 170 icons plus 12 legacy aliases. `tools/import_yaru_icons.py` maps each Impress icon name to a Yaru command icon. It writes the SVG sources (`design/icons/color`, a grayscale copy in `design/icons/mono`, the sprite, and the inline sprite of the interface reference), the 64x64 PNGs with a 1-pixel transparent border, `ribbon/embedded.rs` and `docs/ribbon-icons.png`. The quick-access and clipboard commands (New, Open, Save, Print, Undo, Redo, Cut, Copy, Paste, Find, Margins) use the system Yaru theme's full-colour icons instead, which are drawn for small sizes. Open is the orange Yaru folder and Save the Yaru floppy disk recoloured blue; Copy is blue, Cut red and the Paste clipboards brown (`ICON_TINTS` recolours an icon's grey body); Save as is that floppy with LibreOffice Yaru's pencil pasted into its corner (`COMPOSED_ICONS`). Every icon's visible drawing is scaled to the same size (54 of 64 px) and centred, because Yaru drawings pad themselves differently and a row of them looked uneven; vector icons are re-rendered at the fitted size so they stay sharp. The hand-drawn icon generators `draw_ribbon_icons.py` and `draw_style_toolbar_icons.py` are removed. The artwork is CC BY-SA 4.0 (see `crates/gui/assets/ribbon/YARU_ICONS.md`); the brand logo and the style previews are unchanged.
- Layout engine: a header or footer taller than its margin now pushes the body text away (with a 6 pt gap) instead of overlapping it. Each section measures its tallest header and footer (`HeaderFooterExtents`), and the body always keeps at least 72 pt. The incremental layout cache includes these heights, so editing a header lays out the body again.
- Header and footer editing:
  - While a header or footer is being edited, the body text is veiled. Every header and footer band of the section gets a dashed boundary and a tab with its Yaru icon. The stream being edited is coloured orange (default), aubergine (first page) or blue (even pages), so you can see which pages it affects. The status bar names it, for example "First page header · Esc to return".
  - The Header & footer tools bar gains Header/Footer (move between the two bands), Default/First page/Even pages with distinct icons, Page no., Pages and Date fields, Remove (deletes the stream; removing the last first-page or even-page stream turns that variant off), and Close.
  - Insert > Header and Insert > Footer open the stream directly, without the old list dialog.
  - Insert > Date & time now inserts today's date (ISO 8601) at the caret.
- Model and editor: `HeaderFooterVariant` moved to `core` (the editor re-exports it), with `index`, `from_index`, `for_page` and `label`. `HeaderFooterSet` gains `get`, `slot_mut`, `for_page`, `iter` and `iter_mut`. `Section` gains `header_footers()`, `header_footer_streams()` and `header_footers_mut()`, which replace about twenty hand-written six-stream arrays across the editor and layout. Header/footer commands now live in `editor/src/header_footer.rs`: `switch_header_footer`, `remove_header_footer`, a `HeaderFooterContext` with `title()`, and a generic `insert_field`.
- Render: overlay chrome gains `DashedLine` and `Image` shapes.
- Footnotes and Endnotes dialog (References > Footnotes) replaces the chain of list prompts. It lists the document's notes in reading order, numbered per kind, beside a preview of the selected note. Go to reference, Edit on page (or a double-click) and Delete act on the selected note, and a form inserts a new footnote or endnote at the caret. The dialog only collects choices; each change then runs through the editor as its own undo step. Generated by `tools/build_note_dialog.py`; `notext` moved into `dialog_kit` for reuse.
- Icons no longer look jagged. `ribbon/art.rs` shrank the 64 px artwork with bilinear sampling, which reads only four source pixels and so skipped thin strokes at 16–36 px. Shrinking now averages every covered source pixel (area resampling); enlarging stays bilinear.
- View > Show: the stacked checkboxes share a left edge in each column instead of being centred one by one. Small buttons stacked in a column (Typography/Hyphenation, Styles/Clear direct, …) are left-aligned the same way, so their icons line up.
- Sharp Ribbon buttons on Linux HiDPI screens. Button faces were painted at 1x because GTK reports DIPs equal to logical pixels, and GTK then stretched them 2x. `ribbon/skin.rs` now paints faces at the window's content scale factor (Windows is unchanged: it already paints in physical pixels). The document canvas does the same: the viewport works in physical pixels, mouse positions are converted once where they enter (`logical_to_view`), and the frame is drawn through a DC scaled down by the window factor, so page text is sharp. A test checks that a click hits the same character at 1x, 2x and 3x.
- Faster Ribbon tab switches: a page's buttons and groups are fitted and painted the first time it is shown (and again after a DPI change), not on every switch, and only the visible page's groups are laid out. While the user is idle, unvisited pages are prepared in the background, one per idle tick with the Ribbon frozen, so first visits are as quick as revisits. Switching tabs took 125–185 ms on a revisit and 170–360 ms on a first visit; both now take 12–40 ms. Cached icon rasters and caption sizes and an exact box filter for the 1x face make the preparation itself cheaper.
- Icon-only Ribbon buttons are 30 DIP wide (were 26), so the Font and Paragraph rows have room between icons. Ribbon number boxes (Layout > Paragraph indent and spacing) have a 4 DIP margin, so neighbouring boxes no longer touch.
- New document icon: a slightly bolder page outline and a much larger green plus badge, so it reads at 18 px.
- Room for every Ribbon widget: combo boxes are sized from their longest entry plus GTK's dropdown button (the font size box showed only "1"; `font_preview.rs` no longer forces it back to 60 DIP), the font name and size boxes are 8 DIP apart, and side-by-side columns of stacked commands have a 10 DIP gap, so labels and dropdown arrows never touch the next column.
- Simplified: the header/footer bands no longer carry icon tabs (and the `Image` chrome shape that drew them is gone); the unused grayscale icon copies are removed; system-theme icons are kept as PNGs instead of being wrapped in SVGs; the third header/footer iterator is gone.
- Citation Manager rebuilt on `dialog_kit`: section headings, an aligned Style/Locator grid, padded buttons with consistent gaps, and the standard skinned Insert citation / Cancel footer. The duplicate header Close button is gone (Cancel and Esc close the dialog). `Dialog.finish` gains `cancel_label` and `ok_width`.
- Linux: the Ubuntu dock showed a generic gear for Impress. Under Wayland the dock ignores window icons and shows the icon of the launcher whose name matches the app ID (`Impress`), and none was installed. `packaging/linux/install.sh` installs `Impress.desktop` (from `Impress.desktop.in`) and the scalable logo into `~/.local/share`.
- Fixed a wxWidgets assertion on Linux when typing ("wxGetKeyStateGTK(): Unsupported key 65"). The held-key repeat asked whether the typed key was still down, which GTK can only answer for modifier keys; that check now runs on Windows only, and elsewhere the repeat stops on key-up and focus loss.
- Columns work like Word's. Layout > Columns opens a menu of One, Two, Three, Left and Right, each with a picture of the page, and marks the current layout; More Columns opens a Columns dialog with the presets, the number of columns, each column's width and spacing (or Equal column width), Line between, Apply to (Whole document, This section, This point forward) and Start new column, with a live preview. Each choice is one undo step (`editor/src/columns.rs`).
- Layout engine: columns may have different widths. Each column has its own left edge and width, and a paragraph that continues into a column of another width is laid out again for that width. Line between draws a rule in each gap, and a "next column" section break starts the new section at the top of the next column. DOCX import and export keep unequal columns (`w:col`), `w:sep` and `nextColumn` breaks.
- Footnotes behave like Word's. Insert Footnote (Ctrl+Alt+F) and Insert Endnote (Ctrl+Alt+D) place the reference mark and move the caret into the empty note, ready to type. Hovering over a reference mark shows the note's text; double-clicking the mark jumps to the note, and double-clicking the note's number jumps back. References > Footnotes gains Next Footnote and Show Notes (which moves between the body and the notes), and its group launcher opens the Footnotes and Endnotes dialog.
- Bullets imported from Word no longer show as empty boxes. Word stores bullets as private-use characters of the Symbol and Wingdings fonts; layout and DOCX import (including `w:sym`) now map them to their Unicode equivalents (•, ▪, ➢, ✓, ❖, …), and unknown ones fall back to •.
- The expanded style gallery opens over the in-Ribbon gallery instead of below the group, so the styles stay where they were.
- Home > Font is laid out in two columns: the font name and size boxes on top, with the formatting buttons below; the font colour and highlight buttons line up with the size box.
- The Styles pane's close button (×, top right) is drawn. The pane is hidden when the Ribbon paints its buttons, so the button stayed blank; its face is now painted when the pane opens.
- Header & Footer tools work like the Table tools: while a header or footer is edited they replace the Insert page as a Ribbon page with Navigation (Go to Header, Go to Footer), Options (All pages, Different first page, Different odd & even), Insert (Page number, Page count, Date), Remove and a large red Close button. The hint line and the strip of buttons under the Ribbon are gone.
- Insert > Footer no longer appears to open the header. The footer was entered, but the view only followed the caret when a command left the document unchanged, and creating the footer changed it, so the page stayed scrolled to the top. Commands that change the document and move the caret now scroll to the caret once the new layout arrives.
- Recovery no longer keeps or offers untitled documents with no content (`Document::has_content`). Such snapshots are not written, and old ones are deleted at startup instead of being listed.
- The Recover Documents and Import PDF previews wrap their "No text was found" message instead of running past the page.
- PDF conversion like Word's. Opening a PDF converts it into an editable, untitled document in the background (with the unsaved-changes prompt, which opening a PDF used to skip) instead of showing the import options; File > Import PDF keeps the options and preview. The conversion now keeps:
  - each run's font, size, bold, italic and colour, with Normal and the heading styles taken from the PDF's own fonts, so runs carry only what differs from their style; web links become hyperlinks;
  - paragraph alignment (centred, right, justified) and indents;
  - bulleted, numbered, lettered and roman lists as real lists with nested levels, the marker removed from the text;
  - page columns, read one after the other; one page may mix full-width text and columns;
  - tables, recognised from cells that line up, with borders and column widths;
  - pictures (JPEG, and Gray/RGB/CMYK/indexed images with transparency), placed in the reading order;
  - the page size and margins;
  - repeated headers, footers and page numbers as the document's header and footer, with a page number field.
  Ligatures become their letters and symbol-font bullets real characters, so the text can be searched and spell-checked. Scanned pages without a text layer are kept as page pictures, rendered with hayro when their images are fax, JBIG2 or JPEG 2000. The importer is split into `extract`, `structure`, `build` and `scan` in `crates/format-pdf`, on a patched copy of pdf-extract in `vendor/pdf-extract` that reports fonts, colours and pictures.
- Layout: a justified paragraph could leave one word alone on a line. When reshaping made a line end a word before its planned break, the remaining plan still assumed the old start; the breaks are now planned again from where the line really ended.
- The executable is called `Impress` (was `impress`); the Linux launcher is `Impress.desktop`, and `install.sh` removes the old lowercase launcher and icon.
- Added `STYLE_GUIDE.md` and rewrote `ARCHITECTURE.md` as a complete source tree.

## 0.8.2 — Dialogs centred on the window

- Bumped all workspace packages to 0.8.2. This release includes every change listed below back to 0.8.1.
- Every dialog opens centred on the Impress window and stays on that window's display: Recover Documents, the Format dialogs, Export/Import PDF, Print, Table Position, Style editor, Citations, Find and Replace, and the choice, text and message dialogs. `placement::centre` places each dialog after it has its final size, just before it is shown. The Find window is re-centred each time it opens, instead of keeping the position it had when it was created.

## 2026-09-30 — Recover Documents dialog, selecting tables with the mouse

- Recovery after a crash now uses a proper Recover Documents dialog instead of a plain list. Each kept copy is a card with its file name, when it was saved and its folder, and a preview of the selected copy is shown beside the list.
  - Restore (or a double-click) opens the copy as a new, unsaved document. The original file is never replaced.
  - Delete discards one copy after confirmation.
  - Later keeps every copy for next time.
- Tables can be selected with the mouse, as in Word:
  - Drag from one cell into others to select the rectangle of cells between them. Past the table's edge the drag keeps selecting the outer cells, and back in the first cell it selects text again.
  - Drag along the column handles above the table, or the row handles beside it, to select several columns or rows. The × deletes them all as one undo step; selecting every row or column deletes the table.
  - Shift+click still extends a cell selection, and a click on the corner handle still selects the whole table.
- Dialog footer buttons (Format dialogs, Export/Import PDF, Print, Table Position, Recover Documents) no longer show a native button frame around their painted face on Linux: `dialog_kit` gives them `wxBU_EXACTFIT|wxBU_NOTEXT|wxBORDER_NONE`, like the Ribbon's buttons.
- The Recover Documents dialog appears only after the main window is fully built and drawn, instead of over a half-made window at startup.
- Editor: `delete_table_band` deletes a run of rows or columns, and undo steps can be grouped so compound edits undo at once.

## 2026-09-30 — Table tools in Insert, key repeat, lazy repainting

- Insert > Table works like Word's. Outside a table it opens a size grid (up to 10 × 8): move over it to pick columns and rows, then click, or use the arrow keys and Enter. The table is inserted and the Table tools open. Inside a table, the button opens the Table tools directly. The Table tools replace the Insert ribbon page, and a red Close Table Tools button returns to it. The Table button is no longer on Home, as in Word. The list-box dialog of table operations and the list-box size prompt are gone.
- Held keys repeat from the UI timer at a fixed, fast rate: every 22 ms after 320 ms, for typing, Backspace/Delete, Enter, arrows and Page Up/Down. They stop on release. System repeat events arrive at the user's (often slow) setting, and Windows folds them together whenever a frame runs late, so held keys used to crawl. The key handling is one function (`apply_key`) shared by key presses and repeats.
- Fixed blank grey Ribbon buttons on Windows after moving a table. The Table bar set its font on every text measurement. wxDragon's `Window::set_font` keeps a copy of the font on each call, so the process ran out of GDI handles and Windows could no longer create button images. The bar now sets its font once.
- Lazy repainting (checked by counting paints per surface while idle, typing and hovering):
  - Idle: only the caret blinks (two canvas frames a second).
  - Typing: only the canvas repaints. Before, the style gallery and its group frame repainted on every keystroke; they now repaint only when the styles, the current style or their enabled state change.
  - The Table tools repaint only when the current cell's facts change.
  - The status bar no longer re-lays itself out on every layout result. The layout-reuse counters it showed are removed.
  - The style caption is set only when its text changes.

## 2026-09-30 — Floating tables

- Tables can be free (floating) as well as in line with the text. A free table is placed by position and the text wraps around it, like Word's floating tables. The model stores this as `TableProperties::position` (`TablePosition`):
  - Horizontal: an offset or Left / Center / Right / Inside / Outside, relative to the margin, the page or the column.
  - Vertical: an offset or Top / Center / Bottom, relative to the anchor paragraph, the margin or the page.
  - Distance from the surrounding text on each side.
  - Wrapping: text around the table, or only above and below it.
  - Whether other floating tables may overlap.
- Anchoring follows Word: a free table is anchored to the paragraph that follows it. That paragraph decides the page and is the reference for paragraph-relative placement.
- Layout (`layout/src/floating.rs`): floating pictures, text boxes and tables share one placement and exclusion pass. A free table is laid out once as an unbreakable block, placed, kept on its page and above the footer/footnote area, and excluded from the text flow on each line it covers. The four distances from text are honoured separately. The table's cells keep their hit regions, so editing and the on-page table controls work as usual. A free table must fit on one page. In multi-column sections tables stay in line.
- DOCX: `w:tblpPr` (anchors, offsets, alignments, distances from text) and `w:tblOverlap` are imported and exported, so floating tables from Word keep their position. Word has no top-and-bottom wrap for tables; such tables export as wrapping around.
- Moving a table on the page follows its mode:
  - In line: the corner handle drops the table between paragraphs, as before.
  - Free: the table goes exactly where it is dropped. It re-anchors to the paragraph under its top edge, and its position is stored in the same reference frames as before. Shift moves it sideways only.
- The Table bar is redesigned as a Ribbon tab, modelled on Word's Table Layout tab. It has large icon buttons with captions, and menus (▾) instead of rows of small buttons:
  - Table: Select ▾ (cell, row, column, table) and Position ▾ (in line with text; free with text around; free with text above and below; More Position Options…).
  - Rows & Columns: Delete ▾ (a cell shifting left or up; rows; columns; the table), Insert Above, Below, Left, Right, and Insert Cells ▾.
  - Merge: Merge ▾ (right or below) and Split.
  - Cell Size: labelled width and height steppers, and Distribute columns.
  - Alignment: the three vertical alignments and Repeat header.
- The Position button's icon shows how the table is placed. More Position Options… opens the Table Position dialog: horizontal and vertical placement, references, distances from text, wrapping and overlap, with a live sketch of the page and the text flowing around the table.
- Clicking in a table cell's empty space puts the caret in that cell. Before, it could land in the paragraph above or below the table (`ViewportState::hit_test`).
- Editor: `set_table_position` (float, re-anchor or make inline in one undo step), built on a shared `relocate_table` with `move_table`.
- Validation: new tests cover floating layout (placement, wrapping around it, and text above and below only), DOCX `w:tblpPr` round trips and the editor operations. On Linux, a table was made free, dragged across the page with text wrapping around it, and right-aligned with the Position dialog.

## 2026-09-30 — Table editing, PDF import and export, printing

- Table controls on the page (`gui/src/table_ui.rs`). While the caret is in a table, or the pointer is over one, the table shows slim handles above its columns and beside its rows, a corner handle and a resize handle. Resting on a border in the handle strip shows a blue "+" with a guide line; clicking it inserts a column or row there. Clicking a handle selects the column, row or table and puts a × button beside it that deletes it. A single selected cell (Select → Cell, or Shift+click) gets a × at its corner that deletes the cell and moves the cells to its right one place left. Backspace never deletes table structure; Delete still clears the selected cells' text. Hovering a border shows a guide along the whole table and the resize cursor; dragging shows the guide at the new position. Selections are a light tint with one outline around the whole selection instead of a thick box per cell. The controls are drawn anti-aliased in view pixels (`render::ChromeShape`), so they keep their size at every zoom.
- Moving and resizing tables. Dragging the corner handle moves the table: it goes before the paragraph under its top edge, or after the last line when dropped past the text, and its horizontal position becomes the table indent, which snaps back to the margin within 2 mm. Shift keeps the table where it is and moves it sideways only. Dragging the bottom-right handle scales all columns, and the row heights, in one undoable step; Shift keeps the proportions. Escape cancels either drag. The layout engine has no floating tables with text wrap, so a moved table is always anchored in the text flow.
- Table bar (`gui/src/ribbon/table_bar.rs`). The old table strip, which showed its caption but none of its buttons, is replaced by one painted bar with icons: Select (cell, row, column, table); Rows & Columns (insert above, below, left, right; insert cells shifting right or down); Delete (row, column, table; delete a cell shifting left or up); Merge (right, down, split); Cell Size (width and minimum-height steppers in 1 mm steps, height back to Auto, distribute columns); Alignment (top, centre, bottom, header row). Commands that don't apply are greyed out, the current alignment and header row are highlighted, and small labels are dropped when the window is narrow.
- Editor: `delete_table`, `shift_table_cells` (Word's Insert Cells / Delete Cells; the grid stays rectangular, and the table grows only when a non-empty cell would be pushed out), `move_table` and `scale_table`, each one undo step, with tests.
- PDF import (new crate `format-pdf`, built on `pdf-extract`). It recovers positioned text from the page content streams and rebuilds paragraphs from line spacing, indentation, size changes and sentence ends. Clearly larger text becomes Title / Heading 1–3. Page numbers and running headers or footers repeated across pages are dropped, words hyphenated at line ends are rejoined, and paragraphs continue across page breaks. Title, author and subject are read from the PDF's information dictionary. Pictures and exact layout are not imported. File → Import PDF, or choosing a .pdf in Open, shows the Import PDF dialog. It has a live preview of the result, a page choice, flowing paragraphs or the PDF's line breaks, and clean-up options. The result opens as a new untitled document. `impress file.pdf` imports with the default options.
- Export PDF dialog. It replaces the page-range text prompt with a page preview you can page through, a destination with Browse…, All pages / current page / page ranges with a live summary, title, author, subject and keywords (written to the PDF), an option to keep links clickable, and an option to open the PDF when the export finishes. Exports still run in the background from a snapshot of the displayed pages.
- Print. File → Print is a large button and Ctrl+P opens the new Print dialog: page preview, pages, copies with collation, and fit to paper or actual size. The system print dialog then chooses the printer and paper. On Windows the existing GDI band spooler prints on a worker thread; on Linux and macOS printing goes through the platform print system (wxWidgets printing). Before, printing only worked on Windows.
- Validation: new editor tests (cell shifting, table delete, move, scale) and PDF import tests pass. All three dialogs, importing a generated report and printing it to a file were checked on Linux. The Python ribbon and dialog checks fail on exactly the same tests as the original source.

## 2026-09-30 — Visual equation editor

- Replaced the equation pane's text box and template buttons with a visual editor. The formula is shown as it will look, with a caret, a tinted current slot and dashed boxes for empty slots. Typing builds structure (`/`, `^`, `_`, delimiters, `\name` commands with completion hints, operator names, `<=`-style digraphs). Clicks and drags place the caret and select, arrow keys, Tab and Backspace move through and out of structures, and undo works within the draft. A categorized palette (structures, Greek, operators, relations, arrows, symbols) previews every entry in the same math font; matrices get row and column tools. Insert/Update and Cancel sit at the bottom, and a Source toggle shows the linear syntax, kept in sync both ways. Enter while the page preview is still updating inserts as soon as it is ready.
- Typst output of equations: scripts on a single token no longer draw parentheses around the base (`x^(2)` instead of `(x)^(2)`); other bases use `attach`. Multi-letter identifiers that are not Typst operators render as italic names instead of failing to compile. Commas, slashes and other syntax characters in operators are escaped.
- Styles gallery: the in-ribbon gallery is one Word-style control (`ribbon/style_gallery.rs`). Three cards preview paragraph styles in their own fonts and page through every paragraph style in Word's order (Normal, headings, Title, Subtitle, then alphabetical). A strip on the right has row up and row down, which grey out at the ends, and More, which opens the expanded gallery. Clicking a card applies the style; the mouse wheel pages rows. Built-in names show capitalized and each style appears once. The old fixed style buttons stay only in the hidden compatibility panel.
- The Styles group caption has Word's dialog launcher, a rounded square with a diagonal arrow (`design/icons/dialog-launcher.svg`). It opens the Styles pane, as Word's does.
- Ribbon tabs are painted by the tab strip (`ribbon/tab_strip.rs`) instead of native buttons, so Linux and Windows look the same: plain captions on the title colour, a hover tint, and one white active tab with a rounded top that flares into the ribbon body. The group frames no longer draw a top edge, so the tab and the body are one uniform surface with no native button frame or seam. The hidden tab buttons keep their names and accessibility labels.
- Validation: new unit tests cover typing shortcuts, navigation, matrices, selection wrapping, document round trips, layout geometry and the Typst output fixes. `cargo test --workspace --release` passes (424 tests before the gallery change; the GUI suite, 79 tests, afterwards). The ribbon contract tests now check the single painted gallery control.

## 2026-09-29 — Interface performance

Measured on a generated 61-page DOCX (2-core Linux VM, release build):

| | Before | After |
|---|---|---|
| Open to first rendered page | ~20 s | ~1.5 s |
| Layout per keystroke | ~4.8 s | ~45 ms |
| Frame while scrolling | 55–105 ms | a few ms |

- Typography: rustybuzz faces are parsed once per font and cached in the font catalogue (`FontCatalog::with_face`, `FontCatalog::shape`). Previously every word rebuilt the font's GSUB/GPOS accelerators. Shaped words are cached per font, size and typography options. Missing font families are now cached too; before, the fallback chain re-queried the font database for every word.
- Scene: glyph outlines are shared (`GlyphVector::outline: Arc<VectorPath>`), with one outline per font, glyph and size. Text runs and clipped commands are shared as well. Per-glyph outline copies had made laid-out pages so large that the incremental layout cache refused to retain them, so every keystroke re-laid the whole document.
- Canvas: the renderer is rewritten around page tiles cached at the current zoom. Scrolling copies tiles and rasterizes only newly exposed ones; the native bitmap is shifted and only exposed strips are converted. An edit re-rasterizes only the tiles its changed commands touch. Caret blinks and selection changes rasterize nothing and present only their own pixels. Glyph paths and command bounds are computed once per outline and page revision. Page origins are whole pixels (`ViewportState::page_rects_px`, one pass for all pages).
- Page Up/Down: tiles are rasterized in parallel on a small thread pool. A frame spends at most 8 ms on new tiles; the rest show as blank paper and fill in over the next ticks, so a held key never waits for a whole screen. While the canvas is idle, the screens above and below are rasterized ahead of time, a few tiles per tick. On a 2-core machine, at 1600×1200 and 144 DPI, a Page Down frame costs 45 ms when the whole screen must be drawn at once, about 16 ms for the first budgeted frame, and 6 ms after prefetch.
- Ribbon: status labels are set only when their text changes, and the status bar re-lays out when it does (the texts overlapped before). Table/header/picture context is derived only when the revision, selection or selected object changes, instead of on every 16 ms tick. Idle events go only to windows that ask for them.
- Linux: mouse wheel and pinch zoom work on the portable path (their handlers never matched wxDragon's event category). `impress <file>` opens a document.
- Validation: the workspace builds and runs on Ubuntu 24.04 (GTK 3). `cargo test --workspace --release` passed 406 tests, 0 failed. New canvas tests compare tiled, incremental, scrolled and partially presented frames against cold renders.

## 0.8.1 — Dropdown alignment and typing repaint performance

- Bumped all workspace packages to 0.8.1; the About dialog reads the package version.
- Matched the font field to the native size dropdown's actual height, including size events, and centered the editable font text within it.
- Added 9 logical pixels of horizontal content padding on each side of every ribbon group, preserving connected dividers and caption strips.
- Retained rasterized text outside changed command bounds between layout results. Insertions, deletions, and line movement repaint the affected area; pagination, viewport changes, and complex paths retain full-repaint fallbacks. Selection and caret overlays remain separate from cached page pixels.
- Reused the native viewport bitmap and transferred only changed pixels, including old/new selection and caret bounds, rather than converting the full viewport from RGBA on every keystroke or caret blink. Used the general antialiased path for fractional thin rectangles to avoid the rasterizer's hairline assertion.
- Validation: 60 GUI tests passed, including pixel equality against full repaint for insertion/deletion at 96/144/192 DPI, reflow, clipping, removed content, pagination, and native update regions including overlays. Ribbon validation passed. A release-mode synthetic 2,450-glyph page benchmark took 1,001 ms for 40 full repaints versus 54 ms for incremental repaints; these are raster timings, not end-to-end keyboard latency. A development-build 120-key pipeline measurement averaged 0.14 ms for editing and 6.9 ms for layout per key, excluding native display work.

## 2026-09-27 — Current ribbon fix snapshot (UI verification pending)

- Guarded shared dropdown dismissal against repeated activation events and moved mouse opening to button release. Initialized the font field's child layout explicitly.
- Updated ribbon sizing to measure the painted labels and recalculate group sizes when changing tabs. Adjusted outer rounded corners and caption-strip corners.
- Corrected a regression in the intermediate build that exposed icon-only command labels and duplicated tab labels. Generated painter labels now follow the adopted layout, while native labels are empty. Preserved the Styles resize handle in the generator.
- The latest Windows release compiled successfully. The earlier intermediate build passed 57 GUI tests, but visual testing exposed the label regression. The final correction and mouse dropdown behavior have not been visually verified; computer testing was stopped by the user. This snapshot is work in progress.

## 2026-09-27 — Ribbon controls and startup

- Corrected double DPI scaling in the font and Styles popups. Both now use an anchored modeless dropdown, dismiss on Escape or loss of focus, and leave the main window enabled. Shared placement keeps them within the monitor work area. Removed the blue underline from all ribbon caption strips.
- Replaced the ribbon font bitmap combo with an editable field and a bounded, scrollable list that draws each visible font name in its own face. The list loads installed family names when first opened, avoiding font preview work during startup and the XRC flag error.
- Matched the ribbon font and size field heights and restored all ribbon group captions on short blue-tinted stripes.
- Removed redundant ribbon icon bundles and render button faces only for visible tabs. Visible buttons are sized before their faces are painted, avoiding an immediate second render.

## 2026-09-26 — DOCX run direction, numbering, and drawings

- Imported and exported explicit run-level right-to-left formatting. The layout shaper now honors the run's requested direction when assigning bidirectional levels.
- Imported and exported section page-number restarts and decimal, Roman, or letter formats. PAGE and PAGEREF fields and header/footer display use the section's visible page label.
- Added structured inline DrawingML text boxes with dimensions, paragraph/table content, fill, and outline. Layout paints their contents inside the box; DOCX export regenerates a Word text-box shape. Floating text boxes are placed inline and remain flagged by the compatibility audit.
- Added picture crop and outer-shadow import, paint, and DOCX export. Other DrawingML effects remain explicitly flagged by the audit.
- The supplied English AI Stories DOCX now imports with no compatibility issues reported. DOCX and layout round-trip/rendering tests pass; the workspace builds. The editor suite retains its previously known unrelated nested-table undo failure.

## 2026-09-26 — Source cleanup and smaller Windows build

- Moved the complete DOCX adapter into `crates/docx/`, including its round-trip tests and DOCX document fixtures. Renamed the package to `docx`, updated GUI registration and compatibility calls, and removed stale DOCX source inspections from generic Python tests. Updated the root architecture map.
- Consolidated the seven editor clipboard source files into `editor/src/clipboard.rs` and exposed one clipboard module. The GUI now imports HTML, RTF, and bitmap conversion through that module.
- Consolidated editor comments, per-change revision review, and the native revision journal into `editor/src/review.rs`; removed the two superseded files.
- Combined the Modify Style dialog and docked Styles pane implementation in `gui/src/ribbon/styles.rs` and removed their former source files.
- Replaced the Win32 button subclass with wxDragon bitmap bundles and text drawing for ribbon and style-dialog buttons; removed `skin_win32.rs`.
- Renamed workspace package folders and package names without the `impress-` prefix: `core`, `editor`, `gui`, and the format, typography, layout, render, and output crates. Dependents import the local `core` package as `model` to keep Rust's standard `core` namespace unambiguous.
- Added root `ARCHITECTURE.md` with the source tree, module roles, and dependency boundaries.
- Removed the unused vector tessellation module and its dependencies, and updated active UI asset checks for embedded PNGs.
- Packaged the release executable instead of the development executable. Size fell from 112 MiB to 51.9 MiB; the Windows ZIP is about 23 MB.
- Enabled size-oriented release optimization and link-time dead-code elimination.
- Embedded PNG ribbon assets directly and decoded them on demand; removed the duplicate raw RGBA asset set.
- Removed unused recovery and output-progress code. Build scripts now compile the application crate for distribution.
- Consolidated release notes here and removed obsolete status, validation, and QA snapshot files and their unused scripts.
- Validation after the rename: `cargo check -p gui`, release build, core/DOCX/layout/render/GUI tests, and ribbon/format-dialog checks passed. The supplied DOCX opened to page 1 of 14 in about 0.5 seconds. One editor unit test, `nested_cell_multi_paragraph_delete_stays_inside_cell_and_undoes`, still fails because undo omits a trailing empty paragraph outside the table.

## 2026-09-26 — DOCX import and rendering

- Rechecked the supplied 21 KB DOCX through the standalone adapter: one section, 397 paragraphs, one table, and one list imported. Corrected explicit false `w:bidi` and `w:titlePg` flags, with an importer regression test. The file still reports run-level RTL and page-numbering compatibility limitations.
- Corrected DOCX default paragraph spacing, table widths and indentation, row flags, and hanging list markers. The supplied 14-page DOCX renders with separated bullets and a full-width table.
- Parallelized independent table-row preparation and optimized the layout hot path. The supplied document now reaches its first rendered page in about 0.5 seconds on the tested Windows machine.
- Removed duplicate DOCX helpers and kept format parsing, semantic document data, and prepared layout data in their respective crates.

## 0.8.0-alpha.15 — Interface and style editing

- Added Ctrl+Left/Right Unicode word navigation and Ctrl+Up/Down paragraph navigation; holding Shift extends the selection from its original anchor.
- Left/Right now collapse an existing selection to its respective edge. Navigating after rapid typing requests the current layout immediately, and opening the Styles pane leaves document keyboard focus in place.
- Added Word-style More arrow in the lower-right of the ribbon Styles gallery. It opens a scrollable grid of preview cards in the same visual language as the ribbon, and selecting one applies it.
- Added a native scrollbar to the docked Styles list when styles exceed its height; wheel, thumb, arrow, page, and keyboard navigation remain synchronized.
- Ribbon font-family choices now paint each full font name in its own face. The font-family and size dropdowns share a control height. Gallery style actions clear direct formatting so the chosen paragraph style is visible immediately; selection dragging reaches the nearest insertion point in margins.
- Keystroke layout now lets an in-flight page finish and queues the newest typed state, so holding a key no longer cancels every frame. Removed redundant stale-canvas repaints for repeat input and kept the caret visible while the painted page catches up.
- Removed a full glyph-outline comparison of every page from the layout worker's per-edit status accounting; the incremental paginator already counts the pages it reused.
- Focus the document canvas after ribbon and recovery setup, so typing works immediately on opening the window. Restore the three style gallery previews using their existing assets, trimmed to visible lettering.
- Raised the shared Segoe UI control font to 11 pt and rendered ribbon captions at the bitmap's display DPI.
- Replaced fixed font dropdown entries with the installed, renderable family names from the shared typography catalog in the ribbon, Modify Style, and Font dialog.
- Fixed selection highlighting that the page background had covered. Selection drags now composite over a cached page instead of rerasterizing its glyphs, and selection order is resolved in one pass.
- Added keyboard selection regressions and Ctrl+plus/minus support for main and numeric keypads; the Styles pane avoids rebuilding unchanged style rows on each keystroke.
- Retained immutable prepared blocks with section checkpoints, so an edit prepares only changed paragraphs before pagination. A local 20-page benchmark prepared 1 of 800 blocks in 88–110 ms across two runs and produced the same vector scene as a cold layout.
- Corrected editable-document initialization so a table-only document does not gain an empty paragraph; the full editor test suite now passes.
- Corrected high-DPI ribbon face bundles so physical pixels do not inflate button and group layout twice.
- Imported and exported anchored editable text boxes, VML text boxes/images, OLE payloads with preview images, and glow/soft-edge/reflection picture effects. Text-box paragraphs now participate in caret hit testing and undo.

- Reworked the ribbon and Styles pane around the HTML reference, including pane resizing, joined control styling, and the File tab Open action.
- Added the Modify Style toolbar, live preview, color palette, and connected Format dialogs. Style edits participate in inheritance, document save, and undo.
- Moved document painting to the wxDragon canvas and removed the Win32 Vulkan page renderer.

## Earlier alpha milestones

- Alpha 14 added vector PDF output, native Windows printing, Find/Replace, and background saving.
- Alpha 13 added reconstructed RTF, floating images, and review features.
- Alpha 12 added reference integrity, continuing notes, and richer clipboard interchange.
- Alpha 10 and 11 expanded editing, native input, equation drafts, and raster canvas behavior.
- Alpha 5 and 6 introduced document structures, DOCX fidelity work, structured editing, and pagination.
- The initial releases established the semantic model, typography, vector pages, and wxDragon desktop shell.
