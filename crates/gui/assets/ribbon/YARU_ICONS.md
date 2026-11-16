# Icon artwork

The icon rasters in this folder, and the sources in `design/icons/color` and `design/icons/sprite.svg`, come from the **Yaru** icon theme: mostly as packaged for LibreOffice (`images_yaru_svg.zip`, the `libreoffice-style-yaru` package), and for the file and clipboard commands from the system theme (`/usr/share/icons/Yaru`). Yaru icons are created by the Ubuntu Yaru team (Canonical Ltd. and contributors) and are licensed under the Creative Commons Attribution-ShareAlike 4.0 International licence (CC BY-SA 4.0): https://creativecommons.org/licenses/by-sa/4.0/

Modifications: every icon is a 64x64 PNG whose visible drawing is scaled to a common size and centred; `landscape` is the portrait page rotated 90°; the floppy disk (`save`, `save_as`) is recoloured from grey to blue, and `save_as` has the LibreOffice Yaru pencil pasted into its corner. Regenerate everything with `tools/import_yaru_icons.py`.

The exceptions are `logo.png`, the Impress brand mark, and `style_normal.png`, `style_heading1.png` and `style_title.png`, which preview paragraph styles. They are original Impress artwork under the project's MIT licence.

The CC BY-SA 4.0 licence covers only this artwork. The Impress source code stays under the MIT licence in `LICENSE.md`.
