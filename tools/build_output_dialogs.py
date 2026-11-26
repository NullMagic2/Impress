#!/usr/bin/env python3
"""Generate the Export PDF, Import PDF and Print dialogs (XRC).

Each dialog is a page preview on the left (painted in Rust,
crates/gui/src/output_dialogs.rs) beside the options card, in the same
vocabulary as the Format dialogs (tools/dialog_kit.py).
"""
import xml.etree.ElementTree as ET
from dialog_kit import (Dialog, MUTED, button, notext, checkbox, combo, entry, item, node, painted, radio, section, with_unit,
                        sizer, spacer, spin, text, tip, Columns, form_grid, field)

PREVIEW_W, PREVIEW_H = 250, 340


def preview_column(d, name, caption):
    column = sizer()
    item(d.row, column, 0, 'wxEXPAND|wxRIGHT', 20)
    item(column, text(caption, f'{name}_caption', fg=MUTED), 0, 'wxBOTTOM', 6)
    item(column, painted(name, PREVIEW_W, PREVIEW_H, fill='#eef2f7'), 1, 'wxEXPAND')
    return column


def start(prefix, title):
    d = Dialog(prefix, title)
    d.row = sizer('wxHORIZONTAL')
    item(d.body, d.row, 1, 'wxEXPAND')
    d.options = sizer()
    return d


def pages_section(d, prefix, current=True):
    body = section(d.options, 'Pages', first=False)
    item(body, radio(f'{prefix}_all', 'All pages', first=True), 0, 'wxBOTTOM', 6)
    if current:
        item(body, radio(f'{prefix}_current', 'Current page'), 0, 'wxBOTTOM', 6)
    row = sizer('wxHORIZONTAL')
    item(body, row, 0, 'wxEXPAND')
    item(row, radio(f'{prefix}_range', 'Pages'), 0, 'wxALIGN_CENTER_VERTICAL|wxRIGHT', 10)
    pages = tip(entry(f'{prefix}_pages', 150), 'Page numbers and ranges, for example 1, 3-5')
    ET.SubElement(pages, 'hint').text = 'e.g. 1, 3-5'
    item(row, pages, 1, 'wxALIGN_CENTER_VERTICAL')
    item(body, text('All 1 page', f'{prefix}_summary', fg=MUTED), 0, 'wxTOP', 8)


