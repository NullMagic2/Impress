#!/usr/bin/env python3
"""Generate the Format ▾ dialogs of Modify Style (crates/gui/ui/format_*.xrc).

Each Format item opens its own dialog, as in Microsoft Word: Font, Paragraph,
Tabs, Borders and Shading, Language, Frame, Numbering and Bullets, Customize
Keyboard (Shortcut key) and Format Text Effects. Grouping and control sets
follow Word's dialogs; the look follows Impress (dialog_kit.py).

Some controls are not stored by the style model yet. They are real, working
controls in the dialogs (and are remembered while Modify Style is open); the
list lives in UI_ONLY in format_dialogs.rs so the backend can pick them up.
"""
from dialog_kit import *  # noqa: F401,F403

FONTS = ['Linux Libertine G', 'Times New Roman', 'Cambria', 'Georgia', 'Calibri', 'Arial', 'Segoe UI', 'Consolas']
SIZES = ['8', '9', '10', '11', '12', '14', '16', '18', '20', '22', '24', '26', '28', '36', '48', '72']
UNDERLINES = ['(none)', 'Words only', 'Single', 'Double', 'Thick', 'Dotted', 'Dashed', 'Dot-dash', 'Dot-dot-dash', 'Wave']
WIDE = 170


def font_dialog():
    d = Dialog('fnt', 'Font', tabs=['Font', 'Advanced'])
    # Font tab -------------------------------------------------------------------
    page = d.pages[0]
    top = section(page, 'Font', first=True)
    columns = node(None, 'wxFlexGridSizer', rows=0, cols=3, vgap=4, hgap=HGAP, growablecols='')
    item(top, columns)
    for caption in ('Font:', 'Font style:', 'Size:'):
        item(columns, text(caption))
    item(columns, entry('fnt_family', 250))
    item(columns, entry('fnt_style', 150))
    item(columns, entry('fnt_size', 80))
    item(columns, listbox('fnt_family_list', 250, 116, FONTS))
    item(columns, listbox('fnt_style_list', 150, 116, ['Regular', 'Italic', 'Bold', 'Bold Italic']))
    item(columns, listbox('fnt_size_list', 80, 116, SIZES))
    cols = Columns(label1=76, cell1=174, label2=104)
    grid = cols.grid(top, top=ROW + 4)
    cols.field(grid, 'Font color:', colour_combo('fnt_color'))
    cols.field(grid, 'Underline style:', combo('fnt_underline', UNDERLINES, FIELD), 1)
    cols.field(grid, 'Highlight:', colour_combo('fnt_highlight', hint='Text highlight (Impress)'))
    cols.field(grid, 'Underline color:', colour_combo('fnt_underline_color'), 1)
    effects = section(page, 'Effects')
    checks(effects, [
        [checkbox('fnt_strike', 'Strikethrough'), checkbox('fnt_small_caps', 'Small caps')],
        [checkbox('fnt_double_strike', 'Double strikethrough'), checkbox('fnt_all_caps', 'All caps')],
        [checkbox('fnt_superscript', 'Superscript'), checkbox('fnt_hidden', 'Hidden')],
        [checkbox('fnt_subscript', 'Subscript')],
    ])
    preview = section(page, 'Preview', grow=True)
    item(preview, painted('fnt_preview', -1, 72), 1)
    item(preview, text('Linux Libertine G · OpenType font; the preview uses the installed face.', 'fnt_preview_note',
                       fg=MUTED), 0, 'wxEXPAND|wxTOP', 6)
    # Advanced tab -----------------------------------------------------------------
    page = d.pages[1]
    cols = Columns(label1=104, cell1=170, label2=70)
    spacing = section(page, 'Character Spacing', first=True)
    grid = cols.grid(spacing)
    cols.field(grid, 'Scale:', combo('fnt_scale', ['200%', '150%', '100%', '90%', '80%', '66%', '50%', '33%'], FIELD, True))
    spacer(grid, 1)
    spacer(grid, 1)
    cols.field(grid, 'Spacing:', combo('fnt_spacing', ['Normal', 'Expanded', 'Condensed'], FIELD))
    cols.field(grid, 'By:', with_unit(spin('fnt_spacing_by', 90, 0, 1584, 0.1, 1), 'pt'), 1)
    cols.field(grid, 'Position:', combo('fnt_position', ['Normal', 'Raised', 'Lowered'], FIELD))
    cols.field(grid, 'By:', with_unit(spin('fnt_position_by', 90, 0, 1584, 0.5, 1), 'pt'), 1)
    cols.field(grid, 'Kerning:', combo('fnt_kerning', ['Optical', 'Metrics', 'Off'], FIELD))
    cols.field(grid, 'For fonts:', with_unit(spin('fnt_kerning_from', 90, 1, 1638, 0.5, 1, 8), 'pt and above'), 1)
    opentype = section(page, 'OpenType Features')
    grid = cols.grid(opentype, cols=2)
    cols.field(grid, 'Ligatures:', combo('fnt_ligatures', ['None', 'Standard Only', 'Standard and Contextual',
                                                           'Historical and Discretionary', 'All'], 230))
    cols.field(grid, 'Number spacing:', combo('fnt_number_spacing', ['Default', 'Proportional', 'Tabular'], 230))
    cols.field(grid, 'Number forms:', combo('fnt_number_forms', ['Default', 'Lining', 'Old-style'], 230))
    cols.field(grid, 'Stylistic sets:', combo('fnt_stylistic_set', ['Default'] + [str(i) for i in range(1, 21)], 230))
    checks(opentype, [[checkbox('fnt_contextual_alternates', 'Use Contextual Alternates')]], top=ROW + 2)
    preview = section(page, 'Preview', grow=True)
    item(preview, painted('fnt_preview_advanced', -1, 72), 1)
    d.footer_left('fnt_text_effects', 'Text Effects…', 110)
    d.finish()
    d.write('format_font.xrc')


