#!/usr/bin/env python3
"""Generate the standard-handler XRC and typed Ribbon bindings from one definition.
Run after editing this file; ordinary Cargo builds use the checked-in output.
"""
from pathlib import Path
import json
import xml.etree.ElementTree as E
from xrc_sizer_checks import center_for_sizer, require_valid_alignment
from localization_kit import name_translatable_controls
ROOT=Path(__file__).resolve().parents[1]
APP=ROOT/'crates/gui'
COLORS=json.loads((ROOT/'design/interface-tokens.json').read_text(encoding='utf-8'))
BUTTONS=[]; SIZES=[]; GROUPS=[]; COMBOS=[]
# Logical (96-DPI) sizes only. The native bitmap bundle owns DPI scaling.
# button width/height, artwork width/height (before the 2-DIP transparent inset).
BUTTON_METRICS = {
    'large': (68, 66, 30, 30),
    'large-menu': (62, 78, 32, 32),
    'file-main': (78, 78, 32, 32),
    'file-menu': (78, 78, 32, 32),
    'file-minor': (98, 36, 18, 18),
    'hero': (74, 72, 40, 40),
    'wide': (98, 66, 30, 30),
    'small': (76, 26, 16, 16),
    'icon': (26, 26, 16, 16),
    'quick': (28, 28, 18, 18),
    'logo': (26, 28, 20, 20),
    'gallery': (112, 66, 94, 30),
    'gallery-more': (18, 24, 0, 0),
    'mode': (94, 26, 16, 16),
    # Styles pane commands: outlined faces on the white pane (pane-default is
    # the accent-filled primary command). Painted by the wxDragon face renderer.
    'pane': (76, 28, 16, 16),
    'pane-default': (120, 28, 16, 16),
}
LANGUAGE_FLAG_ARROW_GAP_DIP = 20
RIBBON_HEIGHT = 150
PAGE_HEIGHT = 118

def prop(e,k,v):
    n=E.SubElement(e,k); n.text=str(v); return n

def obj(parent,cls,name=None,**props):
    n=E.SubElement(parent,'object',{'class':cls,**({'name':name} if name else {})})
    for k,v in props.items(): prop(n,k,v)
    return n

def item(parent,option=0,flag='wxEXPAND',border=0):
    n=obj(parent,'sizeritem');prop(n,'option',option)
    if flag:prop(n,'flag',flag)
    if border:prop(n,'border',border)
    return n

def sizer(parent,orient='wxHORIZONTAL'):return obj(parent,'wxBoxSizer',orient=orient)
def space(parent,w=0,h=0,stretch=0):return obj(parent,'spacer',size=f'{w},{h}',option=stretch)
def size(e,name,w,h):
    prop(e,'minsize',f'{w},{h}')
    SIZES.append((name,w,h))

def label(parent,text,name=None,option=0,flag=None,border=0):
    if flag is None: flag=center_for_sizer(parent)
    return obj(item(parent,option,flag,border),'wxStaticText',name,label=text)

def panel(parent,name,orient='wxHORIZONTAL',option=0,bg=None,border=0,flag='wxEXPAND',style='wxTAB_TRAVERSAL'):
    if bg is None: bg=COLORS['ribbon']
    p=obj(item(parent,option,flag,border),'wxPanel',name,bg=bg,style=style)
    return p,sizer(p,orient)

# The red cross that closes a pane (Styles here; the Equation pane draws the
# same icon at the same size, ribbon::CLOSE_MARK_DIP).
PANE_CLOSE = (28, 28, 20, 20)

def button(parent,name,label_,icon,action,kind='large',tip=None,disabled=None,segment='single',metrics=None):
    w,h,art_w,art_h=metrics or BUTTON_METRICS[kind]
    # Icon-only actions have an empty native label from creation, so a hidden
    # command name cannot enter the native button's cached best-size calculation.
    native_label='' if kind in ('quick','icon','logo') else label_
    b=obj(item(parent,0,center_for_sizer(parent)),'wxButton',name,label=native_label,style='wxBU_EXACTFIT|wxBU_NOTEXT|wxBORDER_NONE')
    size(b,name,w,h)
    fulltip=tip or label_.replace('&','')
    if disabled:
        prop(b,'enabled','0'); fulltip+=' — '+disabled
    prop(b,'tooltip',fulltip)
    BUTTONS.append(dict(name=name,label=label_.replace('&',''),icon=icon,action=action,kind=kind,tip=fulltip,disabled=bool(disabled),width_dip=w,height_dip=h,art_width_dip=art_w,art_height_dip=art_h,segment=segment))
    return b

def combo(parent,name,items,value,width,tip,readonly=False):
    c=obj(item(parent,0,'wxRIGHT|'+center_for_sizer(parent),2),'wxComboBox',name,style='wxCB_READONLY' if readonly else 'wxTE_PROCESS_ENTER')
    size(c,name,width,26); content=E.SubElement(c,'content')
    for x in items:prop(content,'item',x)
    prop(c,'value',value);prop(c,'tooltip',tip)
    COMBOS.append(name); return c

