# DrawingML preset geometry data

The 187 definitions are vendored from Apache POI REL_5_4_1:
https://github.com/apache/poi/blob/REL_5_4_1/poi/src/main/resources/org/apache/poi/sl/draw/geom/presetShapeDefinitions.xml

Licensed under Apache-2.0; see LICENSE and NOTICE. Only preset data is used.
Impress evaluates and renders the definitions in Rust. The definitions are
compiled into the application; Apache POI and Java are not runtime dependencies.

Regenerate Rust data with `python3 tools/build_picture_shapes.py`. The generator
removes one redundant trailing zero in eight arrow formulas in the source data.
Product labels are kept in design/picture-shapes.json and the localization catalog.
