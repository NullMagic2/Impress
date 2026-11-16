# Interface design sources

`interface-reference.html` is the visual reference for the ribbon and document shell. `interface-layout.json` provides the native control layout, and `interface-tokens.json` holds shared colors. `format-dialogs.html` previews the XRC Format dialogs.

The active generators are `tools/build_ribbon_ui.py`, `tools/build_style_dialog.py`, `tools/build_format_dialogs.py`, and `tools/xrc_preview.py`. Icons come from the Yaru theme: `tools/import_yaru_icons.py` writes the SVGs in `icons/` (colour, mono and sprite), the Ribbon PNGs in `crates/gui/assets/ribbon`, and the Rust dispatch table (through `tools/embed_ribbon_pngs.py`). The executable embeds PNGs and decodes them on demand.

After changing the layout or artwork, regenerate the affected source and run the Rust checks from the repository root.
