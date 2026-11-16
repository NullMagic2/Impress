# Page rulers

The measurement scale follows the paper that is visible in the canvas. Zero
marks the left/top page margin; the white band ends at the right/bottom margin.
Ticks are drawn over the sheet, including its shaded margins, and stop at its
edges. The vertical ruler restarts on every visible sheet and leaves page gaps
blank. The horizontal ruler follows the sheet with the largest visible area.

Layout carries each physical sheet's margin rectangle in page points, including
section changes, landscape pages and intentionally blank odd/even section pages.
A shared continuous-section page retains the owning section's paper geometry.
Rulers use the canvas's rounded page rectangles and physical-to-window scale,
then translate between the actual widget client origins. Scrolling, zooming,
resizing and display scaling therefore use the same coordinates as painting.

Tab stops and paragraph-indent handles still apply to the selected paragraph,
column or table cell. Their positions can differ from the page margins, but do
not change the page scale's zero or white band. Handles are hidden when the
focused paragraph is on a different sheet from the horizontal ruler; scrolling
does not silently retarget the selection or edit another paragraph.

Regression checks cover margin coordinates at multiple zoom/DPI/display scales,
horizontal and vertical scrolling, different page sizes and margins, page gaps,
and section pagination with an intentional blank sheet. Native X11 screenshots
also check the reported DOCX with a caret left in a table cell while scrolling.
