# Picture crop shapes

Select a picture, then choose **Picture tools → Crop to Shape**. The compact picker groups 176 filled shapes with previews, including ovals, rounded rectangles, polygons, arrows, stars and callouts. The selected shape is highlighted. Changes support Undo and Redo.

The original image remains intact. The shape clips the existing rectangular crop, and an imported outline follows its edge. DOCX and Impress files preserve the preset name, adjustment values, crop fractions and solid RGB outline. Selecting another preset uses that preset's default adjustments.

The renderer evaluates all 187 standard DrawingML preset definitions. Open line-only shapes are retained when imported but are omitted from the picture crop picker. Screen display, PDF export and printing use the same picture scene.

Custom/freeform geometry, theme-based or dashed/compound outlines, three-dimensional picture effects and adjustment-handle editing are outside this implementation. Unknown or invalid geometry displays as a rectangle. Legacy binary DOC export retains rectangular crop only and warns when shape crops or outlines cannot be represented. Keep a DOCX or Impress copy.

Clicking blank canvas or page margins clears object selection and active table controls. Clicking empty space within a table cell still places the text caret in that cell.

Preset data is supplied under Apache-2.0; see `vendor/drawingml-shapes/` for the pinned source, license and notice.

The compact picker uses 95 matching LibreOffice Yaru icons. The other 81 standard presets use neutral white and charcoal geometry previews. Artwork is bundled with Impress; LibreOffice is not required. Shape selection and saved document geometry are unchanged. See `crates/gui/assets/picture-crops/README.md` for artwork sources and notices.