# Export PDF -------------------------------------------------------------------
d = start('pdf_export', 'Export PDF')
preview_column(d, 'pdf_export_preview', 'Preview')
item(d.row, d.options, 1, 'wxEXPAND')
body = section(d.options, 'Save to', first=True)
row = sizer('wxHORIZONTAL')
item(body, row, 0, 'wxEXPAND')
item(row, entry('pdf_export_path', 300), 1, 'wxALIGN_CENTER_VERTICAL')
item(row, button('pdf_export_browse', 'Browse…', 92), 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', 8)
pages_section(d, 'pdf_export')
body = section(d.options, 'Document properties')
grid = form_grid(body)
for caption, name in [('Title', 'pdf_export_title'), ('Author', 'pdf_export_author'),
                      ('Subject', 'pdf_export_subject'), ('Keywords', 'pdf_export_keywords')]:
    field(grid, caption, entry(name, 260))
body = section(d.options, 'Options')
item(body, checkbox('pdf_export_links', 'Keep links and cross-references clickable'), 0, 'wxBOTTOM', 6)
item(body, checkbox('pdf_export_open', 'Open the PDF when the export finishes'), 0)
d.finish('Export')
notext(d)
d.write('pdf_export.xrc')

# Import PDF -------------------------------------------------------------------
d = start('pdf_import', 'Import PDF')
preview_column(d, 'pdf_import_preview', 'Result preview')
item(d.row, d.options, 1, 'wxEXPAND')
body = section(d.options, 'File', first=True)
row = sizer('wxHORIZONTAL')
item(body, row, 0, 'wxEXPAND')
info = sizer()
item(row, info, 1, 'wxALIGN_CENTER_VERTICAL')
item(info, text('document.pdf', 'pdf_import_file', bold=True), 0, 'wxBOTTOM', 2)
item(info, text('', 'pdf_import_info', fg=MUTED), 0)
item(row, button('pdf_import_browse', 'Choose…', 92), 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', 8)
pages_section(d, 'pdf_import', current=False)
body = section(d.options, 'Text')
item(body, radio('pdf_import_reflow', 'Join lines into flowing paragraphs', first=True), 0, 'wxBOTTOM', 6)
item(body, radio('pdf_import_keep', "Keep the PDF's line breaks"), 0)
item(body, checkbox('pdf_import_columns', 'Keep page columns'), 0, 'wxTOP', 8)
body = section(d.options, 'Clean-up')
for name, label in [('pdf_import_headings', 'Turn larger text into headings'),
                    ('pdf_import_running', 'Move page numbers and running headers into the header and footer'),
                    ('pdf_import_hyphens', 'Rejoin words hyphenated at line ends'),
                    ('pdf_import_breaks', 'Start each PDF page on a new page')]:
    item(body, checkbox(name, label), 0, 'wxBOTTOM', 6)
item(d.options, text('Fonts, lists, tables, columns and pictures are kept; the layout may differ from the PDF.', 'pdf_import_note', fg=MUTED), 0, 'wxTOP', 10)
d.finish('Import')
notext(d)
d.write('pdf_import.xrc')

# Print --------------------------------------------------------------------------
d = start('print', 'Print')
preview_column(d, 'print_preview', 'Preview')
item(d.row, d.options, 1, 'wxEXPAND')
body = section(d.options, 'Printer', first=True)
item(body, text('You choose the printer and paper in the next step.', 'print_printer', fg=MUTED), 0)
pages_section(d, 'print')
body = section(d.options, 'Copies')
row = sizer('wxHORIZONTAL')
item(body, row, 0, 'wxEXPAND')
item(row, spin('print_copies', 120, 1, 99, 1, 0, 1), 0, 'wxALIGN_CENTER_VERTICAL')
item(row, checkbox('print_collate', 'Collate'), 0, 'wxALIGN_CENTER_VERTICAL|wxLEFT', 16)
body = section(d.options, 'Scale')
item(body, radio('print_fit', 'Fit each page to the paper', first=True), 0, 'wxBOTTOM', 6)
item(body, radio('print_actual', 'Actual size (100%)'), 0)
d.finish('Print…')
notext(d)
d.write('print_dialog.xrc')

# Table position (floating tables) -------------------------------------------------
d = start('tpos', 'Table Position')
column = sizer()
item(d.row, column, 0, 'wxEXPAND|wxRIGHT', 20)
item(column, text('Preview', 'tpos_preview_caption', fg=MUTED), 0, 'wxBOTTOM', 6)
item(column, painted('tpos_preview', 190, 250, fill='#eef2f7'), 1, 'wxEXPAND')
item(d.row, d.options, 1, 'wxEXPAND')
cols = Columns(78, 150, 78)
body = section(d.options, 'Horizontal', first=True)
grid = cols.grid(body)
cols.field(grid, 'Position', combo('tpos_h_mode', ['Offset', 'Left', 'Center', 'Right', 'Inside', 'Outside'], 150))
cols.field(grid, 'Offset', with_unit(spin('tpos_h_value', 130, -500, 500, 0.5, 1, 0), 'mm'), 1)
cols.field(grid, 'Relative to', combo('tpos_h_from', ['Margin', 'Page', 'Column'], 150))
spacer(grid, 1); spacer(grid, 1)
body = section(d.options, 'Vertical')
grid = cols.grid(body)
cols.field(grid, 'Position', combo('tpos_v_mode', ['Offset', 'Top', 'Center', 'Bottom'], 150))
cols.field(grid, 'Offset', with_unit(spin('tpos_v_value', 130, -500, 500, 0.5, 1, 0), 'mm'), 1)
cols.field(grid, 'Relative to', combo('tpos_v_from', ['Paragraph', 'Margin', 'Page'], 150))
spacer(grid, 1); spacer(grid, 1)
body = section(d.options, 'Distance from surrounding text')
grid = cols.grid(body)
cols.field(grid, 'Top', with_unit(spin('tpos_top', 130, 0, 100, 0.5, 1, 0), 'mm'))
cols.field(grid, 'Left', with_unit(spin('tpos_left', 130, 0, 100, 0.5, 1, 3.2), 'mm'), 1)
cols.field(grid, 'Bottom', with_unit(spin('tpos_bottom', 130, 0, 100, 0.5, 1, 0), 'mm'))
cols.field(grid, 'Right', with_unit(spin('tpos_right', 130, 0, 100, 0.5, 1, 3.2), 'mm'), 1)
body = section(d.options, 'Text wrapping')
item(body, radio('tpos_around', 'Text flows around the table', first=True), 0, 'wxBOTTOM', 6)
item(body, radio('tpos_topbottom', 'Text stops above and continues below'), 0, 'wxBOTTOM', 10)
item(body, radio('tpos_front', 'In Front of Text'), 0, 'wxBOTTOM', 10)
item(body, radio('tpos_behind', 'Behind Text'), 0, 'wxBOTTOM', 10)
item(body, checkbox('tpos_overlap', 'Allow other floating tables to overlap'), 0)
d.finish('OK')
notext(d)
d.write('table_position.xrc')

# Recover documents ------------------------------------------------------------------
d = Dialog('rec', 'Recover Documents')
item(d.body, text('Impress kept copies of documents that were not saved.', 'rec_heading', bold=True), 0, 'wxBOTTOM', 4)
item(d.body, text('Choose one to restore. It opens as a new, unsaved document and never replaces the original file.', 'rec_hint', fg=MUTED), 0, 'wxBOTTOM', 14)
row = sizer('wxHORIZONTAL')
item(d.body, row, 1, 'wxEXPAND')
item(row, painted('rec_list', 330, 300), 1, 'wxEXPAND|wxRIGHT', 16)
column = sizer()
item(row, column, 0, 'wxEXPAND')
item(column, text('Preview', 'rec_preview_caption', fg=MUTED), 0, 'wxBOTTOM', 6)
item(column, painted('rec_preview', 230, 280, fill='#eef2f7'), 1, 'wxEXPAND')
d.footer_left('rec_delete', 'Delete', 96)
d.finish('Restore')
notext(d)
d.write('recovery_dialog.xrc')
