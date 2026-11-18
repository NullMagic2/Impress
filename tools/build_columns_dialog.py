#!/usr/bin/env python3
"""Generate the Columns dialog (Layout > Columns > More Columns…), as in Word.

Presets, number of columns and "Line between" on top; a width-and-spacing
table (one row per column, scrolling past three); "Equal column width";
"Apply to" with "Start new column"; and a preview. The controller is
crates/gui/src/ribbon/columns_dialog.rs; the vocabulary is tools/dialog_kit.py.
"""
from dialog_kit import (Dialog, MUTED, checkbox, combo, entry, item, node, notext, painted, section, sizer,
                        spin, text)

# Must match editor::MAX_COLUMNS.
MAX_COLUMNS = 12
APPLY_TO = ['Whole document', 'This section', 'This point forward']
# Sizes in device-independent pixels.
PRESETS_W, PRESETS_H = 5 * 70, 96
PREVIEW_W, PREVIEW_H = 150, 196
NUMBER_W = 70
TABLE_H = 3 * 34 + 4
COLUMN_GAP = 24
ROW_GAP = 6


d = Dialog('col', 'Columns')
row = sizer('wxHORIZONTAL')
item(d.body, row, 1, 'wxEXPAND')
left = sizer()
item(row, left, 1, 'wxEXPAND|wxRIGHT', COLUMN_GAP)

body = section(left, 'Presets', first=True)
item(body, painted('col_presets', PRESETS_W, PRESETS_H), 0)
options = sizer('wxHORIZONTAL')
item(body, options, 0, 'wxTOP', 12)
item(options, text('Number of columns:'), 0, 'wxALIGN_CENTER_VERTICAL|wxRIGHT', 8)
item(options, spin('col_count', NUMBER_W, 1, MAX_COLUMNS, 1, 0, 1), 0, 'wxALIGN_CENTER_VERTICAL|wxRIGHT', 24)
item(options, checkbox('col_line', 'Line between'), 0, 'wxALIGN_CENTER_VERTICAL')

body = section(left, 'Width and spacing')
header = node(None, 'wxFlexGridSizer', rows=0, cols=3, vgap=ROW_GAP, hgap=16, growablecols='')
item(body, header, 0, 'wxBOTTOM', 4)
for caption, width in (('Col #', 44), ('Width (cm)', 110), ('Spacing (cm)', 110)):
    item(header, text(caption, fg=MUTED, size=f'{width},-1'), 0)
table = node(None, 'wxScrolledWindow', 'col_table', style='wxVSCROLL', scrollrate='0,12', size=f'-1,{TABLE_H}', bg='#ffffff')
item(body, table, 0, 'wxEXPAND')
grid = node(None, 'wxFlexGridSizer', rows=0, cols=3, vgap=ROW_GAP, hgap=16, growablecols='')
table.append(grid)
for index in range(1, MAX_COLUMNS + 1):
    item(grid, text(f'{index}:', f'col_label_{index}', size='44,-1'), 0, 'wxALIGN_CENTER_VERTICAL')
    item(grid, entry(f'col_width_{index}', 110), 0, 'wxALIGN_CENTER_VERTICAL')
    item(grid, entry(f'col_spacing_{index}', 110), 0, 'wxALIGN_CENTER_VERTICAL')
item(body, checkbox('col_equal', 'Equal column width'), 0, 'wxTOP', 10)

body = section(left, 'Apply')
apply_row = sizer('wxHORIZONTAL')
item(body, apply_row, 0)
item(apply_row, text('Apply to:'), 0, 'wxALIGN_CENTER_VERTICAL|wxRIGHT', 8)
apply_to = combo('col_apply', APPLY_TO, 170)
item(apply_row, apply_to, 0, 'wxALIGN_CENTER_VERTICAL|wxRIGHT', 24)
item(apply_row, checkbox('col_new_column', 'Start new column'), 0, 'wxALIGN_CENTER_VERTICAL')
item(body, text('', 'col_error', fg='#b42318'), 0, 'wxTOP', 8)

right = sizer()
item(row, right, 0, 'wxEXPAND')
body = section(right, 'Preview', first=True)
item(body, painted('col_preview', PREVIEW_W, PREVIEW_H, fill='#eef2f7'), 0)

d.finish('OK')
notext(d)
d.write('columns_dialog.xrc')