def group(parent,name,title,leading=False):
    flag='wxLEFT|wxTOP|wxBOTTOM|wxEXPAND' if leading else 'wxTOP|wxBOTTOM|wxEXPAND'
    p,outer=panel(parent,name,'wxVERTICAL',border=3,flag=flag,bg=COLORS['group'],style='wxTAB_TRAVERSAL')
    GROUPS.append(name)
    inset=sizer(item(outer,1,'wxTOP|wxBOTTOM|wxEXPAND',3),'wxVERTICAL')
    content=sizer(item(inset,1,'wxLEFT|wxRIGHT|wxEXPAND',9))
    caption=obj(item(outer,0,'wxLEFT|wxRIGHT|wxBOTTOM|wxEXPAND',2),'wxPanel',name+'_caption',bg=COLORS['caption'])
    size(caption,name+'_caption',-1,22)
    cap=sizer(caption)
    txt=label(cap,title,flag='wxALL|wxALIGN_CENTER_VERTICAL',border=1,option=1)
    prop(txt,'style','wxALIGN_CENTRE')
    return content

def rows(parent): return sizer(item(parent,0,center_for_sizer(parent)),'wxVERTICAL')
def row(parent):return sizer(item(parent,0,'wxEXPAND|wxBOTTOM',2))

def segment_at(index,count):
    return 'single' if count == 1 else 'first' if index == 0 else 'last' if index == count-1 else 'middle'

def joined(parent,entries):
    """No per-button margins; one rounded silhouette with shared inner seams."""
    for index,entry in enumerate(entries):
        button(parent,**entry,segment=segment_at(index,len(entries)))

r=E.Element('resource')
f=obj(r,'wxFrame','main_frame',title='Impress — Untitled',size='1440,900')
root=obj(f,'wxPanel','root_panel',bg=COLORS['root'])
ui_font=E.SubElement(root,'font');prop(ui_font,'face','Segoe UI');prop(ui_font,'size','9')
rootbox=sizer(root,'wxVERTICAL')
qat,q=panel(rootbox,'title_strip',bg=COLORS['title'])
size(qat,'title_strip',-1,38)
space(q,10)
button(q,'app_logo_button','Impress','logo','Brand','logo','Impress')
space(q,8)
label(q,'Impress','app_brand',flag='wxALIGN_CENTER_VERTICAL')
space(q,14)
for name,txt,ico,act in [('new_button','New','new','New'),('open_button','Open','open','Open'),('save_button','Save','save','Save'),('undo_button','Undo','undo','Editor(AppCommand::Undo)'),('redo_button','Redo','redo','Editor(AppCommand::Redo)')]:
    button(q,name,txt,ico,act,'quick',txt+' document' if act in ('New','Open','Save') else txt)
space(q,10)
label(q,'Untitled · Impress','document_name_label',flag='wxALIGN_CENTER_VERTICAL')
space(q,0,0,1)
button(q,'interface_language_button','Interface language','interface-language','InterfaceLanguage','quick','Interface language',metrics=(36,28,24,24))
space(q,LANGUAGE_FLAG_ARROW_GAP_DIP)
button(q,'collapse_ribbon_button','Toggle ribbon','chevron-up','ToggleRibbon','quick','Show or hide the Ribbon command area')
button(q,'help_button','Help','help','Help','quick','About this development build')

nb=obj(item(rootbox),'wxNotebook','ribbon_notebook',style='wxNB_TOP')
size(nb,'ribbon_notebook',1,RIBBON_HEIGHT)

def tab(title,name,selected=False):
    p=obj(nb,'notebookpage',label=title)
    if selected:prop(p,'selected','1')
    sc=obj(p,'wxScrolledWindow',name,style='wxHSCROLL|wxTAB_TRAVERSAL',scrollrate='16,0',bg=COLORS['ribbon'])
    size(sc,name,1,PAGE_HEIGHT)
    # One command row per tab; narrower windows scroll horizontally.
    return sizer(sc,'wxHORIZONTAL')

pending='Unavailable in this alpha: the document backend is not wired yet'
file_row=tab('File','ribbon_file')
g=group(file_row,'file_document','Document')
button(g,'file_new_button','New','new','New','file-main')
v=rows(g)
button(v,'file_open_button','Open','open','Open','file-minor')
button(v,'file_save_button','Save','save','Save','file-minor')
v=rows(g)
button(v,'save_as_button','Save as','save_as','SaveAs','file-minor')
button(v,'recover_button','Recover','open','Recovery','file-minor')
button(g,'recent_files_button','Recent','recent','RecentFiles','file-menu',tip='Open a recent file')
g=group(file_row,'file_output','Output')
button(g,'pdf_button','Export PDF','pdf','Pdf','file-main')
button(g,'import_pdf_button','Import PDF','open','ImportPdf','file-main',tip='Convert a PDF into an editable document, choosing pages and options')
v=rows(g)
button(v,'print_button','Print','print','Print','file-minor')
label(file_row,'Native .impress · DOCX · RTF · Typst · PDF import\nOpen and Save As use the registered format adapters.',flag='wxALL|wxALIGN_CENTER_VERTICAL',border=16)

