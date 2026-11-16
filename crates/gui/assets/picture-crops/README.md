# Picture crop picker icons

95 matching crop choices use the Yaru shape command artwork distributed with LibreOffice. The 81 Word presets without a matching LibreOffice icon keep an exact geometry preview, drawn with neutral white fill and charcoal outlines. All 176 crop choices and their document geometry are unchanged.

Source package: `libreoffice-style-yaru`, Ubuntu version `4:26.2.6.3-0ubuntu0.26.04.2`.
Archive: `images_yaru_svg.zip`.
Archive SHA-256: `46f74828384e2b004ef61a2fb9de8cca2d6e8e5135dd23e7eb3b90fe29e14c04`.
Upstream: https://github.com/LibreOffice/core/tree/master/icon-themes/yaru
Yaru artwork: https://github.com/ubuntu/yaru

The SVG originals are preserved in `design/icons/picture-crops/`. `design/picture-shape-icons.json` records the LibreOffice command for each matching preset. Importing resolves the archive's own icon aliases. No shape is assigned an unrelated generic icon. Each PNG is normalized to the same 64px transparent canvas and 54px ink size used by Impress's toolbar, without recolouring the LibreOffice drawing.

Artwork attribution: Ubuntu Yaru team, Canonical Ltd. and contributors; LibreOffice contributors. The Yaru artwork is covered by CC BY-SA 4.0, https://creativecommons.org/licenses/by-sa/4.0/. See `YARU-COPYRIGHT`, `LIBREOFFICE-COPYRIGHT` and `MPL-2.0.txt` for the packaged copyright and license notices; the LibreOffice distribution also includes MPL-2.0 and Apache-2.0 material. These notices cover artwork only; Impress code retains its existing license.

Regenerate from the preserved SVGs with `python3 tools/import_picture_shape_icons.py`. To refresh the originals, pass an `images_yaru_svg.zip` archive as the argument. The icon sources are bundled in Impress; opening the picker does not require LibreOffice to be installed.
