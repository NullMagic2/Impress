# Independent legacy Word conversion

Impress reads and writes genuine Word 97–2003 binary `.doc` files inside its own
process. No LibreOffice, Microsoft Office, Java, COM automation, conversion service
or helper program is required. The former subprocess bridge has been removed.

The same native format adapter is used by the editor and the standalone
`impress-convert` command:

```sh
impress-convert input.doc output.docx
impress-convert input.docx output.doc
impress-convert input.doc output.impress
```

The output extension selects `.doc`, `.docx` or `.impress`. Existing output files
require `--overwrite`. Warnings go to stderr. Output is written atomically and
reimported with native code before replacing the destination. Keep the source
when converting features the destination does not support.

## Native implementation

`crates/format-doc/` owns validation, integration, the binary writer and the CLI.
The reader statically compiles the MIT-licensed Rust implementation in
`vendor/rwml/`; its source and license are included. Its Word binary parser reads
CFB streams, FIB/CLX piece tables, supported codepages, styles, CHPX/PAPX formatting,
sections, tables, lists, pictures and running stories. It constructs an in-memory
DOCX representation which Impress's existing semantic importer reads. That buffer
never passes through an installed program or a temporary conversion profile.

The native writer creates CFB `WordDocument` and `1Table` streams, a Word97 FIB,
Unicode piece table, font/style tables, character/paragraph formatting pages and
section/running-story records. It exports supported editable text, character and
paragraph formatting, original style inheritance and next-style graphs, page geometry,
unequal columns, nested/merged tables with cell borders/shading and supported
floating placement, and all selected header/footer variants. Supported raster
pictures retain crop, alt text, display dimensions and body/header floating
anchors. Automatic list definitions retain levels, templates, independent list
instances and start/restart values. Supported live fields, bookmarks, comment
ranges, footnotes and endnotes retain native source positions, including
supplementary Unicode. Notes retain supported rich block content.

The fixed 21-capability native checklist is implemented in the current source.
This is a completion measure for the documented model-supported subset, not the
entire Word specification. The checklist and build verification are listed
separately in [docx_progress.html](docx_progress.html).

Representation limits remain explicit. DOC export does not embed fonts and does
not reconstruct arbitrary OLE objects, equations, vector shapes or text-box
geometry. These use warned readable fallbacks. Advanced picture effects,
revision histories and comment thread/date/resolved metadata are not preserved.
On import, binary character insertion/deletion flags become reversible text
revisions, rather than exposing deleted text as ordinary document content.
Legacy revision author/date recovery is still incomplete.
Top-and-bottom floating table wrapping uses warned side exclusion distances;
normal around-text table positioning preserves the authored gaps. General
vector/metafile shapes are not counted as raster-picture support. Keep the
original document when converting these features.

Native format bounds include 63 logical table columns, 64 nested table levels,
45 section columns, nine list levels and 255 UTF-16 units per list template.
Oversized structures or invalid anchors fail clearly instead of silently dropping
supported content.

Input and declared stream totals are bounded to 512 MiB; combined Word story
length is limited to four million UTF-16 units. Encrypted files and pre-97 binary
versions are rejected. Parsing does not execute document macros, open hyperlinks
or launch embedded objects. Open/Save continue to use the existing background
worker. The DOC filter retains its stable index; its unsupported font embedding
checkbox remains disabled.

## Progress and verification

The former bridge's “100% of adapter scope” figure has been withdrawn. The report
now tracks a fixed native capability checklist; its 21 model-supported capabilities are implemented in this source.
Partial behavior and test results earn no implementation credit.

External office programs may be used as independent verification oracles during
development. They are never called by the shipped converter. Verification results
and native Windows limitations are recorded separately in the progress report.
The current writer reserves the fifteen required fixed style indices, emits the
mandatory associated-string table, and keeps imported Normal styles distinct
from Impress's fallback. See [Word 2013 regressions](WORD-2013-REGRESSIONS.md) for
the tests and the outstanding desktop Word verification.

## References

- [Microsoft Word binary format](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/ccd7b486-7881-484c-a137-51170af7cc22)
- [rwml native Rust reader](https://github.com/HyunjoJung/rwml)
- [Vendored reader license and integration notes](../vendor/rwml/IMPRESS-VENDOR.md)

Picture preset shape crops and outlines survive DOCX and native Impress files. Binary DOC export retains rectangular crop only and reports shape/outline loss. Keep the DOCX or Impress source.