h=tab('Home','ribbon_home',True)
g=group(h,'clipboard_commands','Clipboard')
button(g,'paste_button','Paste','paste','Editor(AppCommand::Paste)','hero',tip='Paste (Ctrl+V)')
v=rows(g)
button(v,'cut_button','Cut','cut','Editor(AppCommand::Cut)','small','Cut (Ctrl+X)')
button(v,'copy_button','Copy','copy','Editor(AppCommand::Copy)','small','Copy (Ctrl+C)')
button(v,'paste_plain_button','Text only','paste_plain','Editor(AppCommand::PastePlain)','small','Paste plain text (Ctrl+Shift+V)')
g=group(h,'font_commands','Font')
v=rows(g);rr=row(v)
combo(rr,'font_family_combo',['Linux Libertine G','Times New Roman','Cambria','Georgia','Calibri','Arial','Segoe UI','Consolas'],'Linux Libertine G',168,'Font family; type a family name and press Enter')
combo(rr,'font_size_combo',['8','9','10','11','12','14','16','18','20','24','28','36','48','72'],'11',56,'Font size in points; type a value and press Enter')
rr=row(v)
joined(rr,[dict(name=n,label_=l,icon=i,action=a,kind='icon',tip=t) for n,l,i,a,t in [
    ('bold_button','Bold','bold','Editor(AppCommand::Bold)','Bold (Ctrl+B)'),
    ('italic_button','Italic','italic','Editor(AppCommand::Italic)','Italic (Ctrl+I)'),
    ('underline_button','Underline','underline','Editor(AppCommand::Underline)','Underline (Ctrl+U)')]])
space(rr,3)
joined(rr,[dict(name=n,label_=l,icon=i,action=a,kind='icon') for n,l,i,a in [
    ('grow_font_button','Grow font','grow','GrowFont'),('shrink_font_button','Shrink font','shrink','ShrinkFont')]])
space(rr,3)
joined(rr,[dict(name='font_color_button',label_='Font color',icon='font_color',action='FontColor',kind='icon'),
    dict(name='highlight_button',label_='Highlight',icon='highlight',action='Highlight',kind='icon')])
g=group(h,'paragraph_commands','Paragraph')
v=rows(g);rr=row(v)
joined(rr,[dict(name='bullets_button',label_='Bullets',icon='bullets',action='Bullets',kind='icon',tip='Toggle bulleted list'),
    dict(name='numbering_button',label_='Numbering',icon='numbering',action='Numbering',kind='icon',tip='Toggle numbered list')])
space(rr,3)
joined(rr,[dict(name='align_'+name+'_button',label_=title,icon='align_'+name,action='Align(ParagraphAlignment::'+title+')',kind='icon',tip='Align '+name) for name,title in [('left','Left'),('center','Center'),('right','Right'),('justify','Justify')]])
space(rr,3)
joined(rr,[dict(name='outdent_button',label_='Decrease indent',icon='outdent',action='Indent(-1.0)',kind='icon'),
    dict(name='indent_button',label_='Increase indent',icon='indent',action='Indent(1.0)',kind='icon')])
rr=row(v)
joined(rr,[dict(name=n,label_=l,icon=i,action='Direction(ParagraphDirection::'+a+')',kind='icon') for n,l,i,a in [
    ('auto_direction_button','Auto direction','auto_direction','Auto'),('ltr_button','Left to right','ltr','LeftToRight'),('rtl_button','Right to left','rtl','RightToLeft')]])
space(rr,5)
combo(rr,'line_spacing_combo',['1','1.15','1.2','1.5','2','2.5','3'],'1.2',86,'Line spacing multiplier; Enter applies a typed value')
g=group(h,'styles_commands','Styles')
style_choices=rows(g)
for name,title,cmd in [('normal','Normal','StyleNormal'),('heading1','Heading 1','StyleHeading1'),('title','Title','StyleTitle')]:button(style_choices,'style_'+name+'_button',title,'style_'+name,'Editor(AppCommand::'+cmd+')','small',title+' paragraph style')
button(style_choices,'style_gallery_more_button','','styles','StyleGalleryMore','gallery-more',tip='More styles')
v=rows(g)
button(v,'style_manager_button','Styles','style_normal','StyleManager','small')
button(v,'clear_direct_button','Clear direct','eraser' if False else 'undo','ClearDirect','small')
g=group(h,'typography_commands','Typography')
v=rows(g)
button(v,'typography_menu_button','Typography','kern','Typography','small',tip='Choose kerning and ligatures for selected text or subsequent typing')
button(v,'hyphenation_button','Hyphenation','spacing','Hyphenation','small')
v=rows(g)
g=group(h,'objects_commands','Objects')
button(g,'home_table_button','Table','table','InsertTable',tip='Insert a semantic table')
button(g,'home_picture_button','Picture','image','InsertImage',tip='Insert a resource-backed inline picture')
g=group(h,'equation_commands','Equation')
button(g,'inline_equation_button','Equation','equation','Editor(AppCommand::InsertEquation)',tip='Insert an equation with the equation editor (Ctrl+Shift+M)')

h=tab('Insert','ribbon_insert')
g=group(h,'insert_equations','Mathematics')
button(g,'insert_equation_button','Equation','equation','Editor(AppCommand::InsertEquation)')
button(g,'symbol_button','Symbol…','symbol','Symbol')
g=group(h,'insert_pages','Pages')
button(g,'page_break_button','Page break','page_break','PageBreak')
button(g,'column_break_button','Column break','columns','ColumnBreak',tip='Continue text in the next column (or page in a one-column section)')
button(g,'section_break_button','Section break','page_break','SectionBreak',tip='Start a continuous, next-page, odd-page, or even-page section')
g=group(h,'insert_objects','Objects')
button(g,'insert_table_button','Table','table','InsertTable',tip='Insert a semantic table')
button(g,'insert_image_button','Picture','image','InsertImage',tip='Insert a resource-backed inline picture')
g=group(h,'insert_header_footer','Header & footer')
button(g,'header_button','Header','header','HeaderFooter(true)',tip='Edit the header of the current section')
button(g,'footer_button','Footer','footer','HeaderFooter(false)',tip='Edit the footer of the current section')
button(g,'page_number_button','Page number','page_number','PageNumber',tip='Insert a centered PAGE field in the current section footer')

