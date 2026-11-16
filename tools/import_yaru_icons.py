#!/usr/bin/env python3
"""Import the Yaru icon theme for every Ribbon, dialog and canvas icon.

Most artwork comes from LibreOffice's Yaru theme (`images_yaru_svg.zip`), which
draws one icon per word-processor command. The everyday file and clipboard
commands use the system Yaru theme instead (`/usr/share/icons/Yaru`), whose
full-colour icons are drawn for small sizes and stay legible at 16-18 px.
Each Impress icon name maps to one Yaru icon below. The script writes:

* `design/icons/color/<name>.svg`  the LibreOffice Yaru source, as shipped
  (system-theme icons are copied as `<name>.png` instead);
* `design/icons/sprite.svg` and the inline sprite in
  `design/interface-reference.html`, rebuilt from the same sources;
* `crates/gui/assets/ribbon/<name>.png`  64x64 rasters (SVGs rendered with resvg);
* `crates/gui/src/ribbon/embedded.rs`    via `embed_ribbon_pngs.py`;
* the Styles toolbar data in the HTML reference via `embed_style_dialog_icons.py`;
* `docs/ribbon-icons.png`, a labelled overview of every raster.

Usage: python3 tools/import_yaru_icons.py [path/to/images_yaru_svg.zip [path/to/Yaru]]
"""
from pathlib import Path
import re
import base64
import colorsys
import io
import shutil
import subprocess
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_ARCHIVE = Path('/usr/share/libreoffice/share/config/images_yaru_svg.zip')
DEFAULT_SYSTEM_THEME = Path('/usr/share/icons/Yaru')
# The system theme's 32 px icons at double density: drawn natively at 64x64.
SYSTEM_THEME_SIZE = '32x32@2x'
COLOR_DIR = ROOT / 'design/icons/color'
SPRITE = ROOT / 'design/icons/sprite.svg'
REFERENCE_HTML = ROOT / 'design/interface-reference.html'
PNG_DIR = ROOT / 'crates/gui/assets/ribbon'
RASTERIZER = ROOT / 'tools/render_mock_icons/Cargo.toml'
OVERVIEW = ROOT / 'docs/ribbon-icons.png'
# Transparent pixels kept on each side of a 64x64 raster, so the Ribbon's
# optical padding and DPI guard band never clip the artwork.
RASTER_INSET_PX = 1

# Side of every icon raster, in pixels (the Ribbon art loader expects 64).
RASTER_SIZE_PX = 64
# Every icon's visible ink is scaled so its longer side is this many pixels
# and centred. Yaru drawings pad themselves differently; without this a row
# of icons looks uneven and some crowd their button's edges.
INK_SIZE_PX = 54
# Overview sheet layout, in pixels.
OVERVIEW_COLUMNS = 12
OVERVIEW_CELL = (100, 70)
OVERVIEW_ICON = 40
OVERVIEW_BACKGROUND = '#f1f3f6'

# Yaru draws command icons on a 32x32 grid; the 24 px (lc_) and 16 px (sc_)
# variants are fallbacks for commands that have no 32 px drawing.
SEARCH_ORDER = ('cmd/32/{}.svg', 'cmd/lc_{}.svg', 'cmd/sc_{}.svg')
LINK_DEPTH_LIMIT = 8
SPRITE_SYMBOL_PREFIX = 'i-'

# Artwork that is not a command icon. The brand mark and the style-gallery
# previews stay as they are, in both the PNG and SVG folders.
PRESERVED_PNGS = {'logo', 'style_normal', 'style_heading1', 'style_title'}

