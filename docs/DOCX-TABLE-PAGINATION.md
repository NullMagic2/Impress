# DOCX tables with tall rows

Some Word tables contain more text in one row than fits on a page. Adjacent
cells can have different fonts, blank paragraphs or spacing, so their complete
text lines do not necessarily end at a shared vertical coordinate.

Impress previously required one shared line boundary across the entire row.
When no boundary existed, layout stopped and the document canvas remained
blank even though DOCX import had succeeded.

Pagination still uses a shared boundary when one is available. Otherwise,
each text cell breaks at its own last safe complete line and its remainder
continues below the top padding on the next page. The pieces retain shared
row heights, cell borders, shading, source offsets and caret/selection ranges.
A finished short cell contributes no repeated text or artificial height to
later pieces. Repeated headers continue to reserve room on every page.

Explicit no-break rows, vertical merge groups, indivisible objects and vector
paths without safe bounds retain their existing restrictions. The fallback
requires progress and does not clip or discard text to force a row to fit.

Regression tests construct synthetic documents with offset cell lines,
finished short cells and repeated headers, and cover DOCX save/import/save
roundtrips followed by pagination. Text and caret source ranges are checked
for gaps or duplication. The user-provided Word file is tested separately;
its private contents are not bundled with the source distribution.

# Overset text and cached fields

Long cached field values, including hyperlink timestamps, now wrap visually
at grapheme/shaping-cluster boundaries. Adjacent fragments are reshaped within
a line for kerning/ligatures and regional fonts. The semantic field, its full
cached value and original object-marker source positions remain unchanged.
Ordinary fields that already fit remain single tokens.

Line fitting still reserves cell padding. The physical cell boundary is the
paint clip, allowing ordinary glyph overhang in the padding without entering
a neighboring column. The canvas keeps unclipped ink bounds when deciding
whether to apply an edge mask; intersecting those bounds with the clip first
previously allowed overflowing glyphs to bypass masking.

The supplied document is also audited separately for both text-run bounds
and transformed glyph-outline bounds against every physical table cell, with
zero remaining overset findings. Synthetic tests cover a long Unicode cached
field, retained atomic source positions, and raster pixels beyond a cell edge.