h=tab('Layout','ribbon_layout')
g=group(h,'page_setup','Page setup')
button(g,'portrait_button','Portrait','portrait','Landscape(false)')
button(g,'landscape_button','Landscape','landscape','Landscape(true)')
v=rows(g);rr=row(v)
label(rr,'Paper: ',flag='wxALL|wxALIGN_CENTER_VERTICAL',border=3)
combo(rr,'paper_size_combo',['A4','Letter','Legal','A5'],'A4',132,'Choose a paper-size preset for the current section')
rr=row(v);label(rr,'Margins: ',flag='wxALL|wxALIGN_CENTER_VERTICAL',border=3)
combo(rr,'margins_combo',['Normal (25.4 mm)','Narrow (12.7 mm)','Wide (38.1 mm)'],'Normal (25.4 mm)',132,'Choose a preset for all four margins in the current section')
g=group(h,'layout_spacing','Paragraph spacing (points)')
v=rows(g);rr=row(v);label(rr,'Before: ',flag='wxALL|wxALIGN_CENTER_VERTICAL',border=3)
combo(rr,'space_before_combo',['0','3','6','8','12','18','24'],'0',84,'Space before paragraphs, in points')
rr=row(v);label(rr,'After: ',flag='wxALL|wxALIGN_CENTER_VERTICAL',border=3)
combo(rr,'space_after_combo',['0','3','6','8','12','18','24'],'3',84,'Space after paragraphs, in points')
g=group(h,'layout_columns','Columns')
button(g,'columns_button','Columns','columns','Columns',tip='Choose one, two or three columns for the current section')

h=tab('References','ribbon_references')
g=group(h,'document_outline','Outline')
button(g,'outline_button','Headings','navigation','ShowNavigation','wide','Show the heading navigation pane')
g=group(h,'reference_fields','References')
for n,l,i,a in [('toc_button','Contents','references','Contents'),('footnote_button','Footnotes','references','Footnotes'),('bookmark_button','Bookmarks','references','Bookmarks')]:button(g,n,l,i,a,'wide')
button(g,'compatibility_button','Compatibility','review','Compatibility','wide','Review document format limitations')

h=tab('Review','ribbon_review')
g=group(h,'review_tools','Document tools')
button(g,'find_button','Find…','search','Find')
button(g,'word_count_button','Word count','count','WordCount','wide')
g=group(h,'review_pending','Proofing & review')
for n,l,i,a in [('spelling_button','Spelling','review','Spelling'),('comment_button','Comments','comment','Comments'),('changes_button','Text changes','review','TextTracking')]:button(g,n,l,i,a,'wide')

h=tab('View','ribbon_view')
g=group(h,'view_modes_group','Document view')
for n,l,i,mode in [('view_graphical_button','Graphical','portrait','Graphical'),('view_typst_button','Typst source','typst','Typst'),('view_split_button','Split','split','Split')]:button(g,n,l,i,'View(ViewMode::'+mode+')','wide')
g=group(h,'view_zoom','Zoom')
button(g,'view_fit_button','Page width','fit','FitWidth','wide')
button(g,'view_page_button','Whole page','portrait','FitPage','wide')
button(g,'view_100_button','100%','search','Zoom100')
g=group(h,'view_panes','Panes')
button(g,'navigation_button','Navigation','navigation','ToggleNavigation','wide')

h=tab('Typst','ribbon_typst')
g=group(h,'typst_source_group','Generated source')
button(g,'source_button','Show source','typst','View(ViewMode::Typst)','wide')
button(g,'source_split_button','Split view','split','View(ViewMode::Split)','wide')
button(g,'source_refresh_button','Refresh','redo','RefreshSource','wide')
button(g,'source_copy_button','Copy source','copy','CopySource','wide')
label(h,'Read-only Typst export of the graphical document.\nRefresh regenerates the source; it does not import or replace your document.',flag='wxALL|wxALIGN_CENTER_VERTICAL',border=16)