# Impress icon name -> Yaru command icon (or an archive path for resources
# outside `cmd/`). Grouped by the Ribbon page that first uses the icon.
ICON_SOURCES = {
    # File and quick-access toolbar
    'new': 'newdoc', 'new-window': 'newwindow', 'open': 'open', 'save': 'save',
    'save_as': 'saveas', 'print': 'print', 'export': 'exportdirecttopdf',
    'pdf': 'exporttopdf', 'refresh': 'refresh', 'undo': 'undo', 'redo': 'redo',
    'recent': 'recentfilelist',
    'help': 'helpindex', 'chevron-up': 'upsearch', 'caret': 'downsearch',
    'x': 'cancel', 'close': 'closedoc', 'lock': 'protect', 'trash': 'delete',
    'error': 'showerrors', 'warning': 'warningcellstyles',
    # Home: clipboard, font and paragraph
    'cut': 'cut', 'copy': 'copy', 'paste': 'paste', 'paste-text': 'pastespecial',
    'bold': 'bold', 'italic': 'italic', 'underline': 'underline',
    'grow': 'grow', 'shrink': 'shrink', 'font-color': 'fontcolor',
    'highlight': 'charbackcolor', 'clear': 'setdefault',
    'bullets': 'defaultbullet', 'numbering': 'defaultnumbering',
    'indent': 'incrementindent', 'outdent': 'decrementindent',
    'indent-right': 'hangingindent',
    'align-left': 'leftpara', 'align-center': 'centerpara',
    'align-right': 'rightpara', 'align-justify': 'justifypara',
    'line-spacing': 'linespacing', 'spacing': 'spacing', 'kern': 'spacing',
    'lig': 'fontworksameletterheights', 'ltr': 'paralefttoright',
    'rtl': 'pararighttoleft', 'dir-auto': 'textdirectionlefttoright',
    'styles': 'designerdialog', 'style-tag': 'styleapply', 'format': 'editstyle',
    'accept': 'accepttrackedchange', 'search': 'searchdialog',
    'globe': 'managelanguage',
    # Insert
    'page': 'printlayout', 'page-break': 'insertpagebreak',
    'section-break': 'insertbreak', 'column-break': 'insertcolumnbreak',
    'table': 'inserttable', 'picture': 'insertgraphic', 'image': 'graphicdialog',
    'figure': 'graphic', 'shapes': 'basicshapes', 'chart': 'insertobjectchart',
    'textbox': 'drawtext', 'equation': 'insertmath',
    'inline-eq': 'insertobjectstarmath', 'symbol': 'insertsymbol',
    'link': 'inserthyperlink', 'bookmark': 'insertbookmark',
    'crossref': 'insertreferencefield', 'field': 'insertfieldctrl',
    'calendar': 'datefield', 'plus': 'zoomin',
    # Header & footer
    'header': 'insertheader', 'footer': 'insertfooter',
    'header-footer': 'editheaderandfooter', 'header-first': 'titlepagedialog',
    'header-even': 'showtwopages', 'page-number': 'insertpagenumberfield',
    'page-count': 'insertpagecountfield',
    # Layout
    'margins': 'pagemargin', 'orientation': 'orientation',
    'portrait': 'orientation', 'landscape': 'orientation', 'size': 'attributepagesize',
    'columns': 'formatcolumns', 'page-color': 'backgroundcolor',
    'watermark': 'watermark', 'line-numbers': 'linenumberingdialog',
    'hyph': 'hyphenate', 'position': 'objectposition', 'wrap': 'wrapideal',
    'forward': 'objectforwardone', 'backward': 'objectbackone',
    'align-objects': 'objectalign', 'rotate': 'rotateright', 'tab': 'tabdialog',
    'space-before': 'paraspaceincrease', 'space-after': 'paraspacedecrease',
    # Picture tools
    'wrap-inline': 'wraptext', 'wrap-square': 'wrapon', 'wrap-tight': 'wrapcontour',
    'wrap-top-bottom': 'wrapoff', 'wrap-behind': 'setobjecttobackground',
    'wrap-front': 'setobjecttoforeground', 'anchor-move': 'toggleanchortype',
    'anchor-page': 'pagemode', 'layout-more': 'transformdialog',
    'align-object-left': 'objectalignleft', 'align-object-center': 'alignhorizontalcenter',
    'align-object-right': 'objectalignright',
    # References
    'toc': 'insertmultiindex', 'plus-lines': 'insertneutralparagraph',
    'footnote': 'insertfootnote', 'endnote': 'insertendnote',
    'references': 'footnotedialog', 'caption': 'insertcaptiondialog',
    'table-figures': 'updateallindexes', 'cite': 'insertauthoritiesentry',
    'book': 'bibliographycomponent', 'sources': 'viewdatasourcebrowser',
    'index': 'indexesmenu', 'mark': 'insertindexesentry',
    # Review
    'spell': 'spelling', 'thesaurus': 'thesaurusdialog',
    'word-count': 'wordcountdialog', 'count': 'wordcountdialog',
    'comment': 'insertannotation', 'comment-x': 'deleteannotation',
    'comment-check': 'ok', 'track': 'trackchanges', 'markup': 'showtrackedchanges',
    'reject': 'rejecttrackedchange', 'compare': 'comparedocuments',
    'review': 'trackchangesbar', 'eye': 'showchanges', 'pane-right': 'sidebarmenu',
    'arrow-up': 'upsearch', 'arrow-down': 'downsearch',
    # View
    'one-page': 'showsinglepage', 'side-by-side': 'showtwopages',
    'page-width': 'zoompagewidth', 'fit': 'zoomoptimal', 'zoom-in': 'zoomin',
    'zoom-out': 'zoomout', 'zoom-reset': 'zoom100percent', 'focus': 'fullscreen',
    'web': 'browseview', 'outline': 'setoutline', 'navigation': 'navigator',
    'sidebar': 'sidebar', 'split': 'splitwindow', 'split-h': 'splitwindow',
    # Typst
    'typst': 'sourceview', 'code': 'webhtml', 'code-block': 'preformattedparastyle',
    'compile': 'compilebasic', 'bolt': 'runbasic', 'sync': 'gotopage',
    'function': 'functiondialog', 'variable': 'definename',
    'hash': 'textattributes', 'package': 'com.sun.star.deployment.ui.packagemanagerdialog',
    'template': 'templatemanager', 'vector': 'insertdraw',
    # Styles pane and Modify Style dialog
    'style-align-left': 'leftpara', 'style-align-center': 'centerpara',
    'style-align-right': 'rightpara', 'style-align-justify': 'justifypara',
    'style-line-single': 'spacepara1', 'style-line-onehalf': 'spacepara15',
    'style-line-double': 'spacepara2',
    'style-spacing-increase': 'paraspaceincrease',
    'style-spacing-decrease': 'paraspacedecrease',
    'style-indent-increase': 'incrementindent',
    'style-indent-decrease': 'decrementindent',
    'style-border-top': 'svx/res/fr05.svg', 'style-border-bottom': 'svx/res/fr06.svg',
    'style-border-left': 'svx/res/fr02.svg', 'style-border-right': 'svx/res/fr03.svg',
    'style-border-between': 'svx/res/pr09.svg',
}

