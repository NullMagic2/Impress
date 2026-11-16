"""Shared XRC vocabulary for Impress's Word-style dialogs.

Used by tools/build_style_dialog.py (Modify Style) and
tools/build_format_dialogs.py (the Format ▾ dialogs). One set of interface
tokens and one alignment grid, so every dialog reads like the Ribbon: a white
rounded card on the pale blue-grey chrome, accent section headings with a
hairline, section bodies indented under their heading, 28-DIP fields, and
skinned footer buttons (OK is the accent default).

XRC sizes are DIPs (wxWidgets 3.1.5+). Controls get <size>, not <minsize>:
wxWidgets turns an explicit initial size into the minimum size, and <minsize>
is only honoured on sizer items.
"""
from pathlib import Path
import xml.etree.ElementTree as ET

UI = Path(__file__).resolve().parents[1] / 'crates/gui/ui'

# Interface tokens ------------------------------------------------------------
CHROME = '#e8ecf2'        # --title: dialog background, like the title/tab band
CARD = '#ffffff'          # --ribbon: the white command surface
LINE = '#dfe4ec'          # --line: card border and section rules
DIVIDER = '#d5dce7'       # toolbar group dividers (Ribbon quick-bar divider tone)
CONTEXT = '#f7f9fb'       # --context: description well
INK = '#243042'           # --ink
MUTED = '#6b778a'         # --muted
ACCENT_INK = '#1d3d78'    # --accent-ink: section headings
ERROR = '#b42318'

# Grid --------------------------------------------------------------------------
M, PAD, IND = 12, 16, 12
SECTION = 24    # space above a section heading (after the first): sections breathe
HEAD = 10       # heading to its body
ROW = 8         # between rows in a section
HGAP = 12       # label to control
FIELD_H = 28    # combo / text field height: the Home tab's font boxes are 28 DIP
BTN = 28        # toolbar button side (= FIELD_H, so toolbar rows share one height)
TIGHT = 2       # between buttons in a group
GAP = 7         # each side of a group divider
FIELD = 150     # compact field width
COLUMN_GAP = 24 # between the two field columns of a two-column grid
TAB_H = 30      # Ribbon-style tab height (tabs sit on the chrome above the card)


# Primitives --------------------------------------------------------------------
def node(parent, cls, name=None, **props):
    attrs = {'class': cls, **({'name': name} if name else {})}
    obj = ET.SubElement(parent, 'object', attrs) if parent is not None else ET.Element('object', attrs)
    for key, value in props.items():
        ET.SubElement(obj, key).text = str(value)
    return obj


def sizer(orient='wxVERTICAL'):
    return node(None, 'wxBoxSizer', orient=orient)


def item(parent, child, option=0, flag='wxEXPAND', border=0):
    entry = node(parent, 'sizeritem', option=option, flag=flag, border=border)
    entry.append(child)
    return entry


def spacer(parent, width, height=1, option=0):
    node(parent, 'spacer', size=f'{width},{height}', option=option)


def text(value, name=None, fg=INK, bold=False, **props):
    ctrl = node(None, 'wxStaticText', name, label=value, **props)
    ET.SubElement(ctrl, 'fg').text = fg
    if bold:
        font = ET.SubElement(ctrl, 'font')
        ET.SubElement(font, 'weight').text = 'bold'
    return ctrl


def tip(ctrl, value):
    ET.SubElement(ctrl, 'tooltip').text = value
    return ctrl


def combo(name, choices, width=-1, editable=False, disabled=False):
    props = {'size': f'{width},{FIELD_H}'}
    if not editable:
        props['style'] = 'wxCB_READONLY'
    ctrl = node(None, 'wxComboBox', name, **props)
    content = ET.SubElement(ctrl, 'content')
    for value in choices:
        ET.SubElement(content, 'item').text = value
    if not editable and choices:
        ET.SubElement(ctrl, 'selection').text = '0'
    if disabled:
        ET.SubElement(ctrl, 'enabled').text = '0'
    return ctrl


def entry(name, width=-1, hint=None):
    ctrl = node(None, 'wxTextCtrl', name, size=f'{width},{FIELD_H}')
    return tip(ctrl, hint) if hint else ctrl


def colour_combo(name, width=FIELD, hint=None):
    """Swatch drop-down (wxBitmapComboBox); Rust fills it with colour chips."""
    ctrl = node(None, 'wxBitmapComboBox', name, size=f'{width},{FIELD_H}', style='wxCB_READONLY')
    return tip(ctrl, hint) if hint else ctrl


def spin(name, width=90, minimum=0, maximum=100, increment=1, digits=0, value=0):
    """Word-style spin box (wxSpinCtrlDouble); the unit sits in a label after it."""
    return node(None, 'wxSpinCtrlDouble', name, size=f'{width},{FIELD_H}', min=minimum, max=maximum,
                inc=increment, digits=digits, value=value)


def checkbox(name, label):
    return node(None, 'wxCheckBox', name, label=label)


def radio(name, label, first=False):
    ctrl = node(None, 'wxRadioButton', name, label=label)
    if first:
        ET.SubElement(ctrl, 'style').text = 'wxRB_GROUP'
    return ctrl


