# rwml 0.1.5

Upstream: https://github.com/HyunjoJung/rwml
License: MIT (see LICENSE).
Source: https://static.crates.io/crates/rwml/rwml-0.1.5.crate

Impress statically compiles the native DOC reader and in-memory DOCX serializer.
No Office, LibreOffice, Java, process launcher, or separately installed executable
is used. The DOCX buffer is parsed by Impress's ordinary DOCX importer.

Local manifest changes: rlib only; disable upstream CLI/examples/integration-test
autodiscovery; use the already-selected cfb0.14 API; remove upstream local
font-package path/dev dependencies. Rendering/bundled-font features are not used.
Upstream source and license notices are retained.

Local reader correction: standard nonextended FFN entries decode their primary
UTF-16 family at payload offset39 (40-byte fixed FFN prefix minus length byte),
rather than scanning misaligned pairs. Synthetic extended-record fallback remains.

Local PAPX compatibility correction: accept Word97 physical indent opcodes
0x840E/0x840F/0x8411 in addition to their later logical aliases.

Local native conversion extensions (release42):
- Preserve sparse native STSH paragraph/character identities, base/next graphs,
  direct properties, style-assigned numbering and relative character toggles.
- Retain original native list definitions/instances, starts/restarts and templates.
- Project CP-indexed bookmarks, rich footnotes/endnotes and authored comment ranges
  into the in-memory OOXML bridge, removing only successfully projected stories.
- Decode HugePAPX Data references, nested table depth/inner delimiters, merged grids,
  cell RGB borders/shading, vertical alignment, row heights, table spacing/width,
  floating table placement and legacy/modern paragraph borders.
- Decode bounded OfficeArt raster stores, PICF display goals, crop/alt metadata,
  body/header FSPA anchors and supported wrapping, distances and positions.
- Preserve floating header/footer images through the shared semantic DOCX importer.
- Reject malformed CLX ranges and text pointers outside WordDocument before assembly.
- Derive auxiliary-story warnings from unprojected regions; successfully anchored
  notes/comments and selected running stories no longer produce stale loss warnings.
All new bridge modules retain the upstream MIT license. Raster/vector limitations
and unrepresented review/object metadata remain documented and diagnosed.

The extended semantic bridge is exposed through to_impress_docx() and
impress_conversion_warnings(). The original upstream to_docx(), model() and
report() contracts remain available; conversion-only metadata is cleared from
the original rendering model. All 829 upstream reader tests pass with their original assertions.
Compact/complex PCD overlays append sparse formatting after CHPX and participate
in native run boundaries, preserving explicit-off versus inherited properties.

Native hyperlink field-data markers (U+0001) are skipped during instruction
accumulation. External targets and local \l bookmark fragments retain actual
hyperlink semantics instead of cached generic fields. This is covered by an
independent-producer import/export/reimport regression.