# Impress icon name -> system Yaru theme icon. These override ICON_SOURCES:
# the quick-access toolbar and clipboard need icons drawn for small sizes.
# Values are `<context>/<icon>` inside the theme's 32x32@2x folder. Open is
# the orange Yaru folder and Save the Yaru floppy disk: Yaru's own file icons
# are a grey folder and a plain down arrow.
SYSTEM_ICON_SOURCES = {
    'new': 'actions/document-new', 'open': 'places/folder-open', 'save': 'devices/media-floppy',
    'print': 'actions/document-print', 'undo': 'actions/edit-undo', 'redo': 'actions/edit-redo',
    'cut': 'actions/edit-cut', 'copy': 'actions/edit-copy', 'paste': 'actions/edit-paste',
    'search': 'actions/edit-find', 'margins': 'actions/document-page-setup',
}

# Icons recoloured by Impress name: their grey body takes this RGB colour, so
# the everyday commands are as colourful as the rest of the Ribbon. Blue Save
# stands out beside the orange Open; scissors are red, the clipboard brown.
SAVE_BLUE = (0x19, 0x7C, 0xD8)
CLIPBOARD_BROWN = (0xC4, 0x7A, 0x2C)
ICON_TINTS = {
    'save': SAVE_BLUE, 'save_as': SAVE_BLUE, 'copy': SAVE_BLUE,
    'cut': (0xD9, 0x34, 0x2B), 'paste': CLIPBOARD_BROWN, 'paste-text': CLIPBOARD_BROWN,
}
# Only near-grey pixels in this brightness range are recoloured; the label,
# highlights and outline keep their colours.
TINT_MAX_SATURATION = 0.12
TINT_BRIGHTNESS = (0.15, 0.85)
TINT_LIFT = 1.15