# A hairline under the Ribbon, so its white does not run into the page.
rule=obj(item(rootbox),'wxPanel','ribbon_rule',bg='#d3d9e3');size(rule,'ribbon_rule',-1,1)
body,b=panel(rootbox,'body_panel',option=1,bg=COLORS['workspace'])
nav,nv=panel(b,'navigator_panel','wxVERTICAL',bg=COLORS['source'],style='wxBORDER_SIMPLE|wxTAB_TRAVERSAL')
size(nav,'navigator_panel',250,-1)
label(nv,'Navigation','navigation_title',flag='wxALL',border=10)
q=row(nv)
find=obj(item(q,1,'wxALL|wxEXPAND',4),'wxTextCtrl','navigation_search',style='wxTE_PROCESS_ENTER',tooltip='Literal Unicode search. Leave empty for the multilevel outline.')
button(q,'navigation_go_button','Find','search','SearchNavigation','quick','Search with the current options; empty query shows headings')
rr=row(nv)
button(rr,'search_previous_button','Previous','undo','SearchStep(false)','small')
button(rr,'search_next_button','Next','redo','SearchStep(true)','small')
button(rr,'search_options_button','Options','settings' if False else 'references','SearchOptions','small')
obj(item(nv,0,'wxALL|wxEXPAND',5),'wxTextCtrl','navigation_replace',tooltip='Replacement text. Empty deletes matches.')
rr=row(nv)
button(rr,'replace_one_button','Replace','replace' if False else 'search','ReplaceOne','mode')
button(rr,'replace_all_button','Replace all','search','ReplaceAll','mode')
listbox=obj(item(nv,1,'wxALL|wxEXPAND',5),'wxListBox','navigation_list',style='wxLB_SINGLE|wxLB_HSCROLL')
label(nv,'Headings · click to navigate','navigation_hint',flag='wxALL',border=8)
workspace,ws=panel(b,'workspace_panel','wxVERTICAL',option=1,bg=COLORS['workspace'])
context,ctx=panel(ws,'canvas_context_bar',bg=COLORS['context'],style='wxBORDER_SIMPLE|wxTAB_TRAVERSAL')
size(context,'canvas_context_bar',-1,32)
label(ctx,'Section','section_context_label',flag='wxLEFT|wxALIGN_CENTER_VERTICAL',border=10)
label(ctx,'A4','paper_context_label',flag='wxLEFT|wxALIGN_CENTER_VERTICAL',border=8)
label(ctx,'1 Column','columns_context_label',flag='wxLEFT|wxALIGN_CENTER_VERTICAL',border=8)
space(ctx,0,0,1)
label(ctx,'Normal','style_context_label',flag='wxRIGHT|wxALIGN_CENTER_VERTICAL',border=8)
label(ctx,'Body','editing_context_label',flag='wxRIGHT|wxALIGN_CENTER_VERTICAL',border=8)
label(ctx,'Page 1 of 1','page_context_label',flag='wxRIGHT|wxALIGN_CENTER_VERTICAL',border=10)
# Context tools scroll horizontally rather than widening the document window.
def context_tools(name):
    sc=obj(item(ws),'wxScrolledWindow',name,style='wxHSCROLL|wxTAB_TRAVERSAL',scrollrate='16,0',bg=COLORS['ribbon'])
    size(sc,name,1,76)
    v=sizer(sc,'wxVERTICAL')
    return row(v)
# The Table and Picture tools replace the Insert ribbon page while open; they
# are painted pages (adopt_mockup.py, ribbon/table_bar.rs, picture_bar.rs).
# The Header & Footer strip below only declares its buttons: adopt_mockup.py
# moves them onto a Ribbon page shown in place of Insert.
hc=context_tools('header_context_tools')
# Navigate between the two bands, pick a stream, insert fields, then leave.
button(hc,'header_go_header_button','Header','header','HeaderFooterGo(true)','small',tip='Move to the header of this stream')
button(hc,'header_go_footer_button','Footer','footer','HeaderFooterGo(false)','small',tip='Move to the footer of this stream')
HEADER_VARIANTS=[('default','Default','header-footer','Edit the header or footer used on most pages'),
                 ('first','First page','header-first','Give the first page its own header and footer'),
                 ('even','Even pages','header-even','Give even pages their own header and footer')]
for index,(name,title,icon,tip) in enumerate(HEADER_VARIANTS):button(hc,'header_'+name+'_button',title,icon,f'HeaderVariant({index})','small',tip=tip)
HEADER_FIELDS=[('page_number','Page no.','page-number','Insert the page number'),
               ('page_count','Pages','page-count','Insert the number of pages'),
               ('date','Date','calendar','Insert today\'s date')]
for index,(name,title,icon,tip) in enumerate(HEADER_FIELDS):button(hc,'header_'+name+'_field_button',title,icon,f'InsertField({index})','small',tip=tip)
button(hc,'header_remove_button','Remove','trash','RemoveHeaderFooter','small',tip='Delete this header or footer stream')
button(hc,'header_exit_button','Close','close','ExitHeaderFooter','small',tip='Return to the body text (Esc)')
workrow=sizer(item(ws,1))
canvas_frame,cf=panel(workrow,'canvas_container','wxVERTICAL',option=1,bg=COLORS['canvas'])
ruler=obj(item(cf,0,'wxEXPAND'),'wxPanel','tab_ruler',bg='#e8ecf2',style='wxWANTS_CHARS',tooltip='Click to add a tab stop; drag to move it or drag off to remove it. Double-click for Tabs.')
size(ruler,'tab_ruler',-1,34)
canvas_row=sizer(item(cf,1))
vertical_ruler=obj(item(canvas_row,0,'wxEXPAND'),'wxPanel','vertical_ruler',bg='#e8ecf2',tooltip='Vertical ruler · centimetres')
size(vertical_ruler,'vertical_ruler',34,-1)
cv=obj(item(canvas_row,1),'wxPanel','paper_canvas',bg=COLORS['canvas'],style='wxWANTS_CHARS|wxCLIP_CHILDREN')
obj(item(canvas_row),'wxScrollBar','vertical_scroll',style='wxSB_VERTICAL',value='0',thumbsize='100',range='100',pagesize='100')
obj(item(cf),'wxScrollBar','horizontal_scroll',style='wxSB_HORIZONTAL',value='0',thumbsize='100',range='100',pagesize='100')
# Original is a read-only page preview in the same workspace.
original,original_box=panel(workrow,'original_panel','wxVERTICAL',option=1,bg=COLORS['workspace'])
prop(original,'hidden','1')
original_header=sizer(item(original_box,0,'wxALL|wxEXPAND',8))
label(original_header,'Original · read-only','original_caption',option=1,flag='wxALIGN_CENTER_VERTICAL')
button(original_header,'original_close_button','Close original view','x','ReviewOriginalClose','icon',metrics=PANE_CLOSE)
obj(item(original_box,1),'wxPanel','original_surface',style='wxWANTS_CHARS')
# Split view: the document on the left, its Typst source on the right, with
# a handle between them to drag, like the Styles pane's.
split=obj(item(workrow,0,'wxEXPAND'),'wxPanel','split_handle',bg='#e8ecf2',tooltip='Drag to resize the document and its source')
size(split,'split_handle',6,-1)
source,ss=panel(workrow,'source_panel','wxVERTICAL',option=1,bg=COLORS['source'],style='wxBORDER_SIMPLE|wxTAB_TRAVERSAL')
# Header: the caption, and the panes' close cross, which returns to the
# document alone (Graphical view).
head=sizer(item(ss,0,'wxEXPAND|wxRIGHT',6))
label(head,'Generated Typst · read-only','source_caption',option=1,flag='wxALL|wxALIGN_CENTER_VERTICAL',border=8)
button(head,'source_close_button','Close Typst source','x','View(ViewMode::Graphical)','icon',metrics=PANE_CLOSE)
st=obj(item(ss,1),'wxTextCtrl','typst_editor',style='wxTE_MULTILINE|wxTE_READONLY|wxHSCROLL|wxTE_RICH2',bg=COLORS['source_editor'])
font=E.SubElement(st,'font');prop(font,'face','Consolas');prop(font,'size','10')