def paragraph_dialog():
    d = Dialog('par', 'Paragraph', tabs=['Indents and Spacing', 'Line and Page Breaks'])
    page = d.pages[0]
    cols = Columns(label1=86, cell1=180, label2=96)
    general = section(page, 'General', first=True)
    grid = cols.grid(general)
    cols.field(grid, 'Alignment:', combo('par_alignment', ['Left', 'Centered', 'Right', 'Justified'], FIELD))
    cols.field(grid, 'Outline level:', combo('par_outline_level', ['Body Text'] + [f'Level {i}' for i in range(1, 10)], FIELD), 1)
    cols.field(grid, 'Direction:', combo('par_direction', ['Automatic', 'Left-to-right', 'Right-to-left'], FIELD))
    spacer(grid, 1)
    item(grid, checkbox('par_collapsed', 'Collapsed by default'), 0, 'wxALIGN_CENTER_VERTICAL')
    indentation = section(page, 'Indentation')
    grid = cols.grid(indentation)
    cols.field(grid, 'Left:', with_unit(spin('par_left', 90, -600, 600, 0.5, 1), 'mm'))
    cols.field(grid, 'Special:', combo('par_special', ['(none)', 'First line', 'Hanging'], FIELD), 1)
    cols.field(grid, 'Right:', with_unit(spin('par_right', 90, -600, 600, 0.5, 1), 'mm'))
    cols.field(grid, 'By:', with_unit(spin('par_special_by', 90, 0, 600, 0.5, 1), 'mm'), 1)
    checks(indentation, [[checkbox('par_mirror', 'Mirror indents')]], top=ROW)
    spacing = section(page, 'Spacing')
    grid = cols.grid(spacing)
    cols.field(grid, 'Before:', with_unit(spin('par_before', 90, 0, 1584, 6, 1), 'pt'))
    cols.field(grid, 'Line spacing:', combo('par_line_rule', ['Single', '1.5 lines', 'Double', 'At least', 'Exactly',
                                                              'Multiple'], FIELD), 1)
    cols.field(grid, 'After:', with_unit(spin('par_after', 90, 0, 1584, 6, 1), 'pt'))
    cols.field(grid, 'At:', with_unit(spin('par_line_at', 90, 0, 1584, 0.1, 2, 1.2), 'lines', 'par_line_unit'), 1)
    checks(spacing, [[checkbox('par_contextual', "Don't add space between paragraphs of the same style")]], top=ROW)
    preview = section(page, 'Preview', grow=True)
    item(preview, painted('par_preview', -1, 120), 1)
    # Line and Page Breaks ----------------------------------------------------------
    page = d.pages[1]
    pagination = section(page, 'Pagination', first=True)
    checks(pagination, [
        [checkbox('par_widow', 'Widow/Orphan control'), checkbox('par_keep_lines', 'Keep lines together')],
        [checkbox('par_keep_next', 'Keep with next'), checkbox('par_page_break', 'Page break before')],
    ])
    exceptions = section(page, 'Formatting exceptions')
    checks(exceptions, [[checkbox('par_suppress_numbers', 'Suppress line numbers'),
                         checkbox('par_no_hyphenate', "Don't hyphenate")]])
    typography = section(page, 'Typography')
    checks(typography, [[checkbox('par_hanging_punct', 'Hanging punctuation')]])
    cols = Columns(label1=110, cell1=180)
    grid = cols.grid(typography, cols=2, top=ROW)
    cols.field(grid, 'Glyph expansion:', with_unit(spin('par_expansion', 90, 0, 50, 1, 0, 15), '‰ when justifying'))
    textbox = section(page, 'Textbox options')
    grid = cols.grid(textbox, cols=2)
    cols.field(grid, 'Tight wrap:', combo('par_tight_wrap', ['None', 'All', 'First and last lines', 'First line only',
                                                             'Last line only'], WIDE))
    preview = section(page, 'Preview', grow=True)
    item(preview, painted('par_preview_breaks', -1, 120), 1)
    d.footer_left('par_tabs', 'Tabs…')
    d.finish()
    d.write('format_paragraph.xrc')


