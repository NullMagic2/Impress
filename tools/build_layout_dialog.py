#!/usr/bin/env python3
"""Generate the picture Layout dialog (Wrap Text > More Layout Options…), as in Word.

Three tabs: Position (horizontal and vertical alignment or absolute
position, Move object with text, Lock anchor), Text Wrapping (the wrapping
styles as pictures, distance from text) and Size (height, width, lock aspect
ratio). The controller is crates/gui/src/ribbon/layout_dialog.rs; the
vocabulary is tools/dialog_kit.py.
"""
import xml.etree.ElementTree as ET
from dialog_kit import Dialog, Columns, checkbox, combo, entry, item, notext, painted, radio, section, sizer, text, with_unit

# Sizes in device-independent pixels.
NUMBER_W = 80
CHOICE_W = 120
RELATIVE_W = 120
# The radios share a width, so the controls after them line up.
RADIO_W = 150
STYLES_W, STYLES_H = 6 * 76, 92
COLUMNS = Columns(label1=140, cell1=CHOICE_W + 40)


def position_row(body, prefix, first, caption, control, relation, choices):
    """One way to place the picture along an axis: a radio, its control, and
    what it is measured from."""
    row = sizer('wxHORIZONTAL')
    item(body, row, 0, 'wxBOTTOM', 8)
    choice = radio(f'{prefix}', caption, first=first)
    ET.SubElement(choice, 'size').text = f'{RADIO_W},-1'
    item(row, choice, 0, 'wxALIGN_CENTER_VERTICAL', 0)
    item(row, control, 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', 12)
    item(row, text(relation), 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', 12)
    item(row, combo(f'{prefix}_from', choices, RELATIVE_W), 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', 8)


d = Dialog('lay', 'Layout', tabs=['Position', 'Text Wrapping', 'Size'])

page = d.pages[0]
body = section(page, 'Horizontal', first=True)
position_row(body, 'lay_h_align', True, 'Alignment', combo('lay_h_align_value', ['Left', 'Centered', 'Right'], CHOICE_W),
             'relative to', ['Margin', 'Page'])
position_row(body, 'lay_h_abs', False, 'Absolute position', with_unit(entry('lay_h_abs_value', NUMBER_W), 'cm'),
             'to the right of', ['Page', 'Margin'])
body = section(page, 'Vertical')
position_row(body, 'lay_v_align', True, 'Alignment', combo('lay_v_align_value', ['Top', 'Centered', 'Bottom'], CHOICE_W),
             'relative to', ['Margin', 'Page'])
position_row(body, 'lay_v_abs', False, 'Absolute position', with_unit(entry('lay_v_abs_value', NUMBER_W), 'cm'),
             'below', ['Page', 'Margin', 'Paragraph'])
body = section(page, 'Options')
item(body, checkbox('lay_move', 'Move object with text'), 0, 'wxBOTTOM', 6)
item(body, checkbox('lay_lock', 'Lock anchor'), 0)

page = d.pages[1]
body = section(page, 'Wrapping style', first=True)
item(body, painted('lay_styles', STYLES_W, STYLES_H), 0)
body = section(page, 'Distance from text')
grid = COLUMNS.grid(body, cols=2)
COLUMNS.field(grid, 'Distance:', with_unit(entry('lay_gap', NUMBER_W), 'cm'))

page = d.pages[2]
body = section(page, 'Size', first=True)
grid = COLUMNS.grid(body, cols=2)
COLUMNS.field(grid, 'Height:', with_unit(entry('lay_height', NUMBER_W), 'cm'))
COLUMNS.field(grid, 'Width:', with_unit(entry('lay_width', NUMBER_W), 'cm'))
item(body, checkbox('lay_ratio', 'Lock aspect ratio'), 0, 'wxTOP', 10)

d.finish('OK')
notext(d)
d.write('layout_dialog.xrc')
