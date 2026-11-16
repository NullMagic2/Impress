# Rulers and tab stops

Horizontal and vertical rulers span the document editing area. Measurements are in centimetres, with millimetre ticks; the horizontal scale sits against the lower edge of its strip, alongside the tab markers; the highlighted range follows the page's text area. Zoom, scrolling, page margins, columns and table-cell widths are reflected in the ruler. Use **View → Ruler** to show or hide both rulers.

The black handles use the same center for the triangles and left-indent box when the first-line offset is zero. Drag the upper triangle for the **first-line indent**, the lower left triangle for the **hanging indent**, the small box below it for the **left indent**, and the right triangle for the **right indent**. The left box moves the first line and remaining lines together; the hanging handle keeps the first line fixed. Handles apply to the current paragraph or selected paragraphs. Hold **Alt** for 0.1 mm precision instead of 1 mm snapping. Escape cancels a drag, and each completed drag is one undoable edit.

The symbol at the top-left cycles **Left**, **Center**, **Right**, **Decimal**, and **Bar**. Click the horizontal ruler within the text area to add the selected kind. Drag a stop to move it, or drag it above/below the ruler and release to remove it. Escape cancels a drag. Each completed gesture is one undoable edit. Stops apply to the current paragraph or selected paragraphs, preserving unrelated stops in mixed selections.

Double-click or right-click the horizontal ruler to open **Tabs**. The ribbon has no Tabs button. Enter a position in millimetres, choose alignment and leader, then **Set**. Select an existing position to edit it. **Clear** removes the selected stop; **Clear All** removes explicit/inherited stops from the affected paragraphs. **OK** applies changes in one undo step; **Cancel** discards them. Opening and accepting unchanged settings preserves mixed selections and inheritance.

The dialog supports no leader, dots, dashes, a line, a heavy line, and middle dots. Default tab spacing starts at **12.7 mm** (half an inch). Changing **Default tab stops** changes the document's automatic interval and the affected paragraphs; this is saved in native documents and DOCX settings. Explicit positions are measured from the text-area left margin, independently of paragraph indents.

**Tab** advances to the next explicit alignment stop or automatic stop. Center and Right align the following segment; Decimal aligns its decimal separator. A **Bar** draws a vertical rule through the paragraph lines without changing text position or requiring a Tab character. Inside a table, ordinary Tab navigates cells; **Ctrl+Tab** inserts a text tab. Ctrl+Tab also inserts a literal tab in a list, where ordinary Tab changes the list level.

Native documents preserve all paragraph tab settings. DOCX preserves explicit alignments, leaders, inherited-stop clears and document automatic spacing. Tabs in RTL or mixed-direction paragraphs remain unsupported by the layout engine; the existing layout error reports this limitation.

The tab-type selector returns focus to the document after a click, so typing and Tab continue immediately. Decimal tabs include a dot to distinguish their marker from Center tabs. The selector chooses the type of the next stop; click a position in the ruler to place it.