def tabs_dialog():
    d = Dialog('tab', 'Tabs')
    page = d.pages[0]
    stops = section(page, 'Tab stops', first=True)
    row = sizer('wxHORIZONTAL')
    item(stops, row)
    left = sizer()
    item(row, left, 0)
    item(left, text('Tab stop position:'), 0, 'wxBOTTOM', 4)
    position = entry('tab_position', 150)
    ET.SubElement(position, 'style').text = 'wxTE_PROCESS_ENTER'
    item(left, with_unit(position, 'mm'))
    item(left, listbox('tab_list', 150, 110), 0, 'wxTOP', 4)
    right = sizer()
    item(row, right, 0, 'wxLEFT', COLUMN_GAP + 8)
    item(right, text('Default tab stops:'), 0, 'wxBOTTOM', 4)
    item(right, with_unit(spin('tab_default', 90, 1, 600, 0.5, 1, 12.7), 'mm'))
    item(right, text('Tab stops to be cleared:'), 0, 'wxTOP', ROW + 6)
    item(right, text('—', 'tab_cleared', fg=MUTED, size='170,-1'), 0, 'wxTOP', 4)
    alignment = section(page, 'Alignment')
    checks(alignment, [
        [radio('tab_align_left', 'Left', True), radio('tab_align_center', 'Center'), radio('tab_align_right', 'Right')],
        [radio('tab_align_decimal', 'Decimal'), radio('tab_align_bar', 'Bar')],
    ])
    leader = section(page, 'Leader')
    checks(leader, [
        [radio('tab_leader_none', '1 None', True), radio('tab_leader_dots', '2 .......')],
        [radio('tab_leader_dashes', '3 -------'), radio('tab_leader_line', '4 _______')],
        [radio('tab_leader_heavy', '5 Heavy line'), radio('tab_leader_middle', '6 Middle dots')],
    ])
    actions = sizer('wxHORIZONTAL')
    item(page, actions, 0, 'wxTOP', SECTION)
    item(actions, skinned('tab_set', 'Set'), 0)
    item(actions, skinned('tab_clear', 'Clear'), 0, 'wxLEFT', 8)
    item(actions, skinned('tab_clear_all', 'Clear All'), 0, 'wxLEFT', 8)
    d.finish()
    item(page, text(' ', 'tab_status', fg=MUTED), 0, 'wxTOP', ROW)
    d.write('format_tabs.xrc')


