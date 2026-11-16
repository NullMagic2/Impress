# First-class Typst equation model

An equation is represented by `core::equation::Equation` and an `EquationNode` tree.

The AST supports rows, identifiers, numbers, literal text, symbols, operators, relations, fractions, radicals, superscripts, subscripts, combined scripts, matrices, delimiters, functions, accents, spacing, and a `RawTypst` escape hatch.

This gives us three important properties:

1. The visual equation editor edits semantic nodes directly.
2. Typst source can be generated consistently from the same AST.
3. DOCX export can map the same AST to Office Math Markup Language (OMML), without converting equations to images.

`RawTypst` exists for expressions that the visual builder does not understand yet. The UI can render it through Typst while clearly marking it as source-only until the builder gains support.

Example:

```text
Fraction(
  numerator = Row[x, +, 1],
  denominator = Radical(Row[x, -, 1])
)
```

serializes to structured Typst equation source approximately as:

```typst
$(x + 1) / sqrt(x - 1)$
```

## DOCX / OMML mapping

`docx` owns the Word equation adapter. OMML never enters `core` directly. On import, common Word structures are converted to native `EquationNode`s: fractions, radicals, sub/superscripts, matrices, delimiters, functions, accents, n-ary operators, upper/lower limits, prescripts, bars, group characters, equation arrays, boxes and phantoms. Export performs the inverse mapping.

Unsupported or future constructs should be preserved by the DOCX adapter rather than silently flattened; the native AST can be extended without coupling it to OOXML tag names.

## Vector rendering in the graphical editor

The graphical editor does not rasterize equations and does not round-trip them through SVG. `typst-engine` compiles the native `EquationNode` through Typst and walks Typst's laid-out frame directly. Text items become positioned vector glyph outlines, shapes become Impress vector paths, and nested Typst groups preserve their transforms. The resulting `VectorFragment` is inserted into the same page scene as prose.

This keeps equation geometry resolution-independent throughout Impress and means zooming, printing, and future PDF export can share the same vector representation. SVG remains available only as an export/debug representation; it is not the graphical editor's equation rendering path.

Equation compilation is also part of the multicore preparation stage. Independent equations and paragraphs may be prepared concurrently, while ordered pagination remains deterministic. Stale layout generations are discarded before presentation.

## Inline equations

Inline equations are semantic objects inside a paragraph, not placeholder text in the saved document. During paragraph preparation they are compiled through Typst, converted directly to an Impress `VectorFragment`, and inserted into the prose compositor as an indivisible box with explicit Typst baseline metrics. This lets surrounding prose wrap around the equation naturally while preserving vector geometry and preventing pdfTeX-style prose glyph expansion from distorting the math.

## Visual equation editor

The docked equation pane (`crates/gui/src/equation_editor`) is a structural editor. `tree.rs` holds rows of atoms and structures whose slots are rows. It converts to and from `EquationNode`, and keeps constructs it cannot edit, such as limits and prescripts, as whole items. `layout.rs` lays the formula out in New Computer Modern Math, the font Typst uses, and records the geometry of every slot and caret stop. `mod.rs` paints the pane: the formula card, the palette and the commands. Every edit updates the equation draft, so the page preview follows as you type.

Typing builds structure:

| Input | Result |
| --- | --- |
| `/` | fraction; the preceding term becomes the numerator |
| `^`, `_` | superscript, subscript (`x_i^2` puts both on `x`) |
| `(`, `[`, `{`, `|` | delimiters, closed by typing the closing character |
| `"` | upright text |
| `\name` | symbols and structures (`\alpha`, `\sum`, `\sqrt`, `\to`, …), completed with Space, Enter or Tab |
| `sin`, `log`, `lim`, … | upright operator names |
| `<=`, `>=`, `!=`, `->`, `=>` | ≤, ≥, ≠, →, ⇒ |

Arrow keys move through slots (Up/Down between numerator and denominator, scripts, limits and matrix rows), Tab moves to the next slot, Backspace steps into a structure before removing it, and Ctrl+Z/Ctrl+Y undo within the pane. The equation in the document follows every change (`gui/src/equation_ui.rs`): each change is laid out first and committed once Typst has rendered it, so an equation Typst cannot render never reaches the document, which keeps the last good one. A new equation goes in at the caret once it has content. While the pane is open, edits of one equation are one undo step. The pane has no Insert or Cancel; its × or Escape closes it. The Source toggle shows the same equation as Typst math (`$x$` inline, `$ x $` display), and Typst typed there updates the visual editor. `format_typst::parse_math_source` parses it with Typst's own parser: symbols such as `alpha` and `arrow.r` resolve through Typst's `sym` names, `a/b` is a stacked fraction, `sum_(k=1)^n k` a large operator with its body, and math functions without an AST node (`binom`, `cases`, `cancel`, …) are kept as `RawTypst`. Embedded code (`#…`) and calls of anything but math functions are refused.

## Typst math in the text

Typing the closing `$` of `$…$` in a paragraph turns valid Typst math into an equation, inline for `$x$` and display for `$ x $`. Spaces must be inside both dollars or neither, so "$5 and $10" stays text, and `\$` stays a dollar sign. Ctrl+Z gives the typed source back.

## Readable inline fractions

Inline equations draw fractions as LaTeX's `\dfrac` does (`readable_inline` in `typst-engine`): the outermost fraction's numerator and denominator are full size rather than Typst's inline script size, and a fraction within it gets script size rather than the second, smaller one. Fractions in scripts stay small. Only rendering changes; the equation's Typst source is as written.

## Nested fractions

The main bar of a fraction whose numerator or denominator is a fraction reaches well past the inner bars, both in the equation editor and on the page (a show rule in the equation renderer's preamble).
