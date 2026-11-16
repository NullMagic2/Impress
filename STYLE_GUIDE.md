# Impress style guide

Code in this repository should be easy to read and its building blocks easy to reuse. Use this guide for Rust and Python code. When it disagrees with older code, follow the guide and tidy the old code when you next touch it.

## 1. Readability comes first

- Write code for the next reader. Choose the clear version over the clever or shorter one.
- Use whole-word names: `section_index`, not `si`; `header_footer`, not `hf2`. Short names are fine only for tiny closures and loop indices (`|p| p.id`, `for i in 0..n`).
- Keep functions short and focused. If a function needs a comment to separate its "parts", those parts are probably separate functions.
- Give each line one idea. Split long expression chains and very long lines, and use `rustfmt`'s layout as the default.
- Comments explain *why* something is done, or which rule it follows (for example, "Word falls back to the default stream"). Don't restate what the code says.
- Start every module with a `//!` comment saying what it owns. Public items get a `///` comment when the name alone does not explain them.
- Prefer early returns (`let … else`, `?`) to deep nesting.

## 2. Constants must be named and reusable

- Don't scatter magic numbers or strings through the code. Every value that carries meaning (a size, colour, limit, gap, file name or path) becomes a named `const` at the top of its module, with a comment giving the unit and the reason.

  ```rust
  /// Space kept between a tall header or footer and the body it pushes away.
  const HEADER_FOOTER_BODY_GAP_PT: f32 = 6.0;
  ```

- Put the unit in the name: `_PT`, `_MM`, `_DIP`, `_PX`, `_MS`.
- Define a constant once. When two crates need it, move it to the lowest crate that both depend on (usually `core`) and import it. Don't copy it.
- Python tools follow the same rule: put an `UPPER_CASE` module-level constant near the imports (see `tools/import_yaru_icons.py`).
- Generated tables (`ribbon/generated.rs`, `ribbon/embedded.rs`) are the one exception: their numbers come from the generator's named constants.

## 3. Data types must be reusable

- Model a concept once, in the crate that owns it. Every other crate uses that type and doesn't define a near-copy. For example, `model::HeaderFooterVariant` is the only header/footer variant type: the editor re-exports it, and layout and the GUI convert to and from its compact `index()`.
- Prefer small, named types to tuples and booleans that carry meaning. A struct such as `SectionPageRange` is easier to read than a six-element tuple. An enum is clearer than `u8` codes, except where a compact code is required (such as a hit region). In that case, give the enum `index()` and `from_index()` and use them everywhere.
- Put behaviour on the type it belongs to. Iteration and lookup helpers live on the type: `HeaderFooterSet::get`, `HeaderFooterSet::for_page`, `Section::header_footers()`. Don't rebuild the same `[a, b, c].into_iter().flatten()` at every call site.
- Derive the common traits (`Clone`, `Copy` for small values, `Debug`, `PartialEq`, `Default`, and `Serialize`/`Deserialize` for persisted data) so other code can use the type without wrappers.
- Persisted model types must stay backward compatible. A new field needs `#[serde(default)]`.

## 4. Modules must be reusable

- One module, one responsibility. If you can't describe a module's job in one sentence, split it. Header and footer editing lives in `editor/src/header_footer.rs`, and its canvas chrome in `gui/src/header_footer_ui.rs`, not in a general-purpose file.
- Respect the crate layers described in [ARCHITECTURE.md](ARCHITECTURE.md): `core` ← `editor` ← `gui`, and `core` → `layout` → `render` → `output`. A lower layer never imports a higher one. GUI code never edits the document directly: it calls an `EditorState` method.
- Write generic helpers and build specific ones on them. For example, `EditorState::insert_field` and `insert_inline_equation` both call one `insert_inline_object`. Before writing a helper, search for an existing one.
- Keep public interfaces small. Default to `pub(crate)` or private, and make an item `pub` only when another crate needs it.
- Platform-independent code (`render`, `layout`, `editor`) must not depend on wxWidgets or on the operating system.

## 5. Generated files and assets

- Don't hand-edit generated files. Change the generator, then run it:
  - `tools/build_ribbon_ui.py` writes `crates/gui/ui/main.xrc`, `ribbon-manifest.json` and `ribbon/generated.rs`.
  - `tools/import_yaru_icons.py` writes every Ribbon PNG, `ribbon/embedded.rs`, the design SVGs and `docs/ribbon-icons.png`.
- Icons come from the Yaru theme. To add one, map the new Impress icon name to a Yaru command icon in `ICON_SOURCES` and run the importer. Don't draw icons by hand in Python.

## 6. Tests and checks

- Don't write new tests. When one seems needed, ask the user first and say why it matters (what could break unnoticed without it). A test name describes the expected behaviour (`tall_header_pushes_the_body_down_instead_of_overlapping_it`).
- Tests live in test folders, never in source files: a crate's unit tests in `crates/<crate>/tests/unit/<source path>.rs`, included with `#[cfg(test)] #[path = "..."] mod tests;` (`include!` inside an inline module) so they keep access to private items; integration tests directly in `crates/<crate>/tests/`; tool checks in `tools/tests/`.
- Before committing, run `cargo test --workspace` (or `check.bat` / `check.ps1`), plus the `tools/tests/test_*.py` checks for the interface you changed.
- Fix new compiler warnings. Don't silence them with `allow`.

## 7. Errors

- Return `Result` with a message a user can act on ("Place the caret in a header or footer first"). Never `unwrap()` on input or document data outside tests.
- Use `expect("reason")` only for invariants the code has just established, and give the reason.