def borders_dialog():
    d = Dialog('bdr', 'Borders and Shading', tabs=['Borders', 'Shading'])
    page = d.pages[0]
    row = sizer('wxHORIZONTAL')
    item(page, row, 1)
    setting = sizer()
    item(row, setting, 0, 'wxEXPAND')
    body = section(setting, 'Setting', first=True, grow=True)
    item(body, painted('bdr_setting', 120, 254), 1, 'wxEXPAND')
    style = sizer()
    item(row, style, 0, 'wxEXPAND|wxLEFT', COLUMN_GAP)
    body = section(style, 'Style', first=True, grow=True)
    item(body, painted('bdr_style_list', 170, 196), 1, 'wxEXPAND')
    grid = form_grid(body, cols=1, growable='0', top=ROW + 4)
    item(grid, text('Color:'))
    item(grid, colour_combo('bdr_color', 170))
    item(grid, text('Width:'))
    item(grid, combo('bdr_width', ['¼ pt', '½ pt', '¾ pt', '1 pt', '1½ pt', '2¼ pt', '3 pt', '4½ pt', '6 pt'], 170))
    preview = sizer()
    item(row, preview, 1, 'wxEXPAND|wxLEFT', COLUMN_GAP)
    body = section(preview, 'Preview', first=True, grow=True)
    item(body, text('Click the diagram or use the buttons to apply borders', fg=MUTED), 0, 'wxBOTTOM', 6)
    edges = sizer('wxHORIZONTAL')
    item(body, edges, 0, 'wxBOTTOM', 6)
    for index, (name, hint) in enumerate([('bdr_edge_top', 'Top border'), ('bdr_edge_bottom', 'Bottom border'),
                                          ('bdr_edge_left', 'Left border'), ('bdr_edge_right', 'Right border'),
                                          ('bdr_edge_between', 'Border between paragraphs')]):
        tool(edges, name, hint[0], hint, index == 0)
    item(body, painted('bdr_preview', 190, 150), 1, 'wxEXPAND')
    grid = form_grid(body, cols=2, growable='1', top=ROW + 4)
    field(grid, 'Apply to:', combo('bdr_apply', ['Paragraph', 'Text'], 120))
    distance = section(page, 'Distance from text')
    grid = form_grid(distance, cols=8, growable='')
    for index, (name, caption) in enumerate([('bdr_from_top', 'Top:'), ('bdr_from_bottom', 'Bottom:'),
                                             ('bdr_from_left', 'Left:'), ('bdr_from_right', 'Right:')]):
        field(grid, caption, with_unit(spin(name, 70, 0, 31, 0.5, 1, 1 if index < 2 else 4), 'pt'), expand=False,
              column_gap=COLUMN_GAP if index else 0, label_w=48)
    page = d.pages[1]
    cols = Columns(label1=70, cell1=WIDE)
    fill = section(page, 'Fill', first=True)
    grid = cols.grid(fill, cols=2)
    cols.field(grid, 'Fill:', colour_combo('shd_fill', WIDE))
    patterns = section(page, 'Patterns')
    grid = cols.grid(patterns, cols=2)
    cols.field(grid, 'Style:', combo('shd_pattern', ['Clear', 'Solid (100%)', '5%', '10%', '12.5%', '20%', '25%', '30%',
                                                '40%', '50%', '60%', '70%', '75%', '80%', '90%', 'Dk Horizontal',
                                                'Dk Vertical', 'Dk Down Diagonal', 'Dk Up Diagonal', 'Dk Grid',
                                                'Lt Horizontal', 'Lt Vertical', 'Lt Grid'], WIDE))
    cols.field(grid, 'Color:', colour_combo('shd_pattern_color', WIDE))
    cols.field(grid, 'Apply to:', combo('shd_apply', ['Paragraph', 'Text'], WIDE))
    preview = section(page, 'Preview', grow=True)
    item(preview, painted('shd_preview', -1, 120), 1)
    d.footer_left('bdr_options', 'Horizontal Line…', 128)
    d.finish()
    d.write('format_borders.xrc')