# Visual equation editor: one owner-painted surface (formula, palette and
# commands; crates/gui/src/equation_editor) above an optional source field.
ep,es=panel(workrow,'equation_panel','wxVERTICAL',bg=COLORS['source'],style='wxBORDER_NONE|wxTAB_TRAVERSAL')
size(ep,'equation_panel',340,-1)
obj(item(es,1,'wxEXPAND'),'wxPanel','equation_surface',bg=COLORS['source'],style='wxWANTS_CHARS|wxBORDER_NONE')
et=obj(item(es,0,'wxLEFT|wxRIGHT|wxBOTTOM|wxEXPAND',12),'wxTextCtrl','equation_input',style='wxTE_MULTILINE|wxHSCROLL',hidden='1')
size(et,'equation_input',-1,84)
font=E.SubElement(et,'font');prop(font,'face','Consolas');prop(font,'size','10')
# Comments pane: one owner-painted surface (the threads as cards, their
# commands; ribbon/comments_pane.rs) above a field for writing a comment, a
# reply or an edit, shown while one is written.
# Docked document Styles pane, in the Home tab's design language: a bold
# heading with a flat close icon, a 28-DIP filter, a painted style list (names
# previewed in their own fonts, Ribbon-style selection) and outlined commands. Sizing remains logical DIP, like the Ribbon.
PANE_PAD = 12
resize=obj(item(workrow,0,'wxEXPAND'),'wxPanel','styles_resize_handle',bg='#e8ecf2',tooltip='Drag to resize Styles')
size(resize,'styles_resize_handle',6,-1)
sp,sv=panel(workrow,'styles_panel','wxVERTICAL',bg='#ffffff',style='wxBORDER_SIMPLE|wxTAB_TRAVERSAL')
size(sp,'styles_panel',280,-1)
def styles_row(top=0,bottom=8):
    flag='wxLEFT|wxRIGHT|wxEXPAND'
    holder=sizer(item(sv,0,flag,PANE_PAD),'wxVERTICAL')
    if top: space(holder,0,top)
    row_=sizer(item(holder,0,'wxEXPAND'))
    if bottom: space(holder,0,bottom)
    return row_
def bold(e,points=10):
    f=E.SubElement(e,'font');prop(f,'inherit','1');prop(f,'size',points);prop(f,'weight','bold')
rr=styles_row(top=10,bottom=8)
heading=label(rr,'Styles','styles_heading',option=1,flag='wxALIGN_CENTER_VERTICAL')
bold(heading,11);prop(heading,'fg','#243042')
button(rr,'styles_close_button','Close Styles','x','StylesClose','icon',metrics=PANE_CLOSE)
rr=styles_row()
combo(rr,'styles_filter',['All styles','In use','Paragraph styles','Character styles'],'All styles',0,'Styles to display',True)
# The filter fills the inset row; EXPAND does not combine with cross-axis alignment.
rr[-1].find('option').text='1'
rr[-1].find('flag').text='wxEXPAND'
rr[-1].find('border').text='0'
SIZES[-1]=('styles_filter',0,28);rr[-1].find('object').find('minsize').text='0,28'
# Painted list (ribbon/styles.rs): rows in each style's own font, kind marker,
# accent selection; keyboard, wheel and double-click handled in Rust.
sl_row=sizer(item(sv,1,'wxLEFT|wxRIGHT|wxEXPAND',PANE_PAD))
sl=obj(item(sl_row,1,'wxEXPAND'),'wxPanel','styles_list',bg='#ffffff',
       style='wxWANTS_CHARS|wxTAB_TRAVERSAL|wxFULL_REPAINT_ON_RESIZE',tooltip='Click to preview. Double-click or press Enter to apply.')
