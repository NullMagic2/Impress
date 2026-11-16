# Typography architecture

Impress's body-text renderer is designed around the same ideas that make TeX-family output attractive while using modern OpenType technology.

## Pipeline

1. **Resolve a font face** without putting platform font handles into the document model.
2. **Shape Unicode text with OpenType** using `rustybuzz`. GSUB/GPOS handles standard/contextual ligatures, kerning, combining-mark placement, script shaping, and font positioning.
3. **Compose the paragraph globally** with a Knuth–Plass-inspired line breaker rather than greedy word wrapping.
4. **Apply microtypography** during composition, including optical margin alignment and conservative glyph expansion.
5. **Convert shaped glyph IDs to vector outlines** while retaining fractional typographic coordinates.
6. **Place those outlines into the vector page scene.** The wxDragon canvas paints vector page commands rather than cached glyph/page bitmaps.
7. **Render equations separately through Typst.** Equations are semantic `EquationNode`s in `core`; Typst is their typesetting engine, not the storage format for the surrounding document.

## Current defaults

Normal prose enables kerning (`kern`), standard ligatures (`liga`), contextual ligatures (`clig`), contextual alternates (`calt`), and mark-to-base/mark-to-mark positioning (`mark`, `mkmk`) when the selected font provides them. Discretionary ligatures are opt-in.

Optical margin alignment is enabled by default. Punctuation can protrude slightly into the margin so visible text edges align more naturally.

Glyph expansion is intentionally conservative. The default maximum is approximately ±1.5%, allowing paragraph composition to reduce visibly loose or tight justified lines without obvious character distortion.

The document model stores typography intent rather than shaped glyph IDs or final positions. A saved document can therefore be laid out again when fonts, page geometry, or renderer capabilities change.

## Vector text

Impress does not use a glyph texture atlas or complete page bitmap as its canonical rendered representation. Shaping produces glyph IDs and fractional advances; `render` retrieves their font outlines and stores Bézier geometry in the vector scene.

Zoom therefore transforms the same vector geometry rather than scaling a cached text bitmap. PDF/print exporters can consume the same vector layout path later.

## Implemented in paginated layout

The Knuth–Plass-inspired compositor, optical margin alignment, and conservative glyph expansion are now used by `layout` when building real page lines. Mixed-format text runs retain their own resolved character properties through composition, and inline Typst equations enter the same line breaker as baseline-aware atomic vector boxes. Paragraph preparation runs on the background multicore scheduler and emits source-aware vector text hit regions for caret/selection. Underlines are emitted as vector page geometry.

## Next typography work

- Knuth–Liang language-aware hyphenation.
- Per-font optical-margin profiles instead of the current conservative punctuation table.
- Optical-size (`opsz`) variable-font selection.
- Font fallback by script and missing glyph rather than by entire paragraph.
- Visual-line-aware vertical caret navigation and more advanced BiDi-aware source mapping.
- Per-font tuning of glyph-expansion limits and penalties.

These features belong to `typography` and `layout`, never to a DOCX/Typst/native format crate or the wxDragon GUI.