# New document: the Yaru page has a faint 2 px outline and a small plus that
# both vanish at 18 px. Darken and widen the outline and draw a larger
# green badge with a larger plus.
NEW_DOCUMENT = 'actions/document-new'
OUTLINE_MAX_RGB = 60          # outline pixels are translucent black
OUTLINE_WIDEN_PX = 1          # grown by this much on each side
OUTLINE_ALPHA = 105           # was about 71
PLUS_GREEN = (15, 132, 32)    # the Yaru plus badge's own green
PLUS_DIAMETER_PX = 34         # green disc; was 18
PLUS_ARM_PX = 20              # length of each bar of the plus
PLUS_BAR_PX = 6               # thickness of each bar
SUPERSAMPLE = 4               # drawing resolution for antialiased edges

# Icons Yaru lacks: a system-theme base with a LibreOffice Yaru badge pasted
# into its bottom-right corner: name -> (base, badge).
COMPOSED_ICONS = {'save_as': ('devices/media-floppy', 'editdoc')}
# Badge side as a share of the icon side.
BADGE_SCALE = 0.55

# Older code refers to some icons with underscores; these raster copies keep
# those names working.
PNG_ALIASES = {
    'align_left': 'align-left', 'align_center': 'align-center',
    'align_right': 'align-right', 'align_justify': 'align-justify',
    'auto_direction': 'dir-auto', 'font_color': 'font-color',
    'page_break': 'page-break', 'page_number': 'page-number',
    'paste_plain': 'paste-text', 'zoom_in': 'zoom-in', 'zoom_out': 'zoom-out',
    'ligatures': 'lig',
}

# Yaru has no landscape page; the portrait page turned a quarter is one.
ROTATED_ICONS = {'landscape': 90}
YARU_GRID = 32


class YaruArchive:
    """Command-name lookup inside a LibreOffice icon-theme archive."""

    def __init__(self, path):
        self.zip = zipfile.ZipFile(path)
        self.names = set(self.zip.namelist())
        self.links = {}
        if 'links.txt' in self.names:
            for line in self.zip.read('links.txt').decode('utf-8').splitlines():
                parts = line.split()
                if len(parts) == 2:
                    self.links[parts[0]] = parts[1]

    def resolve(self, member):
        for _ in range(LINK_DEPTH_LIMIT):
            if member in self.names:
                return member
            if member not in self.links:
                return None
            member = self.links[member]
        return None

    def svg(self, source):
        candidates = [source] if '/' in source else [pattern.format(source) for pattern in SEARCH_ORDER]
        for candidate in candidates:
            member = self.resolve(candidate)
            if member:
                return self.zip.read(member).decode('utf-8')
        raise KeyError(f'Yaru icon {source!r} is not in the archive')


def view_box(svg):
    found = re.search(r'viewBox="([^"]+)"', svg)
    return found.group(1) if found else f'0 0 {YARU_GRID} {YARU_GRID}'


