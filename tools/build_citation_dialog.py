#!/usr/bin/env python3
"""Generate the Citation manager dialog (XRC).

Sources on the left with Add source / Remove, the citation style, locator,
preview and style presets on the right, and a New source form that opens
below the preview. The controller is crates/gui/src/ribbon/citation_ui.rs;
the vocabulary (sections, field grid, skinned buttons) is tools/dialog_kit.py.
"""
import xml.etree.ElementTree as ET
from dialog_kit import (Dialog, MUTED, button, combo, entry, field, form_grid, item, listbox, node, notext,
                        section, sizer, spacer, text)

STYLES = ['APA', 'IEEE', 'Chicago', 'MLA', 'Harvard']
# Sizes in device-independent pixels.
LIST_W, LIST_H = 260, 230
RIGHT_W = 340
PREVIEW_H = 110
FIELD_W = 220
COLUMN_GAP = 24
BUTTON_GAP = 6


def hidden(ctrl):
    ET.SubElement(ctrl, 'hidden').text = '1'
    return ctrl


def button_row(parent, buttons, top=10):
    row = sizer('wxHORIZONTAL')
    item(parent, row, 0, 'wxTOP', top)
    for index, (name, label, width) in enumerate(buttons):
        ctrl = button(name, label, width)
        item(row, ctrl, 0, 'wxALIGN_CENTER_VERTICAL' + ('|wxLEFT' if index else ''), BUTTON_GAP if index else 0)
    return row


d = Dialog('citation_manager', 'Citation Manager')
row = sizer('wxHORIZONTAL')
item(d.body, row, 1, 'wxEXPAND')

# Left: the sources of this session.
left = sizer()
item(row, left, 0, 'wxEXPAND|wxRIGHT', COLUMN_GAP)
body = section(left, 'Sources', first=True, grow=True)
item(body, entry('source_search_field', LIST_W, hint='Search sources'), 0, 'wxEXPAND|wxBOTTOM', 8)
item(body, listbox('source_list', LIST_W, LIST_H), 1, 'wxEXPAND')
button_row(body, [('add_source_button', 'Add source', 104), ('remove_source_button', 'Remove', 88)])

# Right: how the citation looks, and the form for a new source.
right = sizer()
item(row, right, 1, 'wxEXPAND')
body = section(right, 'Citation style', first=True)
grid = form_grid(body)
style = combo('citation_style_list', STYLES, FIELD_W)
ET.SubElement(style, 'value').text = STYLES[0]
field(grid, 'Style', style)
field(grid, 'Locator', entry('citation_locator_field', FIELD_W, hint='Page, chapter or section'))
presets = button_row(body, [('new_citation_style_button', 'New preset', 104)])
item(presets, hidden(entry('new_style_name_field', 120, hint='Preset name')), 1, 'wxALIGN_CENTER_VERTICAL|wxLEFT', BUTTON_GAP)
base = hidden(combo('new_style_base_combo', STYLES, 96))
ET.SubElement(base, 'value').text = STYLES[0]
item(presets, base, 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', BUTTON_GAP)
item(presets, hidden(button('add_citation_style_button', 'Add preset', 96)), 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', BUTTON_GAP)

body = section(right, 'Preview', grow=True)
preview = node(None, 'wxTextCtrl', 'citation_preview', size=f'{RIGHT_W},{PREVIEW_H}',
               style='wxTE_MULTILINE|wxTE_READONLY|wxTE_WORDWRAP',
               value='Select a source to preview its citation and bibliography entry.')
item(body, preview, 1, 'wxEXPAND')

# New source: hidden until Add source; it opens below the preview.
form = hidden(node(None, 'wxPanel', 'source_form', bg='#ffffff'))
item(right, form, 0, 'wxEXPAND')
form_sizer = sizer()
form.append(form_sizer)
body = section(form_sizer, 'New source')
grid = form_grid(body)
for caption, name in [('Author', 'source_author_field'), ('Title', 'source_title_field'),
                      ('Year', 'source_year_field'), ('Publisher', 'source_container_field')]:
    field(grid, caption, entry(name, FIELD_W))
button_row(body, [('save_source_button', 'Save source', 104), ('cancel_source_button', 'Cancel', 88)])

item(d.footer, text('Choose a source and citation style.', 'citation_hint', fg=MUTED), 0, 'wxALIGN_CENTER_VERTICAL')
d.finish('Insert citation', ok_width=124)
notext(d)
d.write('citation_dialog.xrc')
