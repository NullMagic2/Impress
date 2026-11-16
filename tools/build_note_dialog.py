#!/usr/bin/env python3
"""Generate the Footnotes and Endnotes dialog (XRC).

The dialog lists the document's notes in reading order beside a preview of the
selected one, with its actions (go to the reference, edit on the page, delete),
and a form that inserts a new footnote or endnote at the caret. The controller
is crates/gui/src/ribbon/note_dialog.rs; the vocabulary is tools/dialog_kit.py.
"""
from dialog_kit import Dialog, MUTED, button, item, listbox, node, notext, radio, section, sizer, text

# Sizes in device-independent pixels.
LIST_W, LIST_H = 330, 250
SIDE_W = 300
PREVIEW_H = 96
NOTE_TEXT_H = 84
COLUMN_GAP = 24
ACTION_GAP = 6


def multiline(name, height, read_only=False):
    style = 'wxTE_MULTILINE|wxTE_WORDWRAP' + ('|wxTE_READONLY' if read_only else '')
    return node(None, 'wxTextCtrl', name, size=f'{SIDE_W},{height}', style=style)


d = Dialog('note', 'Footnotes and Endnotes')
row = sizer('wxHORIZONTAL')
item(d.body, row, 1, 'wxEXPAND')

# Left: the notes already in the document, with actions for the selected one.
left = sizer()
item(row, left, 1, 'wxEXPAND|wxRIGHT', COLUMN_GAP)
body = section(left, 'In this document', first=True, grow=True)
item(body, listbox('note_list', LIST_W, LIST_H), 1, 'wxEXPAND|wxBOTTOM', 10)
item(body, text('No footnotes or endnotes yet.', 'note_empty', fg=MUTED), 0, 'wxBOTTOM', 8)
actions = sizer('wxHORIZONTAL')
item(body, actions, 0)
for index, (name, label, width) in enumerate([('note_goto', 'Go to reference', 120), ('note_edit', 'Edit on page', 108), ('note_delete', 'Delete', 80)]):
    item(actions, button(name, label, width), 0, 'wxLEFT' if index else '', ACTION_GAP if index else 0)

# Right: what the selected note says, then the form for a new note.
right = sizer()
item(row, right, 0, 'wxEXPAND')
body = section(right, 'Selected note', first=True)
item(body, multiline('note_preview', PREVIEW_H, read_only=True), 0, 'wxEXPAND')
body = section(right, 'Insert at the caret')
item(body, radio('note_kind_footnote', 'Footnote, at the bottom of the page', first=True), 0, 'wxBOTTOM', 6)
item(body, radio('note_kind_endnote', 'Endnote, at the end of the document'), 0, 'wxBOTTOM', 12)
item(body, text('Note text', 'note_text_caption', fg=MUTED), 0, 'wxBOTTOM', 6)
item(body, multiline('note_text', NOTE_TEXT_H), 0, 'wxEXPAND')

d.finish('Insert', cancel_label='Close')
notext(d)
d.write('note_dialog.xrc')