def rotated(svg, degrees):
    """Wrap the drawing in a rotation about the centre of its view box."""
    x, y, width, height = (float(value) for value in view_box(svg).split())
    centre_x, centre_y = x + width / 2, y + height / 2
    opening = re.search(r'<svg\b[^>]*>', svg)
    body_start, body_end = opening.end(), svg.rindex('</svg>')
    return (svg[:body_start] + f'<g transform="rotate({degrees} {centre_x:g} {centre_y:g})">'
            + svg[body_start:body_end] + '</g>' + svg[body_end:])


def sprite_symbol(name, svg):
    """Turn one SVG file into a `<symbol>` with document-unique ids."""
    body = svg[re.search(r'<svg\b[^>]*>', svg).end():svg.rindex('</svg>')]
    prefix = f'{name}-'
    body = re.sub(r'\bid="([^"]+)"', lambda m: f'id="{prefix}{m.group(1)}"', body)
    body = re.sub(r'url\(#([^)]+)\)', lambda m: f'url(#{prefix}{m.group(1)})', body)
    body = re.sub(r'href="#([^"]+)"', lambda m: f'href="#{prefix}{m.group(1)}"', body)
    return f'<symbol id="{SPRITE_SYMBOL_PREFIX}{name}" viewBox="{view_box(svg)}">{body.strip()}</symbol>'


def replace_symbols(text, symbols):
    """Swap matching `<symbol id="i-…">` elements; append the new ones."""
    seen = set()
    def swap(match):
        name = match.group(1)
        seen.add(name)
        return symbols.get(name, match.group(0))
    pattern = re.compile(r'<symbol id="' + SPRITE_SYMBOL_PREFIX + r'([^"]+)".*?</symbol>', re.S)
    text = pattern.sub(swap, text)
    return text, [symbol for name, symbol in symbols.items() if name not in seen]