def listbox(name, width, height, choices=()):
    ctrl = node(None, 'wxListBox', name, size=f'{width},{height}', style='wxLB_SINGLE')
    if choices:
        content = ET.SubElement(ctrl, 'content')
        for value in choices:
            ET.SubElement(content, 'item').text = value
    return ctrl


def button(name, label, width=88):
    return node(None, 'wxButton', name, label=label, size=f'{width},{FIELD_H}')


# Rust paints the whole face of a skinned button, caption included (as in the
# Ribbon), so the native frame and caption must not show around it.
SKINNED = 'wxBU_EXACTFIT|wxBU_NOTEXT|wxBORDER_NONE'


def skinned(name, label, width=88):
    b = button(name, label, width)
    ET.SubElement(b, 'style').text = SKINNED
    return b


def notext(dialog):
    """Rust skins every button with a painted face that includes its caption
    (as the Ribbon does); the native caption stays for accessibility only."""
    for b in dialog.root.iter('object'):
        if b.get('class') == 'wxButton':
            style = b.find('style')
            if style is None:
                style = ET.SubElement(b, 'style')
            style.text = SKINNED


def rounded(name, fill, **props):
    """A surface whose background is its fill colour. Rust paints its rounded
    1-DIP border and the corners in the parent colour; children keep clear of
    the corners through their padding."""
    return node(None, 'wxPanel', name, bg=fill, **props)


def painted(name, width, height, fill=CARD):
    """A Rust-painted surface (previews, tile pickers)."""
    return node(None, 'wxPanel', name, bg=fill, size=f'{width},{height}',
                style='wxWANTS_CHARS|wxFULL_REPAINT_ON_RESIZE')


def rule(colour=LINE, vertical=False, length=18):
    size = f'1,{length}' if vertical else '-1,1'
    return node(None, 'wxPanel', size=size, bg=colour)


# Layout ------------------------------------------------------------------------
def section(parent, caption, first=False, name=None, grow=False):
    """Heading: accent caption and a hairline to the card edge.
    Returns the body sizer, indented by IND."""
    head = sizer('wxHORIZONTAL')
    item(parent, head, 0, 'wxEXPAND' if first else 'wxEXPAND|wxTOP', 0 if first else SECTION)
    item(head, text(caption, name, fg=ACCENT_INK, bold=True), 0, 'wxALIGN_CENTER_VERTICAL|wxRIGHT', 10)
    item(head, rule(), 1, 'wxALIGN_CENTER_VERTICAL')
    indent = sizer('wxHORIZONTAL')
    item(parent, indent, 1 if grow else 0, 'wxEXPAND|wxTOP', HEAD)
    spacer(indent, IND)
    body = sizer()
    item(indent, body, 1, 'wxEXPAND')
    return body


def form_grid(parent, cols=2, growable='1', top=0):
    grid = node(None, 'wxFlexGridSizer', rows=0, cols=cols, vgap=ROW, hgap=HGAP, growablecols=growable)
    item(parent, grid, 0, 'wxEXPAND|wxTOP' if top else 'wxEXPAND', top)
    return grid


def field(grid, caption, control, expand=True, column_gap=0, label_w=None, cell_w=None):
    """Label + control in a form grid. label_w / cell_w pin the column widths so
    columns line up across the sections of a dialog (see Columns)."""
    flag = 'wxALIGN_CENTER_VERTICAL|wxLEFT' if column_gap else 'wxALIGN_CENTER_VERTICAL'
    label = text(caption, size=f'{label_w},-1') if label_w else text(caption)
    item(grid, label, 0, flag, column_gap)
    entry_ = item(grid, control, 0, 'wxEXPAND|wxALIGN_CENTER_VERTICAL' if expand else 'wxALIGN_CENTER_VERTICAL')
    if cell_w:
        ET.SubElement(entry_, 'minsize').text = f'{cell_w},-1'


class Columns:
    """One column spec for every form grid of a dialog: the first label column,
    the first control column and the second label column have fixed widths, so
    labels and fields align from section to section (Word's dialogs do this)."""

    def __init__(self, label1, cell1, label2=0):
        self.label1, self.cell1, self.label2 = label1, cell1, label2

    def grid(self, parent, cols=4, top=0):
        return form_grid(parent, cols=cols, growable='', top=top)

    def field(self, grid, caption, control, column=0):
        if column == 0:
            field(grid, caption, control, expand=False, label_w=self.label1, cell_w=self.cell1)
        else:
            field(grid, caption, control, expand=False, column_gap=COLUMN_GAP, label_w=self.label2)


