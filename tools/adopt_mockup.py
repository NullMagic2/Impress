"""Replace the old visible ribbon with the supplied mockup's native wx hierarchy.

The document/editor services still address some historical control names. Controls
absent from the new design live in an invisible compatibility panel, so only the
new design appears while those services continue to compile and run.
"""
from copy import deepcopy
from pathlib import Path
import json

# Space after each column of stacked checkboxes, in device-independent pixels.
CHECK_COLUMN_GAP_DIP = 14
# Margin around each number box, so neighbouring boxes sit twice this apart.
SPIN_MARGIN_DIP = 4
# Combo boxes are sized from their longest entry. GTK's dropdown button and
# the field padding take COMBO_CHROME_DIP; an average caption character is
# about COMBO_CHAR_DIP wide; every combo fits at least COMBO_MIN_CHARS
# characters (so a font size such as "10.5" is never cut off).
COMBO_CHROME_DIP = 56
COMBO_CHAR_DIP = 7.5
COMBO_MIN_CHARS = 4
# Gap between side-by-side columns of stacked commands in a group, so a long
# label or a dropdown arrow never touches the next column.
COLUMN_GAP_DIP = 10


def adopt(env):
    E=env['E']; r=env['r']; root=env['root']; rootbox=env['rootbox']; old_nb=env['nb']
    obj=env['obj']; item=env['item']; sizer=env['sizer']; size=env['size']
    button=env['button']; combo=env['combo']; label=env['label']; space=env['space']
    group=env['group']; colors=env['COLORS']; groups=env['GROUPS']
    buttons=env['BUTTONS']; sizes=env['SIZES']; center=env['center_for_sizer']
    layout=json.loads((env['ROOT']/'design/interface-layout.json').read_text(encoding='utf-8'))
    original={node.get('name'):deepcopy(node) for node in old_nb.iter('object')
              if node.get('name') and node.get('class') in ('wxButton','wxComboBox')}
    specs={spec['name']:spec for spec in buttons}
    used=set()

    # The old notebook is removed from the visible tree. The new tabs take its
    # original position between the quick-access strip and document workspace.
    old_slot=next(slot for slot in rootbox.findall('object') if slot.find('object') is old_nb)
    index=list(rootbox).index(old_slot)
    rootbox.remove(old_slot)
    new_slot=item(rootbox)
    rootbox.remove(new_slot); rootbox.insert(index,new_slot)
    nb=obj(new_slot,'wxPanel','ribbon_notebook',bg=colors['title'],style='wxTAB_TRAVERSAL')
    size(nb,'ribbon_notebook',1,154)
    shell=sizer(nb,'wxVERTICAL')
    tabs=obj(item(shell,0,'wxEXPAND'),'wxPanel','ribbon_tab_strip',bg=colors['title'])
    size(tabs,'ribbon_tab_strip',-1,36)
    tab_row=sizer(tabs,'wxHORIZONTAL')
    space(tab_row,14)
    for index,tab in enumerate(layout):
        title=tab['title']; name=f'ribbon_{title.lower()}_tab'
        width={'File':56,'Home':72,'Insert':72,'Layout':78,'References':106,'Review':78,'View':62,'Typst':68}[title]
        # The strip paints the tabs itself (ribbon/tab_strip.rs), so the active
        # tab is one uniform surface with the ribbon body and no native button
        # frame shows. The hidden buttons keep names and accessibility labels.
        node=obj(item(tab_row,0,'wxALIGN_CENTER_VERTICAL'),'wxButton',name,label=title,style='wxBU_EXACTFIT',tooltip=f'{title} ribbon',hidden='1')
        size(node,name,width,36)
        buttons.append(dict(name=name,label=title,icon='help',action=f'SelectRibbonTab({index})',kind='tab',tip=f'{title} ribbon',disabled=False,
                            width_dip=width,height_dip=36,art_width_dip=1,art_height_dip=1,segment='single'))
        space(tab_row,3)
    content=obj(item(shell,1,'wxEXPAND'),'wxPanel','ribbon_panel_content',bg=colors['ribbon'])
    content_box=sizer(content,'wxVERTICAL')
    groups.clear()

    aliases={
        'insert_picture_button':'insert_image_button', 'blank_page_button':'page_break_button',
        'breaks_button':'section_break_button', 'insert_footnote_button':'footnote_button',
        'track_changes_button':'changes_button',
        'outline_view_button':'navigation_button', 'zoom_100_button':'view_100_button',
        'zoom_one_page_button':'view_page_button', 'zoom_page_width_button':'view_fit_button',
        'typst_source_button':'source_button', 'typst_split_button':'source_split_button',
        'compile_button':'source_refresh_button', 'export_pdf_button':'pdf_button',
        'print_layout_button':'view_graphical_button', 'split_window_button':'view_split_button',
    }
    # Mockup-only buttons whose command exists although no historical button does.
    mockup_actions={'zoom_dialog_button':'ZoomDialog','insert_citation_button':'CitationManager','date_time_button':'InsertField(2)',
        # Word's footnote commands: insert and edit on the page, step through notes.
        'insert_footnote_button':'InsertNote(false)','insert_endnote_button':'InsertNote(true)',
        'next_footnote_button':'NextFootnote(true)','show_notes_button':'ShowNotes',
        # Comments: the pane's commands on the current comment.
        'new_comment_button':'CommentNew','delete_comment_button':'CommentDelete',
        'resolve_comment_button':'CommentResolve','previous_comment_button':'CommentStep(false)',
        'next_comment_button':'CommentStep(true)',
        # Both insert at the caret: Equation in the equation pane, Inline in
        # a box in the text line.
        'display_equation_button':'Editor(AppCommand::InsertEquation)',
        'insert_inline_equation_button':'Editor(AppCommand::InsertInlineEquation)'}
    styles={'big-button':('large',62,78,32,32),
            'file-main':('file-main',78,78,32,32), 'file-menu':('file-menu',78,78,32,32), 'file-minor':('file-minor',98,36,18,18),
            'small-button':('small',68,24,16,16),
            'tool-button':('icon',30,28,18,18), 'style-choice':('gallery',74,70,64,30)}
    style_icons={'style_normal_button':'style_normal','style_heading1_button':'style_heading1','style_title_button':'style_title'}

    def set_prop(node,key,value):
        child=node.find(key)
        if child is None: child=E.SubElement(node,key)
        child.text=str(value)

    def insert_widget(parent,node,flag=None):
        slot=item(parent,0,flag or center(parent))
        slot.append(node)

    def stacked(parent,style):
        # Small buttons stacked in a column share one left edge, so their
        # icons line up; centring each one would stagger them by label width.
        return style=='small-button' and parent.findtext('orient')=='wxVERTICAL'

    def align_left(parent,name):
        for slot in parent.findall('object'):
            child=slot.find('object')
            if child is not None and child.get('name')==name:
                slot.find('flag').text='wxALIGN_LEFT'

    def emit(data,parent):
        kind=data['kind']
        if kind in ('h','v'):
            if kind=='h' and {c.get('name') for c in data['children']} == {'style_normal_button','style_heading1_button','style_title_button'}:
                # Word-style gallery, painted by ribbon/style_gallery.rs: three
                # style cards that scroll through every paragraph style, and a
                # strip with row up, row down and More. The old fixed style
                # buttons stay in the hidden compatibility panel.
                inner=obj(item(parent,0,center(parent)),'wxPanel','style_gallery',bg=colors['group'],style='wxWANTS_CHARS')
                size(inner,'style_gallery',3*74+17,70)
                return
            box=sizer(item(parent,0,'wxALIGN_CENTER_VERTICAL' if parent.findtext('orient')=='wxHORIZONTAL' else 'wxEXPAND'),
                      'wxHORIZONTAL' if kind=='h' else 'wxVERTICAL')
            for index,child in enumerate(data['children']):
                if index and kind=='h' and child['kind']=='v': space(box,COLUMN_GAP_DIP)
                emit(child,box)
            return
        name=data['name']; used.add(name)
        if kind=='button':
            style=data['style']; button_kind,w,h,aw,ah=styles[style]
            # Typography and Hyphenation open menus: a caret, and one width.
            menu_buttons=('typography_menu_button','hyphenation_button')
            if name in menu_buttons: w=132
            art=style_icons.get(name,data['icon'])
            text=data['label']; tip=data['tip']
            if name in original:
                node=original[name]
                set_prop(node,'label','' if button_kind=='icon' else text)
                set_prop(node,'minsize',f'{w},{h}')
                set_prop(node,'tooltip',tip)
                insert_widget(parent,node,'wxALIGN_LEFT' if stacked(parent,style) else None)
                spec=specs[name]
                spec.update(label=text,icon=art,kind=button_kind,tip=tip,width_dip=w,height_dip=h,
                            art_width_dip=aw,art_height_dip=ah,segment='single')
                if name in menu_buttons: spec['kind']='typography-menu'
                sizes.append((name,w,h))
            else:
                action=(mockup_actions.get(name) or
                        (specs[aliases[name]]['action'] if name in aliases else 'Unavailable'))
                button(parent,name,text,art,action,button_kind,tip,
                       disabled='Not available in this alpha' if action=='Unavailable' else None)
                if stacked(parent,style): align_left(parent,name)
                spec=buttons[-1]
                spec.update(width_dip=w,height_dip=h,art_width_dip=aw,art_height_dip=ah)
                node=next(n for n in parent.iter('object') if n.get('name')==name)
                set_prop(node,'minsize',f'{w},{h}')
                sizes.append((name,w,h))
            return
        if kind=='combo':
            # The font name box is painted by Impress (font_preview.rs), not a
            # native combo, so it keeps its designed width.
            longest=max([len(entry) for entry in data['items']+[data['value']]]+[COMBO_MIN_CHARS])
            native=name!='font_family_combo'
            w=max(data['width'],round(longest*COMBO_CHAR_DIP)+COMBO_CHROME_DIP) if native else data['width']; h=28
            if name in original:
                node=original[name]
                set_prop(node,'minsize',f'{w},{h}')
                insert_widget(parent,node)
                sizes.append((name,w,h))
            else:
                combo(parent,name,data['items'],data['value'],w,name.replace('_',' '),True)
                node=next(n for n in parent.iter('object') if n.get('name')==name)
                set_prop(node,'minsize',f'{w},{h}')
                sizes.append((name,w,h))
            return
        if kind=='check':
            # Stacked checkboxes share one left edge, as in a form; centring
            # each one would stagger them by label width. A right border
            # separates side-by-side columns.
            vertical=parent.findtext('orient')=='wxVERTICAL'
            flag='wxALIGN_LEFT|wxRIGHT' if vertical else center(parent)
            node=obj(item(parent,0,flag,CHECK_COLUMN_GAP_DIP if vertical else 0),'wxCheckBox',name,label=data['label'],checked='1' if data['checked'] else '0')
            size(node,name,-1,24)
            return
        if kind=='spin':
            node=obj(item(parent,0,center(parent)+'|wxALL',SPIN_MARGIN_DIP),'wxTextCtrl',name,value=data['value'],style='wxTE_PROCESS_ENTER')
            size(node,name,54,24)

    def page(title,selected=False):
        sc=obj(item(content_box,1,'wxEXPAND'),'wxScrolledWindow','ribbon_'+title.lower(),style='wxHSCROLL|wxTAB_TRAVERSAL',
               scrollrate='16,0',bg=colors['ribbon'])
        if not selected: set_prop(sc,'hidden','1')
        size(sc,'ribbon_'+title.lower(),1,118)
        return sizer(sc,'wxHORIZONTAL')

    def add_group(row,tab,index,title,body):
        slug=''.join(c.lower() if c.isalnum() else '_' for c in title).strip('_')
        name=f'{tab.lower()}_{slug}_{index}_group'
        container=group(row,name,title,leading=index==0)
        emit(body,container)

    for tab in layout:
        row=page(tab['title'],tab['title']=='Home')
        for index,g in enumerate(tab['groups']): add_group(row,tab['title'],index,g['title'],g['layout'])
    # Table tools: one owner-painted page (crates/gui/src/ribbon/table_bar.rs)
    # shown in place of the Insert page after Insert > Table, until closed.
    tools=obj(item(content_box,1,'wxEXPAND'),'wxPanel','table_context_tools',style='wxTAB_TRAVERSAL|wxBORDER_NONE',bg=colors['ribbon'],hidden='1')
    size(tools,'table_context_tools',1,118)

    def tool(name,label,icon,tip,style='small-button'):
        return dict(kind='button',name=name,label=label,icon=icon,tip=tip,style=style)

    def contextual_page(name,prefix,page_groups):
        """A Ribbon page shown in place of the Insert page, as Word's
        contextual tabs. Its buttons come from the strip build_ribbon_ui.py
        made under the Ribbon, which is removed."""
        strip=next(node for node in r.iter('object') if node.get('name')==name)
        for node in strip.iter('object'):
            if node.get('name') and node.get('class')=='wxButton': original[node.get('name')]=deepcopy(node)
        slot_parent=next(slot for slot in r.iter('object') if strip in list(slot))
        for holder in r.iter('object'):
            if slot_parent in list(holder): holder.remove(slot_parent); break
        tools_page=obj(item(content_box,1,'wxEXPAND'),'wxPanel',name,style='wxTAB_TRAVERSAL|wxBORDER_NONE',
                       bg=colors['ribbon'],hidden='1')
        size(tools_page,name,1,118)
        tools_row=sizer(tools_page,'wxHORIZONTAL')
        for index,(title,body) in enumerate(page_groups):
            slug=''.join(c.lower() if c.isalnum() else '_' for c in title).strip('_')
            emit(body,group(tools_row,f'{prefix}_{slug}_{index}_group',title,leading=index==0))

    # Header & Footer tools: shown while a header or footer is edited.
    contextual_page('header_context_tools','header_tools',[
        ('Navigation',dict(kind='v',children=[
            tool('header_go_header_button','Go to Header','header','Move to the header of this page style'),
            tool('header_go_footer_button','Go to Footer','footer','Move to the footer of this page style')])),
        ('Options',dict(kind='v',children=[
            tool('header_default_button','All pages','header-footer','Edit the header or footer used on most pages'),
            tool('header_first_button','Different first page','header-first','Give the first page its own header and footer'),
            tool('header_even_button','Different odd & even','header-even','Give even pages their own header and footer')])),
        ('Insert',dict(kind='v',children=[
            tool('header_page_number_field_button','Page number','page-number','Insert the page number'),
            tool('header_page_count_field_button','Page count','page-count','Insert the number of pages'),
            tool('header_date_field_button','Date','calendar',"Insert today's date")])),
        ('Edit',tool('header_remove_button','Remove','trash','Delete this header or footer','big-button')),
        ('Close',tool('header_exit_button','Close','x','Close Header and Footer (Esc)','big-button')),
    ])
    # Picture tools: one painted page (crates/gui/src/ribbon/picture_bar.rs)
    # shown in place of the Insert page while a picture is selected.
    pictures=obj(item(content_box,1,'wxEXPAND'),'wxPanel','picture_context_tools',style='wxTAB_TRAVERSAL|wxBORDER_NONE',bg=colors['ribbon'],hidden='1')
    size(pictures,'picture_context_tools',1,118)

    # Every group uses the same short caption bar. The hidden static label
    # retains its accessible text while group_relief.rs paints the visible one.
    for name in (f'{group_name}_caption' for group_name in groups):
        caption=next(node for node in r.iter('object') if node.get('name')==name)
        set_prop(caption,'bg','#eef3fc')
        set_prop(caption,'minsize','-1,18')
        for text_node in caption.iter('object'):
            if text_node.get('class')=='wxStaticText': set_prop(text_node,'hidden','1')
        sizes.append((name,-1,18))

    # Service controls that are absent from the new design have no visible
    # counterpart. Keep their typed fields available to the existing editor
    # controller without exposing an obsolete ribbon row.
    compatibility=obj(root,'wxPanel','interface_services',hidden='1',bg=colors['root'])
    compat_sizer=sizer(compatibility,'wxVERTICAL')
    status=env['s']
    for slot in list(status.findall('object')):
        child=slot.find('object')
        if child is not None and child.get('name')=='fit_width_button':
            status.remove(slot)
            item(compat_sizer).append(child)
    for name,node in original.items():
        if name not in used:
            slot=item(compat_sizer)
            slot.append(node)
    for spec in buttons:
        if spec['name'] not in used:
            spec['segment']='single'
    # Scale only controls present in the resulting hierarchy, choosing the
    # latest mockup size when a moved control had an older metric.
    present={node.get('name') for node in r.iter('object') if node.get('name')}
    latest={name:(w,h) for name,w,h in sizes if name in present}
    sizes[:]=[(name,*wh) for name,wh in latest.items()]
