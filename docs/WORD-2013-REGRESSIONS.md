# Word 2013 compatibility regressions

The desktop Word 2013 comparison exposed failures that native round trips alone
did not catch. The following corrections are implemented in the source.

| Failure | Correction |
| --- | --- |
| Binary DOC exports were rejected by Word file validation | Emit the mandatory 18-entry associated-string table and reserve all fifteen fixed style indices. |
| Imported Normal formatting could be replaced on export | Keep exported style names unique and use the imported Normal in binary DOC's Normal slot. |
| Deleted DOC text appeared as ordinary content | Decode the binary character insertion/deletion flags into reversible text revisions. |
| Fixed table layout was lost | Preserve `w:tblLayout` through DOCX and native files. |
| Nested tables stacked despite fitting side by side | Use authored expanded grids for automatic tables and actual table bounds for collision checks. Text still respects wrapping clearance. |
| Saving introduced a field-update prompt | Retain the file's existing `updateFields` preference instead of forcing updates. |
| The proof document required repair for nonexistent notes | Remove orphan footnote/endnote separator references from exported settings when the corresponding notes do not exist. |
| Comments acquired a literal PAGE instruction | Import visible comment text and field results without field instructions or deleted text. |
| Missing Office fonts or mislabeled embedded aliases changed typography | Prefer genuine matching fonts, then installed Carlito/Caladea substitutes for Calibri/Cambria, before unrelated embedded aliases. Stored font intent is unchanged. |

## Verification

The DOCX, binary DOC, layout, editor and typography suites pass 197 tests.
New regressions cover the failures above using synthetic fixtures. Private user
documents are not included in the source repository.

The rebuilt Impress executable rendered the proof document's pages 2 and 3 in
an isolated session. The tables are side by side, text uses the expected
sans-serif face, and those areas have no visible clipping. LibreOffice also
opened the corrected simple and table-heavy DOC exports. These checks do not
establish identical pagination or complete Word compatibility.

Desktop Word acceptance testing of the corrected exports is pending access to
the signed-in VM's compatibility console. The original rejected DOC exports were
tested with Word file validation enabled. Passing native tests does not establish
that Word accepts the corrected binary files; no validation bypass is used.

Binary DOC export still flattens revision history, and imported binary revision
author/date metadata is incomplete. The complete DOC/DOCX specifications remain
beyond the supported subset documented in the format guides.