def language_dialog():
    d = Dialog('lng', 'Language')
    page = d.pages[0]
    mark = section(page, 'Mark selected text as', first=True, grow=True)
    item(mark, listbox('lng_list', 380, 210), 1, 'wxEXPAND')
    grid = form_grid(mark, cols=2, growable='1', top=ROW + 4)
    field(grid, 'Language tag:', entry('lng_tag', 160, 'BCP 47 tag, for example en-US or pt-BR'), expand=False)
    note = ('The speller and other proofing tools automatically use dictionaries of the selected language, '
            'if available.')
    item(mark, text(note, 'lng_note', fg=MUTED, wrap=380), 0, 'wxEXPAND|wxTOP', ROW)
    proofing = section(page, 'Proofing')
    checks(proofing, [[checkbox('lng_no_proof', 'Do not check spelling or grammar')],
                      [checkbox('lng_detect', 'Detect language automatically')]])
    d.footer_left('lng_default', 'Set As Default', 110)
    d.finish()
    d.write('format_language.xrc')


def frame_dialog():
    d = Dialog('frm', 'Frame')
    page = d.pages[0]
    wrap = section(page, 'Text wrapping', first=True)
    item(wrap, painted('frm_wrapping', 220, 92), 0)
    cols = Columns(label1=124, cell1=140, label2=86)
    size = section(page, 'Size')
    grid = cols.grid(size)
    cols.field(grid, 'Width:', combo('frm_width_rule', ['Auto', 'Exactly'], 120))
    cols.field(grid, 'At:', with_unit(spin('frm_width_at', 90, 0, 558, 0.5, 1), 'mm'), 1)
    cols.field(grid, 'Height:', combo('frm_height_rule', ['Auto', 'At least', 'Exactly'], 120))
    cols.field(grid, 'At:', with_unit(spin('frm_height_at', 90, 0, 558, 0.5, 1), 'mm'), 1)
    horizontal = section(page, 'Horizontal')
    grid = cols.grid(horizontal)
    cols.field(grid, 'Position:', combo('frm_h_position', ['Left', 'Right', 'Center', 'Inside', 'Outside'], 120, True))
    cols.field(grid, 'Relative to:', combo('frm_h_relative', ['Column', 'Margin', 'Page'], 120), 1)
    cols.field(grid, 'Distance from text:', with_unit(spin('frm_h_distance', 90, 0, 558, 0.5, 1), 'mm'))
    vertical = section(page, 'Vertical')
    grid = cols.grid(vertical)
    cols.field(grid, 'Position:', combo('frm_v_position', ['Top', 'Bottom', 'Center', 'Inside', 'Outside'], 120, True))
    cols.field(grid, 'Relative to:', combo('frm_v_relative', ['Margin', 'Page', 'Paragraph'], 120), 1)
    cols.field(grid, 'Distance from text:', with_unit(spin('frm_v_distance', 90, 0, 558, 0.5, 1), 'mm'))
    checks(vertical, [[checkbox('frm_move', 'Move with text'), checkbox('frm_lock', 'Lock anchor')]], top=ROW + 2)
    d.footer_left('frm_remove', 'Remove Frame', 110)
    d.finish()
    d.write('format_frame.xrc')


def numbering_dialog():
    d = Dialog('num', 'Numbering and Bullets', tabs=['Bullets', 'Numbering'])
    page = d.pages[0]
    body = section(page, 'Bullet library', first=True, grow=True)
    item(body, painted('num_bullets', 440, 230), 1, 'wxEXPAND')
    row = sizer('wxHORIZONTAL')
    item(body, row, 0, 'wxTOP', ROW + 4)
    item(row, button('num_define_bullet', 'Define New Bullet…', 150))
    page = d.pages[1]
    body = section(page, 'Numbering library', first=True, grow=True)
    item(body, painted('num_numbers', 440, 230), 1, 'wxEXPAND')
    options = section(page, 'List')
    checks(options, [[radio('num_restart', 'Restart numbering', True), radio('num_continue', 'Continue previous list')]])
    cols = Columns(label1=70, cell1=110, label2=50)
    grid = cols.grid(options, top=ROW + 2)
    cols.field(grid, 'Start at:', spin('num_start', 90, 1, 32767, 1, 0, 1))
    cols.field(grid, 'Level:', combo('num_level', [str(i) for i in range(1, 10)], 80), 1)
    row = sizer('wxHORIZONTAL')
    item(options, row, 0, 'wxTOP', ROW + 4)
    item(row, button('num_define_number', 'Define New Number Format…', 200))
    d.finish()
    d.write('format_numbering.xrc')