size(sl,'styles_list',230,160)
scroll=obj(item(sl_row,0,'wxEXPAND'),'wxScrollBar','styles_scroll',style='wxSB_VERTICAL',value='0',thumbsize='100',range='100',pagesize='100')
rr=styles_row(top=10,bottom=10)
obj(item(rr,0,'wxALIGN_CENTER_VERTICAL'),'wxCheckBox','styles_clear_direct',label='Clear direct formatting when applying',checked='0')
visible_styles_sizer=sv
legacy_styles=obj(item(sv,0,'wxEXPAND'),'wxPanel','styles_legacy_commands',hidden='1')
sv=sizer(legacy_styles,'wxVERTICAL')
rr=styles_row(bottom=8)
button(rr,'styles_apply_button','Apply style','accept','StyleApply','pane-default')
slot=rr[-1];slot.find('option').text='1';slot.find('flag').text='wxEXPAND'
for bottom,entries in [(8,[('styles_new_button','New…','new','StyleNew'),('styles_modify_button','Modify…','format','StyleModify')]),
                       (PANE_PAD,[('styles_inspect_button','Inspect…','search','StyleInspect'),('styles_delete_button','Delete…','trash','StyleDelete')])]:
    rr=styles_row(bottom=bottom)
    for index,(name,title,icon,action) in enumerate(entries):
        button(rr,name,title,icon,action,'pane')
        slot=rr[-1];slot.find('option').text='1'
        slot.find('flag').text='wxEXPAND'+('|wxRIGHT' if index+1<len(entries) else '')
        prop(slot,'border','8' if index+1<len(entries) else '0')

sv=visible_styles_sizer
rr=styles_row(bottom=PANE_PAD)
button(rr,'styles_manage_button','Manage Styles…','','ManageStyles','pane',metrics=(220,28,0,0))
rr[-1].find('option').text='1';rr[-1].find('flag').text='wxEXPAND'

# The Reviewing pane lives alongside the canvas and follows the active document.
review_panel,review_box=panel(workrow,'review_panel','wxVERTICAL',style='wxBORDER_SIMPLE|wxTAB_TRAVERSAL')
prop(review_panel,'hidden','1')
size(review_panel,'review_panel',300,-1)
review_content=sizer(item(review_box,1,'wxALL|wxEXPAND',PANE_PAD),'wxVERTICAL')
review_header=sizer(item(review_content,0,'wxEXPAND'))
bold(label(review_header,'Reviewing pane','review_heading',option=1,flag='wxALIGN_CENTER_VERTICAL'),11)
button(review_header,'review_close_button','Close','x','ReviewPane','icon',metrics=PANE_CLOSE)
space(review_content,0,8)
obj(item(review_content,2,'wxEXPAND'),'wxListBox','review_list')
space(review_content,0,8)
obj(item(review_content,1,'wxEXPAND'),'wxTextCtrl','review_details',style='wxTE_MULTILINE|wxTE_READONLY')
space(review_content,0,8)
review_actions=sizer(item(review_content,0,'wxEXPAND'))
for name_,label_,action_ in [('review_accept_button','Accept','RevisionResolve(true)'),('review_reject_button','Reject','RevisionResolve(false)')]:
    button(review_actions,name_,label_,'',action_,'pane',metrics=(90,30,0,0))
    slot=review_actions[-1];slot.find('option').text='1';slot.find('flag').text='wxEXPAND'
    if name_=='review_accept_button': space(review_actions,8)

status,s=panel(rootbox,'status_panel',bg=COLORS['status']);size(status,'status_panel',-1,32)
for index,(n,l,i,mode) in enumerate([('graphical_mode_button','Graphical','portrait','Graphical'),('typst_mode_button','Typst','typst','Typst'),('split_mode_button','Split','split','Split')]):button(s,n,l,i,'View(ViewMode::'+mode+')','mode',segment=segment_at(index,3))
button(s,'text_language_button','Spelling','spell','TextLanguage','small','Spelling options and installed languages',metrics=(112,26,16,16))
status_text=label(s,'Ready','status_label',option=1,flag='wxLEFT|wxALIGN_CENTER_VERTICAL',border=10)
prop(status_text,'style','wxST_ELLIPSIZE_END');size(status_text,'status_label',1,-1)
label(s,'Page 1 of 1','status_page_count',flag='wxLEFT|wxRIGHT|wxALIGN_CENTER_VERTICAL',border=10)
word_count=label(s,'0 words','status_word_count',flag='wxRIGHT|wxALIGN_CENTER_VERTICAL',border=10)
prop(word_count,'tooltip','Word count')
button(s,'status_navigation_button','Navigation','sidebar','ToggleNavigation','quick','Show navigation pane')
button(s,'zoom_out_button','Zoom out','zoom_out','ZoomStep(-0.1)','quick')
z=obj(item(s,0,'wxALIGN_CENTER_VERTICAL'),'wxSlider','zoom_slider',style='wxSL_HORIZONTAL',min='20',max='800',value='100')
size(z,'zoom_slider',140,26)
button(s,'zoom_in_button','Zoom in','zoom_in','ZoomStep(0.1)','quick')
label(s,'100%','zoom_label',flag='wxALL|wxALIGN_CENTER_VERTICAL',border=4)
button(s,'fit_width_button','Page width','fit','FitWidth','quick','Fit page width')


# The supplied design becomes the only visible ribbon. Historical typed
# service widgets are retained offscreen for editor operations they support.
from adopt_mockup import adopt
adopt(globals())
for spec in BUTTONS:
    if spec['name']=='text_language_button': spec['kind']='status-menu'
    elif spec['name'] in ('status_navigation_button','zoom_out_button','zoom_in_button'): spec['kind']='status-quick'