def with_unit(control, unit, name=None):
    """A control followed by its unit label, as one grid cell."""
    row = sizer('wxHORIZONTAL')
    item(row, control, 0, 'wxALIGN_CENTER_VERTICAL')
    item(row, text(unit, name, fg=MUTED), 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', 6)
    return row


def pair(first, second, gap=8):
    """Two controls side by side in one grid cell (e.g. "Spacing" + "By")."""
    row = sizer('wxHORIZONTAL')
    item(row, first, 0, 'wxALIGN_CENTER_VERTICAL')
    item(row, second, 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', gap)
    return row


def labelled(caption, control):
    """Caption above its control (Word's Font / Font style / Size columns)."""
    col = sizer()
    item(col, text(caption), 0, 'wxBOTTOM', 4)
    item(col, control, 1, 'wxEXPAND')
    return col


def checks(parent, rows, top=0):
    """A grid of check boxes / radio buttons: rows of controls in columns."""
    cols = max(len(r) for r in rows)
    grid = node(None, 'wxFlexGridSizer', rows=0, cols=cols, vgap=ROW, hgap=COLUMN_GAP + 8, growablecols='')
    item(parent, grid, 0, 'wxEXPAND|wxTOP' if top else 'wxEXPAND', top)
    for r in rows:
        for control in r:
            item(grid, control, 0, 'wxALIGN_CENTER_VERTICAL')
        for _ in range(cols - len(r)):
            spacer(grid, 1)
    return grid


def tool(row, name, fallback, hint, first):
    b = node(None, 'wxButton', name, label=fallback, size=f'{BTN},{BTN}',
             style='wxBU_EXACTFIT|wxBORDER_NONE', bg=CARD)
    item(row, tip(b, hint), 0, 'wxALIGN_CENTER_VERTICAL' if first else 'wxALIGN_CENTER_VERTICAL|wxLEFT',
         0 if first else TIGHT)


def divider(row):
    item(row, rule(DIVIDER, vertical=True), 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT|wxRIGHT', GAP)


# Dialog skeleton -----------------------------------------------------------------
class Dialog:
    """wxDialog on the chrome: optional Ribbon-style tabs, a white rounded card
    (one page panel per tab) and a footer with OK / Cancel on the card's edge.

    Names: <prefix>_dialog, <prefix>_tab_<i>, <prefix>_card, <prefix>_page_<i>,
    <prefix>_ok, <prefix>_cancel. Rust (format_dialogs.rs) finds them by name."""

    def __init__(self, prefix, title, tabs=(), resizable=False):
        self.prefix = prefix
        self.root = ET.Element('resource')
        style = 'wxDEFAULT_DIALOG_STYLE' + ('|wxRESIZE_BORDER' if resizable else '')
        self.dialog = node(self.root, 'wxDialog', f'{prefix}_dialog', title=title, style=style, bg=CHROME)
        self.outer = sizer()
        self.dialog.append(self.outer)
        if tabs:
            strip = sizer('wxHORIZONTAL')
            item(self.outer, strip, 0, 'wxLEFT|wxRIGHT|wxTOP', M)
            spacer(strip, 8)
            for index, caption in enumerate(tabs):
                width = max(88, 16 + 7 * len(caption))
                item(strip, node(None, 'wxButton', f'{prefix}_tab_{index}', label=caption,
                                 size=f'{width},{TAB_H}', style='wxBU_EXACTFIT|wxBORDER_NONE', bg=CHROME),
                     0, 'wxALIGN_BOTTOM')
        card = rounded(f'{prefix}_card', CARD)
        item(self.outer, card, 1, 'wxEXPAND|wxLEFT|wxRIGHT' + ('' if tabs else '|wxTOP'), M)
        card_sizer = sizer()
        card.append(card_sizer)
        self.body = sizer()
        item(card_sizer, self.body, 1, 'wxEXPAND|wxALL', PAD)
        self.pages = []
        for index in range(max(1, len(tabs))):
            if tabs:
                # Only the first page starts visible: XRC fits the window to
                # its visible content, and GTK keeps that first size, so all
                # pages visible at once made a window as tall as all of them.
                page = node(None, 'wxPanel', f'{prefix}_page_{index}', bg=CARD, **({'hidden': '1'} if index else {}))
                item(self.body, page, 1, 'wxEXPAND')
                page_sizer = sizer()
                page.append(page_sizer)
                self.pages.append(page_sizer)
            else:
                self.pages.append(self.body)
        self.footer = sizer('wxHORIZONTAL')
        item(self.outer, self.footer, 0, 'wxEXPAND|wxALL', M)
        self._left = 0

    def footer_left(self, name, label, width=96):
        item(self.footer, skinned(name, label, width), 0,
             'wxALIGN_CENTER_VERTICAL' + ('|wxLEFT' if self._left else ''), 8 if self._left else 0)
        self._left += 1

    def finish(self, ok_label='OK', cancel=True, cancel_label='Cancel', ok_width=88):
        spacer(self.footer, 1, 1, 1)
        ok = skinned(f'{self.prefix}_ok', ok_label, ok_width)
        ET.SubElement(ok, 'default').text = '1'
        item(self.footer, ok, 0, 'wxALIGN_CENTER_VERTICAL')
        if cancel:
            item(self.footer, skinned(f'{self.prefix}_cancel', cancel_label), 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', 8)

    def write(self, filename):
        write(self.root, filename)


def write(root, filename):
    from localization_kit import name_translatable_controls
    name_translatable_controls(root)
    ET.indent(root, space='  ')
    target = UI / filename
    target.write_bytes(ET.tostring(root, encoding='utf-8', xml_declaration=True))
    print(target)