def shortcut_dialog():
    d = Dialog('key', 'Customize Keyboard')
    page = d.pages[0]
    command = section(page, 'Specify a command', first=True)
    row = sizer('wxHORIZONTAL')
    item(command, row)
    item(row, labelled('Categories:', listbox('key_categories', 190, 96, ['Styles'])), 0)
    item(row, labelled('Commands:', listbox('key_commands', 230, 96)), 0, 'wxLEFT', COLUMN_GAP)
    sequence = section(page, 'Specify keyboard sequence')
    row = sizer('wxHORIZONTAL')
    item(sequence, row)
    item(row, labelled('Current keys:', listbox('key_current', 190, 80)), 0)
    right = sizer()
    item(row, right, 0, 'wxLEFT', COLUMN_GAP)
    item(right, text('Press new shortcut key:'), 0, 'wxBOTTOM', 4)
    item(right, entry('key_new', 230, 'Press the key combination'))
    item(right, text('Currently assigned to:', fg=MUTED), 0, 'wxTOP', ROW)
    item(right, text('[unassigned]', 'key_assigned', size='230,-1'), 0, 'wxTOP', 2)
    grid = form_grid(sequence, cols=2, growable='', top=ROW + 4)
    field(grid, 'Save changes in:', combo('key_save_in', ['This document', 'Normal template'], 190), expand=False)
    description = section(page, 'Description')
    item(description, text('Applies the style.', 'key_description', fg=MUTED, wrap=440))
    d.footer_left('key_assign', 'Assign')
    d.footer_left('key_remove', 'Remove')
    d.footer_left('key_reset', 'Reset All…')
    d.finish('Close', cancel=False)
    d.write('format_shortcut.xrc')