# The adopted layout decides which commands show text. Keep that visible label
# in the painter specification; native buttons must not draw it a second time.
nodes={node.get('name'):node for node in r.iter('object') if node.get('name')}
for spec in BUTTONS:
    native_label=nodes[spec['name']].find('label')
    spec['label']=native_label.text or '' if native_label is not None else ''
    if native_label is not None: native_label.text=''
font_combo=next(node for node in r.iter('object') if node.get('name')=='font_family_combo')
font_combo.set('class','wxPanel')
font_combo.find('style').text='wxTAB_TRAVERSAL'
for child in list(font_combo):
    if child.tag in ('content','value'): font_combo.remove(child)
prop(font_combo,'bg','#ffffff')
field=obj(font_combo,'wxTextCtrl','font_family_input',style='wxTE_PROCESS_ENTER|wxBORDER_NONE',pos='7,3',size='122,22',value='Linux Libertine G')
COMBOS.remove('font_family_combo')

# Reject invalid flags before writing resources, including future vertical stacks.
require_valid_alignment(r)

# Avoid wxDragon's named self-closing XRC object parsing bug.
name_translatable_controls(r)
E.indent(r,space='  ')
(APP/'ui/main.xrc').write_text('<?xml version="1.0" encoding="UTF-8"?>\n'+E.tostring(r,encoding='unicode',short_empty_elements=False)+'\n',encoding='utf-8')
meta=dict(buttons=BUTTONS,sizes=SIZES,groups=GROUPS,combos=COMBOS,button_metrics=BUTTON_METRICS,ribbon_height=RIBBON_HEIGHT,page_height=PAGE_HEIGHT)
(APP/'ui/ribbon-manifest.json').write_text(json.dumps(meta,indent=2,ensure_ascii=False)+'\n',encoding='utf-8')
# Definitions share the same data as the XRC; no field-name drift.
def rs(s):return json.dumps(s,ensure_ascii=False)
lines=['// Generated by tools/build_ribbon_ui.py. Do not edit by hand.', 'use super::{Action, ButtonSpec, ViewMode};', 'use crate::{MainUi, commands::AppCommand};','use model::{ParagraphAlignment, ParagraphDirection};','use wxdragon::{prelude::*, window::WxWidget};','', 'pub fn buttons(ui: &MainUi) -> Vec<ButtonSpec> {', '    vec![']
for b in BUTTONS:
    lines.append('        ButtonSpec { button: ui.'+b['name']+', name: '+rs(b['name'])+', label: '+rs(b['label'])+', icon: '+rs(b['icon'])+', action: Action::'+b['action']+', segment: '+rs(b['segment'])+', kind: '+rs(b['kind'])+', tip: '+rs(b['tip'])+', disabled: '+str(b['disabled']).lower()+', width_dip: '+str(b['width_dip'])+', height_dip: '+str(b['height_dip'])+', art_width_dip: '+str(b['art_width_dip'])+', art_height_dip: '+str(b['art_height_dip'])+' },')
lines+=['    ]','}','', 'pub fn scale_controls(ui: &MainUi) {']
for n,w,h in SIZES: lines.append(f'    ui.{n}.set_min_size(ui.{n}.from_dip(Size::new({w}, {h})));')
lines+=['}', '', 'pub fn install_group_relief(ui: &MainUi) {']
caption_metrics=[]
for index,n in enumerate(GROUPS):
    page=n.split('_',1)[0]
    first=index==0 or GROUPS[index-1].split('_',1)[0]!=page
    last=index+1==len(GROUPS) or GROUPS[index+1].split('_',1)[0]!=page
    lines.append(f'    super::group_relief::install(ui.{n}, {str(first).lower()}, {str(last).lower()});')
    caption=next(node for node in r.iter('object') if node.get('name')==f'{n}_caption')
    title=next(label.text or '' for label in caption.iter('label'))
    # Word-style dialog launcher in the corner of groups that open a pane.
    launcher=str((page,title) in {('home','Styles'),('references','Footnotes'),('layout','Paragraph')}).lower()
    caption_metrics.append((n,title,launcher))
    lines.append(f'    super::group_relief::install_caption(ui.{n}_caption, {rs(title)}, {str(first).lower()}, {str(last).lower()}, {launcher});')
lines+=['}', '', 'pub fn layout_groups(ui: &MainUi) {']
for n,title,launcher in caption_metrics:
    lines.append(f'    super::group_relief::measure_caption(ui.{n}_caption, {rs(title)}, {launcher});')
for n in GROUPS: lines.append(f'    super::group_relief::layout(ui.{n});')
lines+=['}', '', '/// Every Ribbon group panel, for work limited to the visible page.', 'pub fn groups(ui: &MainUi) -> Vec<Panel> {', '    vec![']
for n in GROUPS: lines.append(f'        ui.{n},')
lines+=['    ]', '}', '', 'pub fn group_height(ui: &MainUi) -> i32 {', '    [']
for n in GROUPS: lines.append(f'        ui.{n}.get_best_size().height,')
lines+=['    ].into_iter().max().unwrap_or(0)', '}', '', f'pub const RIBBON_HEIGHT_DIP: i32 = {RIBBON_HEIGHT};', f'pub const PAGE_HEIGHT_DIP: i32 = {PAGE_HEIGHT};', '']
(APP/'src/ribbon').mkdir(exist_ok=True)
(APP/'src/ribbon/generated.rs').write_text('\n'.join(lines),encoding='utf-8')
print(f'Generated {len(BUTTONS)} buttons, 8 real Ribbon pages and {len(GROUPS)} flat groups.')
