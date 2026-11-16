# Impress changes to pdf-extract 0.12.1

Upstream: https://github.com/jrmuizel/pdf-extract (MIT, see LICENSE).
Every change is marked with an `Impress:` comment in `src/lib.rs`.

- `OutputDev::output_styled_character` (defaults to `output_character`)
  reports each character's font name, FontDescriptor flags and fill colour
  (`TextStyle`), so imports keep bold, italic, font and colour.
- The `g`/`rg`/`k` and `G`/`RG`/`K` colour operators set the colour space
  and colour; upstream ignored them, so most text colours were lost.
- Image XObjects are decoded (`PdfImage`: JPEG bytes, or RGBA from 8-bit
  Gray/RGB/CMYK/ICC/Indexed samples with an optional soft mask) and passed
  to `OutputDev::image`, instead of being parsed as a content stream.
- The `f*`, `B`, `B*`, `b`, `b*` and `s` operators paint their path and
  end it; upstream ignored them and let the path grow into the next shape.
- `Cargo.toml` drops the dev-dependencies and the release profile.