def text_effects_dialog():
    d = Dialog('fx', 'Format Text Effects', tabs=['Text Fill & Outline', 'Text Effects'])
    cols = Columns(label1=96, cell1=190, label2=96)
    page = d.pages[0]
    fill = section(page, 'Text Fill', first=True)
    checks(fill, [[radio('fx_fill_none', 'No fill', True), radio('fx_fill_solid', 'Solid fill'),
                   radio('fx_fill_gradient', 'Gradient fill')]])
    grid = cols.grid(fill, top=ROW + 2)
    cols.field(grid, 'Color:', colour_combo('fx_fill_color', 150))
    cols.field(grid, 'Transparency:', with_unit(spin('fx_fill_transparency', 80, 0, 100, 1), '%'), 1)
    outline = section(page, 'Text Outline')
    checks(outline, [[radio('fx_line_none', 'No line', True), radio('fx_line_solid', 'Solid line'),
                      radio('fx_line_gradient', 'Gradient line')]])
    grid = cols.grid(outline, top=ROW + 2)
    cols.field(grid, 'Color:', colour_combo('fx_line_color', 150))
    cols.field(grid, 'Transparency:', with_unit(spin('fx_line_transparency', 80, 0, 100, 1), '%'), 1)
    cols.field(grid, 'Width:', with_unit(spin('fx_line_width', 80, 0, 1584, 0.25, 2, 0.75), 'pt'))
    cols.field(grid, 'Dash type:', combo('fx_line_dash', ['Solid', 'Round dot', 'Square dot', 'Dash', 'Dash dot',
                                                          'Long dash', 'Long dash dot'], 130), 1)
    cols.field(grid, 'Cap type:', combo('fx_line_cap', ['Flat', 'Round', 'Square'], 150))
    cols.field(grid, 'Join type:', combo('fx_line_join', ['Round', 'Bevel', 'Miter'], 130), 1)
    preview = section(page, 'Preview', grow=True)
    item(preview, painted('fx_preview', -1, 64), 1)
    page = d.pages[1]
    shadow = section(page, 'Shadow', first=True)
    grid = cols.grid(shadow)
    cols.field(grid, 'Presets:', combo('fx_shadow', ['No shadow', 'Outer: offset bottom', 'Outer: offset right',
                                                     'Outer: centered', 'Inner: centered', 'Perspective: below'], 170))
    cols.field(grid, 'Color:', colour_combo('fx_shadow_color', 130), 1)
    cols.field(grid, 'Transparency:', with_unit(spin('fx_shadow_transparency', 80, 0, 100, 1, 0, 60), '%'))
    cols.field(grid, 'Blur:', with_unit(spin('fx_shadow_blur', 80, 0, 100, 0.5, 1, 4), 'pt'), 1)
    cols.field(grid, 'Angle:', with_unit(spin('fx_shadow_angle', 80, 0, 359.9, 1, 1, 45), '°'))
    cols.field(grid, 'Distance:', with_unit(spin('fx_shadow_distance', 80, 0, 200, 0.5, 1, 3), 'pt'), 1)
    reflection = section(page, 'Reflection')
    grid = cols.grid(reflection)
    cols.field(grid, 'Presets:', combo('fx_reflection', ['No reflection', 'Tight, touching', 'Half, touching',
                                                         'Full, touching', 'Tight, 4 pt offset'], 170))
    cols.field(grid, 'Size:', with_unit(spin('fx_reflection_size', 80, 0, 100, 1, 0, 50), '%'), 1)
    cols.field(grid, 'Transparency:', with_unit(spin('fx_reflection_transparency', 80, 0, 100, 1, 0, 50), '%'))
    cols.field(grid, 'Distance:', with_unit(spin('fx_reflection_distance', 80, 0, 100, 0.5, 1), 'pt'), 1)
    glow = section(page, 'Glow')
    grid = cols.grid(glow)
    cols.field(grid, 'Presets:', combo('fx_glow', ['No glow', '5 pt glow', '8 pt glow', '11 pt glow', '18 pt glow'], 170))
    cols.field(grid, 'Color:', colour_combo('fx_glow_color', 130), 1)
    cols.field(grid, 'Size:', with_unit(spin('fx_glow_size', 80, 0, 150, 0.5, 1), 'pt'))
    cols.field(grid, 'Transparency:', with_unit(spin('fx_glow_transparency', 80, 0, 100, 1, 0, 60), '%'), 1)
    bevel = section(page, '3-D Format')
    grid = cols.grid(bevel)
    cols.field(grid, 'Top bevel:', combo('fx_bevel', ['None', 'Circle', 'Relaxed inset', 'Cross', 'Cool slant', 'Angle',
                                                      'Soft round', 'Convex', 'Slope', 'Divot', 'Riblet', 'Hard edge',
                                                      'Art deco'], 170))
    cols.field(grid, 'Material:', combo('fx_material', ['Matte', 'Warm matte', 'Plastic', 'Metal', 'Dark edge',
                                                        'Soft edge', 'Flat', 'Wire frame'], 130), 1)
    cols.field(grid, 'Depth:', with_unit(spin('fx_depth', 80, 0, 1584, 0.5, 1), 'pt'))
    cols.field(grid, 'Lighting:', combo('fx_lighting', ['Three point', 'Balanced', 'Soft', 'Harsh', 'Flood',
                                                        'Contrasting'], 130), 1)
    preview = section(page, 'Preview', grow=True)
    item(preview, painted('fx_preview_effects', -1, 64), 1)
    d.finish()
    d.write('format_text_effects.xrc')


if __name__ == '__main__':
    font_dialog()
    paragraph_dialog()
    tabs_dialog()
    borders_dialog()
    language_dialog()
    frame_dialog()
    numbering_dialog()
    shortcut_dialog()
    text_effects_dialog()
