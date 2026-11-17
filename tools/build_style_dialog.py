#!/usr/bin/env python3
"""Generate the native Modify Style dialog (crates/gui/ui/style_editor.xrc).

Structure follows Word's Modify Style dialog; colours follow the Impress
interface tokens (design/interface-tokens.json and the reference HTML), so the
dialog reads like the Ribbon: a white card on the pale blue-grey chrome.

Alignment grid (DIPs):
  M     outer margin on the chrome; the footer buttons end on the card's edge.
  PAD   padding inside the white card.
  IND   every section body is indented under its heading: heading, labels
        and controls form three levels.
  BTN   every toolbar button is the same square. Rust paints them with the
        Ribbon skin and matches the square to the native combo height, so both
        toolbar rows share one height and one left edge.

Toolbar row 2 is Word's: alignment | line spacing | paragraph spacing | indent,
with one divider rhythm (GAP, a 1-DIP rule, GAP) between all groups.

XRC sizes are DIPs (wxWidgets 3.1.5+). Nothing is absolutely positioned.
"""
import xml.etree.ElementTree as ET
from dialog_kit import *  # noqa: F401,F403 - tokens, grid and XRC helpers shared with the Format dialogs

def group_width(buttons):
    return buttons * BTN + (buttons - 1) * TIGHT


DIVIDER_W = 2 * GAP + 1
# Row 1 sits on row 2's grid: the font box spans the alignment and line-spacing
# groups, the size box spans the paragraph-spacing group, so both rows share
# their dividers and B / I line up with the indent buttons below.
FONT_W = group_width(4) + DIVIDER_W + group_width(3)
# Keep room for two digits beside the native dropdown arrow.
MIN_SIZE_W = 76
SIZE_W = max(group_width(2), MIN_SIZE_W)
COLOR_W = 150   # swatch + colour name + caret

FONT_TOGGLES = [
    ('style_bold_button', 'B', 'Bold'),
    ('style_italic_button', 'I', 'Italic'),
    ('style_underline_button', 'U', 'Underline'),
]
PARAGRAPH_GROUPS = [
    [('style_left_button', 'L', 'Align left'),
     ('style_center_button', 'C', 'Center'),
     ('style_right_button', 'R', 'Align right'),
     ('style_justify_button', 'J', 'Justify')],
    [('style_single_spacing', '1', 'Single line spacing'),
     ('style_onehalf_spacing', '1.5', '1.5 line spacing'),
     ('style_double_spacing', '2', 'Double line spacing')],
    [('style_spacing_increase', '+', 'Increase paragraph spacing (space before +6 pt)'),
     ('style_spacing_decrease', '-', 'Decrease paragraph spacing (space before -6 pt)')],
    [('style_indent_decrease', '<', 'Decrease indent (5 mm)'),
     ('style_indent_increase', '>', 'Increase indent (5 mm)')],
]


# Dialog --------------------------------------------------------------------------
root = ET.Element('resource')
dialog = node(root, 'wxDialog', 'style_editor', title='Modify Style',
              style='wxDEFAULT_DIALOG_STYLE|wxRESIZE_BORDER', bg=CHROME)
outer = sizer()
dialog.append(outer)

card = rounded('style_card', CARD)
item(outer, card, 1, 'wxEXPAND|wxLEFT|wxRIGHT|wxTOP', M)
card_sizer = sizer()
card.append(card_sizer)
body = sizer()
item(card_sizer, body, 1, 'wxEXPAND|wxALL', PAD)

# Properties
props = section(body, 'Properties', first=True)
grid = form_grid(props)
field(grid, 'Name:', entry('style_name', 320))
field(grid, 'Style type:', combo('style_type', ['Paragraph', 'Character'], disabled=True))
field(grid, 'Style based on:', combo('style_base', ['No base style']))
field(grid, 'Style for following paragraph:', combo('style_next', ['Same style']))

# Formatting
formatting = section(body, 'Formatting', grow=True)  # the preview absorbs extra height
fonts = sizer('wxHORIZONTAL')
item(formatting, fonts)
item(fonts, tip(combo('style_font_family', ['Inherit from base', 'Linux Libertine G', 'Times New Roman', 'Cambria',
                                            'Georgia', 'Calibri', 'Arial', 'Segoe UI', 'Consolas'], FONT_W, True), 'Font'),
     0, 'wxALIGN_CENTER_VERTICAL')
divider(fonts)
item(fonts, tip(combo('style_font_size', ['8', '9', '10', '11', '12', '14', '16', '18', '20', '24', '28', '36', '48',
                                          '72'], SIZE_W, True), 'Font size'),
     0, 'wxALIGN_CENTER_VERTICAL')
divider(fonts)
for index, (name, fallback, hint) in enumerate(FONT_TOGGLES):
    tool(fonts, name, fallback, hint, index == 0)
divider(fonts)
# Keep native choice storage separate from the swatch painted by Rust.
colour = node(None, 'wxBitmapComboBox', 'style_font_color', size='1,1', style='wxCB_READONLY')
item(fonts, tip(colour, 'Stored font color choice'), 0, 'wxALIGN_CENTER_VERTICAL')
colour_display = node(None, 'wxPanel', 'style_color_display',
                      size=f'{COLOR_W},{FIELD_H}', minsize=f'{COLOR_W},{FIELD_H}',
                      bg=CARD, style='wxWANTS_CHARS')
item(fonts, tip(colour_display, 'Font color choices'), 0, 'wxALIGN_CENTER_VERTICAL')

paragraph = sizer('wxHORIZONTAL')
item(formatting, paragraph, 0, 'wxEXPAND|wxTOP', ROW - 2)
for group_index, group in enumerate(PARAGRAPH_GROUPS):
    if group_index:
        divider(paragraph)
    for index, (name, fallback, hint) in enumerate(group):
        tool(paragraph, name, fallback, hint, index == 0)

# The preview is painted by Rust (paint_style_preview): previous, sample and
# following paragraphs at true size with the styles' real line and paragraph
# spacing, so it has no child controls.
preview = rounded('style_preview_panel', CARD, size='-1,200')
item(formatting, preview, 1, 'wxEXPAND|wxTOP', ROW + 7)

summary = rounded('style_summary_panel', CONTEXT)
item(formatting, summary, 0, 'wxEXPAND|wxTOP', ROW + 1)
summary_sizer = sizer()
summary.append(summary_sizer)
item(summary_sizer, text('Font:', 'style_summary', fg=MUTED, size='-1,48'), 0, 'wxEXPAND|wxALL', 10)

# Format ▾ items open Word-style dialogs (tools/build_format_dialogs.py).
item(body, text('', 'style_error', fg=ERROR), 0, 'wxEXPAND|wxTOP', ROW)

# Keep the Format menu at the leading edge and group the native actions.
item(outer, node(None, 'wxStaticLine'), 0, 'wxEXPAND|wxTOP|wxLEFT|wxRIGHT', M)
buttons = sizer('wxHORIZONTAL')
item(outer, buttons, 0, 'wxEXPAND|wxALL', M)
footer_action(buttons, 'style_format', 'Format ▾')
spacer(buttons, 1, 1, 1)
footer_pair(buttons, 'style_save', 'OK', 'style_cancel', 'Cancel')

for control in body.iter('object'):
    if control.get('class') == 'wxButton':
        style = control.find('style')
        if style is None: style = ET.SubElement(control, 'style')
        style.text = SKINNED

write(root, 'style_editor.xrc')