def fit_ink(icon):
    """Scale an icon's visible ink to INK_SIZE_PX and centre it, as the SVG
    rasterizer does for vector icons."""
    from PIL import Image
    bounds = icon.getchannel('A').point(lambda alpha: 255 if alpha >= 8 else 0).getbbox()
    canvas = Image.new('RGBA', (RASTER_SIZE_PX, RASTER_SIZE_PX))
    if bounds is None:
        return canvas
    ink = icon.crop(bounds)
    factor = INK_SIZE_PX / max(ink.width, ink.height)
    ink = ink.resize((max(1, round(ink.width * factor)), max(1, round(ink.height * factor))), Image.LANCZOS)
    canvas.alpha_composite(ink, ((RASTER_SIZE_PX - ink.width) // 2, (RASTER_SIZE_PX - ink.height) // 2))
    return canvas


def tinted(icon, rgb):
    """Recolour an icon's grey body with `rgb`, keeping its shading."""
    hue, saturation, _ = colorsys.rgb_to_hsv(*(channel / 255 for channel in rgb))
    pixels = icon.load()
    for y in range(icon.height):
        for x in range(icon.width):
            red, green, blue, alpha = pixels[x, y]
            _, grey_saturation, value = colorsys.rgb_to_hsv(red / 255, green / 255, blue / 255)
            if grey_saturation < TINT_MAX_SATURATION and TINT_BRIGHTNESS[0] < value < TINT_BRIGHTNESS[1]:
                new = colorsys.hsv_to_rgb(hue, saturation, min(1.0, value * TINT_LIFT))
                pixels[x, y] = (*(round(channel * 255) for channel in new), alpha)
    return icon


def emphasised_new_document(icon):
    """The Yaru new-document page with a bolder outline and a bigger plus."""
    from PIL import Image, ImageDraw, ImageFilter
    outline = Image.new('L', icon.size)
    for y in range(icon.height):
        for x in range(icon.width):
            red, green, blue, alpha = icon.getpixel((x, y))
            if alpha and max(red, green, blue) <= OUTLINE_MAX_RGB:
                outline.putpixel((x, y), 255)
    outline = outline.filter(ImageFilter.MaxFilter(2 * OUTLINE_WIDEN_PX + 1))
    layer = Image.new('RGBA', icon.size, (0, 0, 0, OUTLINE_ALPHA))
    icon.paste(layer, (0, 0), outline)
    # The plus: a green disc with a white cross, drawn large and shrunk for
    # smooth edges, covering the original badge in the bottom-right corner.
    big = PLUS_DIAMETER_PX * SUPERSAMPLE
    badge = Image.new('RGBA', (big, big))
    draw = ImageDraw.Draw(badge)
    draw.ellipse((0, 0, big - 1, big - 1), fill=(*PLUS_GREEN, 255))
    centre, arm, bar = big / 2, PLUS_ARM_PX * SUPERSAMPLE / 2, PLUS_BAR_PX * SUPERSAMPLE / 2
    draw.rectangle((centre - arm, centre - bar, centre + arm, centre + bar), fill='white')
    draw.rectangle((centre - bar, centre - arm, centre + bar, centre + arm), fill='white')
    badge = badge.resize((PLUS_DIAMETER_PX, PLUS_DIAMETER_PX), Image.LANCZOS)
    corner = icon.width - PLUS_DIAMETER_PX
    icon.alpha_composite(badge, (corner, corner))
    return icon


def system_png(theme, name):
    """A system-theme raster, adjusted if listed and fitted like every other icon."""
    from PIL import Image
    icon = Image.open(theme / SYSTEM_THEME_SIZE / f'{name}.png').convert('RGBA')
    if name == NEW_DOCUMENT:
        icon = emphasised_new_document(icon)
    return fit_ink(icon)


def png_symbol(name, image):
    """A sprite `<symbol>` showing a raster icon."""
    buffer = io.BytesIO()
    image.save(buffer, 'PNG')
    data = base64.b64encode(buffer.getvalue()).decode('ascii')
    side = image.width
    return (f'<symbol id="{SPRITE_SYMBOL_PREFIX}{name}" viewBox="0 0 {side} {side}">'
            f'<image width="{side}" height="{side}" href="data:image/png;base64,{data}"/></symbol>')


def rasterize(svg_dir, png_dir):
    """Render every SVG in `svg_dir` to a 64x64 PNG in `png_dir` with resvg,
    its ink fitted to INK_SIZE_PX."""
    subprocess.run(['cargo', 'run', '--quiet', '--release', '--manifest-path', str(RASTERIZER),
                    '--', str(svg_dir), str(png_dir), str(RASTER_INSET_PX), str(INK_SIZE_PX)], check=True)


def composed_png(archive, theme, base, badge):
    """A system-theme raster with a LibreOffice Yaru badge in its corner."""
    from PIL import Image
    with tempfile.TemporaryDirectory() as svg_dir, tempfile.TemporaryDirectory() as png_dir:
        (Path(svg_dir) / 'badge.svg').write_text(archive.svg(badge), encoding='utf-8')
        rasterize(svg_dir, png_dir)
        badge_image = Image.open(Path(png_dir) / 'badge.png').convert('RGBA')
    side = round(RASTER_SIZE_PX * BADGE_SCALE)
    icon = system_png(theme, base)
    offset = RASTER_SIZE_PX - RASTER_INSET_PX - side
    icon.alpha_composite(badge_image.resize((side, side), Image.LANCZOS), (offset, offset))
    return fit_ink(icon)


def raster_icons(archive, theme):
    """Icons that come as rasters: system-theme icons and composed ones."""
    icons = {name: system_png(theme, source) for name, source in SYSTEM_ICON_SOURCES.items()}
    icons.update({name: composed_png(archive, theme, *parts) for name, parts in COMPOSED_ICONS.items()})
    return {name: tinted(icon, ICON_TINTS[name]) if name in ICON_TINTS else icon for name, icon in icons.items()}


def write_sources(archive, rasters):
    """Design sources: Yaru SVGs, and PNG copies of the raster icons."""
    COLOR_DIR.mkdir(parents=True, exist_ok=True)
    for stale in list(COLOR_DIR.glob('*.svg')) + list(COLOR_DIR.glob('*.png')):
        if stale.stem not in PRESERVED_PNGS:
            stale.unlink()
    symbols = {}
    for name, source in sorted(ICON_SOURCES.items()):
        if name in rasters:
            rasters[name].save(COLOR_DIR / f'{name}.png')
            symbols[name] = png_symbol(name, rasters[name])
            continue
        svg = archive.svg(source)
        if name in ROTATED_ICONS:
            svg = rotated(svg, ROTATED_ICONS[name])
        (COLOR_DIR / f'{name}.svg').write_text(svg, encoding='utf-8')
        symbols[name] = sprite_symbol(name, svg)
    return symbols


def write_sprites(symbols):
    sprite, added = replace_symbols(SPRITE.read_text(encoding='utf-8'), symbols)
    closing = sprite.rindex('</svg>')
    sprite = sprite[:closing] + ''.join(symbol + '\n' for symbol in added) + sprite[closing:]
    SPRITE.write_text(sprite, encoding='utf-8')
    html = REFERENCE_HTML.read_bytes().decode('utf-8')
    html, _ = replace_symbols(html, symbols)
    REFERENCE_HTML.write_bytes(html.encode('utf-8'))


def write_rasters(rasters):
    from PIL import Image
    for png in PNG_DIR.glob('*.png'):
        if png.stem not in PRESERVED_PNGS:
            png.unlink()
    with tempfile.TemporaryDirectory() as staging:
        rasterize(COLOR_DIR, staging)
        for png in Path(staging).glob('*.png'):
            if png.stem in ICON_TINTS and png.stem not in rasters:
                tinted(Image.open(png).convert('RGBA'), ICON_TINTS[png.stem]).save(PNG_DIR / png.name)
            else:
                shutil.copy(png, PNG_DIR / png.name)
    for name, image in rasters.items():
        image.save(PNG_DIR / f'{name}.png')
    for alias, name in PNG_ALIASES.items():
        shutil.copy(PNG_DIR / f'{name}.png', PNG_DIR / f'{alias}.png')
    for script in ('embed_ribbon_pngs.py', 'embed_style_dialog_icons.py'):
        subprocess.run([sys.executable, str(ROOT / 'tools' / script)], check=True)


def write_overview():
    from PIL import Image, ImageDraw
    pngs = sorted(PNG_DIR.glob('*.png'))
    cell_width, cell_height = OVERVIEW_CELL
    rows = (len(pngs) + OVERVIEW_COLUMNS - 1) // OVERVIEW_COLUMNS
    sheet = Image.new('RGB', (OVERVIEW_COLUMNS * cell_width, rows * cell_height), OVERVIEW_BACKGROUND)
    draw = ImageDraw.Draw(sheet)
    for index, png in enumerate(pngs):
        x = (index % OVERVIEW_COLUMNS) * cell_width
        y = (index // OVERVIEW_COLUMNS) * cell_height
        icon = Image.open(png).convert('RGBA')
        icon.thumbnail((OVERVIEW_ICON, OVERVIEW_ICON))
        sheet.paste(icon, (x + (cell_width - icon.width) // 2, y + 4), icon)
        draw.text((x + 3, y + OVERVIEW_ICON + 12), png.stem[:16], fill='black')
    sheet.save(OVERVIEW)


def main():
    archive = YaruArchive(Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_ARCHIVE)
    theme = Path(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_SYSTEM_THEME
    rasters = raster_icons(archive, theme)
    symbols = write_sources(archive, rasters)
    write_sprites(symbols)
    write_rasters(rasters)
    write_overview()
    print(f'Imported {len(symbols)} Yaru icons and {len(PNG_ALIASES)} aliases')


if __name__ == '__main__':
    main()
