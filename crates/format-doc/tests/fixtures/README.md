# Native Word97 import fixture

`libreoffice-word97.doc` was generated during development from this repository's
synthetic `crates/docx/tests/fixtures/mixed.docx` with LibreOffice 26.2.6.3's
"MS Word 97" export filter. It is an independent producer's binary DOC, not an
Impress DOC export and not a user document. It contains synthetic Greek/accented
text, a numbered table cell, two page setups, a raster picture, a live header
page field, a footnote, a comment and a bookmark.

SHA-256: `071466557186d0ab49d79c4f7301f23bb9fff0f2468ce381b74675dc54b84cac`.

LibreOffice is a fixture-generation and independent QA tool only. The tests use
the checked-in bytes and the native Rust importer; they do not start an office
application or require one to be installed. Corruption tests mutate disposable
in-memory copies of this fixture.
