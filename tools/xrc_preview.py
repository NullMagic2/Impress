#!/usr/bin/env python3
"""Render the Format ▾ dialogs' XRC as a static HTML design preview.

design/format-dialogs.html is generated from the same XRC the app loads, so
layout, grouping and sizes match; controls are drawn in the interface
reference's style (Windows 11 fields, Ribbon-skin buttons and tabs). Panels the
app paints in Rust (previews, tile pickers) get canvas approximations here.

Usage: python tools/xrc_preview.py   (writes design/format-dialogs.html)
"""
from pathlib import Path
import base64
import re
import html
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / 'crates/gui/ui'
ICONS = ROOT / 'crates/gui/assets/ribbon'
DIALOGS = ['format_font.xrc', 'format_paragraph.xrc', 'format_tabs.xrc', 'format_borders.xrc', 'format_language.xrc',
           'format_frame.xrc', 'format_numbering.xrc', 'format_shortcut.xrc', 'format_text_effects.xrc']
# Lists and colour boxes the app fills at runtime, shown with typical content.
PREVIEW_ITEMS = {
    'lng_list': ['Dutch (Netherlands)  ·  nl-NL', 'English (Australia)  ·  en-AU', 'English (Canada)  ·  en-CA',
                 'English (United Kingdom)  ·  en-GB', 'English (United States)  ·  en-US', 'Finnish  ·  fi-FI',
                 'French (Canada)  ·  fr-CA', 'French (France)  ·  fr-FR', 'German (Germany)  ·  de-DE'],
    'key_commands': ['Heading 1', 'Normal', 'Title'], 'tab_list': ['25 mm', '80 mm', '150 mm'],
    'fnt_family_list': ['Linux Libertine G', 'Times New Roman', 'Cambria', 'Georgia', 'Calibri', 'Arial', 'Segoe UI', 'Consolas'],
}
PREVIEW_SELECTED = {'lng_list': 4, 'key_commands': 1, 'fnt_size_list': 3}
# Values the app sets when a dialog opens (for a Normal-like style).
PREVIEW_VALUES = {'fnt_family': 'Linux Libertine G', 'fnt_style': 'Regular', 'fnt_size': '11', 'fnt_scale': '100%',
                  'bdr_width': '½ pt', 'lng_tag': 'en-US', 'tab_position': '25'}
# Controls the app hides once the dialog has content (empty-state hints).
PREVIEW_HIDDEN = set()
NO_COLOUR_BOXES = {'fnt_highlight', 'shd_fill'}
TOOL_ICONS = {'bdr_edge_top': 'style-border-top', 'bdr_edge_bottom': 'style-border-bottom', 'bdr_edge_left': 'style-border-left',
              'bdr_edge_right': 'style-border-right', 'bdr_edge_between': 'style-border-between'}
CHEVRON = ("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%236b778a' "
           "stroke-width='2.6' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E")


def prop(obj, key, default=None):
    value = obj.find(key)
    return value.text if value is not None and value.text is not None else default


def size_of(obj):
    raw = prop(obj, 'size')
    if not raw:
        return None, None
    w, h = (int(v) for v in raw.split(','))
    return (w if w > 0 else None), (h if h > 0 else None)


def flags(entry):
    return set((prop(entry, 'flag', '') or '').replace(' ', '').split('|')) - {''}


def margin_css(entry):
    border = int(prop(entry, 'border', '0') or 0)
    f = flags(entry)
    if not border:
        return ''
    sides = {'top': 'wxTOP', 'right': 'wxRIGHT', 'bottom': 'wxBOTTOM', 'left': 'wxLEFT'}
    return ''.join(f'margin-{side}:{border}px;' for side, flag in sides.items() if 'wxALL' in f or flag in f)


class Renderer:
    def __init__(self, page):
        self.page = page  # which <prefix>_page_<n> is visible

    def children(self, obj):
        return [c for c in obj.findall('object')]

    def sizer(self, obj, parent_kind=None):
        cls = obj.get('class')
        if cls == 'wxFlexGridSizer':
            cols = int(prop(obj, 'cols', '1'))
            growable = {int(c) for c in (prop(obj, 'growablecols', '') or '').split(',') if c.strip()}
            template = ' '.join('minmax(0,1fr)' if i in growable else 'max-content' for i in range(cols))
            style = (f"display:grid;grid-template-columns:{template};row-gap:{prop(obj, 'vgap', '0')}px;"
                     f"column-gap:{prop(obj, 'hgap', '0')}px;align-items:center;")
            inner = ''.join(self.entry(e, 'grid') for e in self.children(obj))
            return f'<div style="{style}">{inner}</div>'
        orient = prop(obj, 'orient', 'wxHORIZONTAL')
        kind = 'row' if orient == 'wxHORIZONTAL' else 'col'
        style = f"display:flex;flex-direction:{'row' if kind == 'row' else 'column'};min-width:0;"
        inner = ''.join(self.entry(e, kind) for e in self.children(obj))
        return f'<div style="{style}">{inner}</div>'

    def entry(self, entry, kind):
        cls = entry.get('class')
        if cls == 'spacer':
            w, h = size_of(entry)
            grow = prop(entry, 'option', '0')
            return f'<div style="flex:{grow} 0 auto;width:{w or 0}px;height:{h or 0}px"></div>'
        child = entry.find('object')
        if child is None:
            return ''
        f = flags(entry)
        grow = int(prop(entry, 'option', '0') or 0)
        css = margin_css(entry)
        if kind in ('row', 'col'):
            css += f'flex:{grow} {1 if grow else 0} auto;min-width:0;'
            if 'wxEXPAND' in f:
                css += 'align-self:stretch;'
            elif kind == 'row' and 'wxALIGN_CENTER_VERTICAL' in f:
                css += 'align-self:center;'
            elif kind == 'row' and 'wxALIGN_BOTTOM' in f:
                css += 'align-self:flex-end;'
            elif kind == 'col':
                css += 'align-self:flex-start;'
        else:
            css += 'justify-self:stretch;' if 'wxEXPAND' in f else 'justify-self:start;'
            css += 'align-self:center;'
        minimum = prop(entry, 'minsize')
        if minimum:
            mw = int(minimum.split(',')[0])
            if mw > 0:
                css += f'min-width:{mw}px;'
        name = child.get('name', '')
        if name.endswith(tuple(f'_page_{i}' for i in range(9))) and not name.endswith(f'_page_{self.page}'):
            return ''
        return f'<div style="{css}display:flex;flex-direction:column">{self.control(child)}</div>'

    def control(self, obj):
        cls = obj.get('class')
        name = obj.get('name', '')
        w, h = size_of(obj)
        dims = (f'width:{w}px;' if w else '') + (f'height:{h}px;' if h else '')
        if name in PREVIEW_HIDDEN:
            return ''
        if cls in ('wxBoxSizer', 'wxFlexGridSizer'):
            return self.sizer(obj)
        if cls == 'wxPanel':
            bg = prop(obj, 'bg', 'transparent')
            inner = ''.join(self.control(c) for c in self.children(obj))
            if name.endswith('_card'):
                return f'<div class="card">{inner}</div>'
            if prop(obj, 'style', '') and 'wxWANTS_CHARS' in prop(obj, 'style', ''):
                return f'<canvas class="painted" data-paint="{name}" style="{dims}flex:1 1 auto;min-height:{h or 60}px"></canvas>'
            if not inner:
                return f'<div style="{dims}background:{bg};flex:none"></div>'
            return f'<div style="background:{bg};display:flex;flex-direction:column;flex:1 1 auto">{inner}</div>'
        if cls == 'wxStaticText':
            label = html.escape(prop(obj, 'label', '') or '')
            weight = '700' if obj.find('font/weight') is not None else '400'
            colour = prop(obj, 'fg', '#243042')
            wrap = prop(obj, 'wrap')
            flow = f'white-space:normal;width:{wrap}px;' if wrap else 'white-space:nowrap;'
            return f'<span style="color:{colour};font-weight:{weight};{dims}{flow}">{label}</span>'
        if cls == 'wxTextCtrl':
            value = html.escape(PREVIEW_VALUES.get(name, ""))
            style = prop(obj, 'style', '') or ''
            if 'wxTE_MULTILINE' in style:
                read_only = ' readonly' if 'wxTE_READONLY' in style else ''
                return f'<textarea class="field multiline" style="{dims}"{read_only}>{value}</textarea>'
            return f'<input class="field" style="{dims}" value="{value}">'
        if cls == 'wxComboBox':
            items = [i.text or '' for i in obj.findall('content/item')]
            disabled = ' disabled' if prop(obj, 'enabled') == '0' else ''
            chosen = PREVIEW_VALUES.get(name)
            options = ''.join(f'<option{" selected" if i == chosen else ""}>{html.escape(i)}</option>' for i in items)
            return f'<select class="field select" style="{dims}"{disabled}>{options}</select>'
        if cls == 'wxBitmapComboBox':
            empty = name in NO_COLOUR_BOXES
            return (f'<div class="field select colour" style="{dims}" data-colour="{name}"><span class="chip{" none" if empty else ""}"></span>'
                    f'<span class="colour-name">{"No Color" if empty else "Automatic"}</span></div>')
        if cls == 'wxSpinCtrlDouble':
            digits = int(prop(obj, 'digits', '0') or 0)
            value = float(prop(obj, 'value', '0') or 0)
            return f'<div class="field spin" style="{dims}"><span>{value:.{digits}f}</span><i></i></div>'
        if cls == 'wxCheckBox':
            return f'<label class="check"><input type="checkbox">{html.escape(prop(obj, "label", ""))}</label>'
        if cls == 'wxRadioButton':
            group = name.rsplit('_', 1)[0]
            checked = ' checked' if prop(obj, 'style', '') == 'wxRB_GROUP' else ''
            return f'<label class="check"><input type="radio" name="{group}"{checked}>{html.escape(prop(obj, "label", ""))}</label>'
        if cls == 'wxListBox':
            items = [i.text or '' for i in obj.findall('content/item')] or PREVIEW_ITEMS.get(name, [])
            chosen = PREVIEW_SELECTED.get(name, 0)
            rows = ''.join(f'<div class="{"sel" if index == chosen else ""}">{html.escape(i)}</div>' for index, i in enumerate(items))
            return f'<div class="list" style="{dims}" data-list="{name}">{rows}</div>'
        if cls == 'wxButton':
            label = html.escape(prop(obj, 'label', '') or '')
            if '_tab_' in name:
                selected = ' selected' if name.endswith('_tab_0') else ''
                return f'<button class="tab{selected}" style="{dims}">{label}</button>'
            if name in TOOL_ICONS:
                data = base64.b64encode((ICONS / f'{TOOL_ICONS[name]}.png').read_bytes()).decode()
                return f'<button class="tool" style="background-image:url(data:image/png;base64,{data})" title="{label}"></button>'
            primary = ' primary' if name.endswith('_ok') else ''
            return f'<button class="btn{primary}" style="{dims}">{label}</button>'
        return ''

    def dialog(self, obj):
        title = html.escape(prop(obj, 'title', ''))
        body = ''.join(self.control(c) for c in self.children(obj))
        return f'<div class="dialog"><div class="title">{title}</div>{body}</div>'


def pages_of(root):
    return sum(1 for o in root.iter('object') if re.search(r'_page_\d+$', o.get('name', ''))) or 1


def build():
    sections = []
    for filename in DIALOGS:
        root = ET.parse(UI / filename).getroot()
        dialog = root.find('object')
        for page in range(pages_of(root)):
            name = f"{dialog.get('name')}_{page}"
            sections.append(f'<section id="{name}">{Renderer(page).dialog(dialog)}</section>')
    return TEMPLATE.replace('<!--DIALOGS-->', '\n'.join(sections)).replace('%CHEVRON%', CHEVRON)


TEMPLATE = r'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Format dialogs — Impress</title>
<!-- Generated by tools/xrc_preview.py from crates/gui/ui/format_*.xrc. Do not edit by hand. -->
<style>
  :root { --ink:#243042; --muted:#6b778a; --line:#dfe4ec; --line-strong:#cad3df; --accent:#3a68c2; --accent-soft:#e5edfc;
          --accent-line:#b5c8ee; --hover:#eef2f8; --title:#e8ecf2; }
  body { margin:0; padding:24px; background:#dde2ea; font:12.5px "Segoe UI",system-ui,sans-serif; color:var(--ink);
         display:flex; flex-wrap:wrap; gap:24px; align-items:flex-start; }
  section { display:inline-block; }
  .dialog { background:var(--title); border:1px solid var(--line-strong); border-radius:8px; box-shadow:0 18px 44px #1b2a4030;
            display:inline-flex; flex-direction:column; }
  .title { height:34px; display:flex; align-items:center; padding:0 14px; }
  .card { background:#fff; border:1px solid var(--line); border-radius:7px; display:flex; flex-direction:column; flex:1 1 auto; }
  .field { height:28px; box-sizing:border-box; border:1px solid var(--line-strong); border-radius:6px; background:#fff;
           padding:0 9px; font:inherit; color:var(--ink); min-width:0; }
  .field.multiline { padding:6px 9px; resize:none; line-height:1.4; }
  .field.multiline[readonly] { background:#f7f9fb; }
  .select { appearance:none; background:#fff url("%CHEVRON%") no-repeat right 8px center / 10px; padding-right:26px; }
  select.select:disabled { background-color:#f7f9fb; color:var(--muted); border-color:var(--line); }
  .colour { display:flex; align-items:center; gap:8px; }
  .chip { width:18px; height:12px; background:#000; flex:none; }
  .chip.none { background:#fff linear-gradient(to top right, transparent 45%, #c42b1c 45%, #c42b1c 55%, transparent 55%); border:1px solid #aab2be; box-sizing:border-box; }
  .spin { display:flex; align-items:center; justify-content:space-between; padding-right:4px; }
  .spin i { width:10px; height:16px; background:linear-gradient(#fff,#fff) padding-box;
            border-left:1px solid var(--line); position:relative; }
  .spin i::before, .spin i::after { content:""; position:absolute; left:2px; border:3px solid transparent; }
  .spin i::before { top:1px; border-bottom-color:var(--muted); } .spin i::after { bottom:1px; border-top-color:var(--muted); }
  .check { display:flex; align-items:center; gap:6px; white-space:nowrap; }
  .check input { accent-color:var(--accent); margin:0; width:14px; height:14px; }
  .list { border:1px solid var(--line-strong); border-radius:6px; background:#fff; overflow:hidden; box-sizing:border-box; padding:3px; }
  .list div { padding:3px 7px; border-radius:4px; white-space:nowrap; }
  .list div.sel { background:var(--accent-soft); }
  .btn { height:28px; box-sizing:border-box; border:1px solid #cad3df; border-radius:5px; background:#fff; font:inherit; color:var(--ink); padding:0 6px; white-space:nowrap; }
  .btn.primary { background:var(--accent); border-color:var(--accent); color:#fff; }
  .tab { height:30px; border:0; background:transparent; font:inherit; color:var(--ink); padding:0; white-space:nowrap; border-radius:6px 6px 0 0; }
  .tab.selected { background:#fff; }
  .tool { width:28px; height:28px; border:1px solid transparent; border-radius:5px; background:#fff center / 16px no-repeat; }
  .tool.on { background-color:var(--accent-soft); border-color:var(--accent-line); }
  canvas.painted { display:block; width:100%; }
  section:target .dialog { outline:3px solid var(--accent); outline-offset:4px; }
</style></head><body>
<!--DIALOGS-->
<script>
// Canvas approximations of the panels format_dialogs.rs paints.
const LINE='#dfe4ec', STRONG='#cad3df', INK='#243042', MUTED='#6b778a', GHOST='#a3aebf', ACCENT='#3a68c2', SOFT='#e5edfc', ALINE='#b5c8ee';
function round(ctx,x,y,w,h,r,fill,stroke){ctx.beginPath();ctx.roundRect(x+.5,y+.5,w-1,h-1,r);if(fill){ctx.fillStyle=fill;ctx.fill();}if(stroke){ctx.strokeStyle=stroke;ctx.lineWidth=1;ctx.stroke();}}
function lines(ctx,x,y,w,h,rows){const g=h/(rows+1);ctx.fillStyle=STRONG;for(let r=0;r<rows;r++)ctx.fillRect(x,y+g*(r+1)-1,r==rows-1?w*.6:w,2);}
function tiles(ctx,W,H,count,cols,th,draw,sel){const gap=6,ins=6,w=(W-2*ins-(cols-1)*gap)/cols;for(let i=0;i<count;i++){const c=i%cols,r=Math.floor(i/cols),x=ins+c*(w+gap),y=ins+r*(th+gap);round(ctx,x,y,w,th,5,i==sel?SOFT:'#fff',i==sel?ALINE:LINE);draw(x,y,w,th,i);}}
function text(ctx,t,x,y,font,colour,align){ctx.font=font;ctx.fillStyle=colour||INK;ctx.textAlign=align||'left';ctx.textBaseline='middle';ctx.fillText(t,x,y);}
function para(ctx,W,H){const z=.6,m=Math.max(W*.05,14);ctx.save();ctx.textBaseline='alphabetic';
  const run=(t,y,size,colour,weight,rows,partial)=>{ctx.font=`${weight} ${size*z*1.333}px "Linux Libertine G",Georgia,serif`;ctx.fillStyle=colour;const words=t.split(' ');let line='',n=0;
    for(let i=0;i<400&&n<rows;i++){const wd=words[i%words.length],test=line?line+' '+wd:wd;if(ctx.measureText(test).width>W-2*m){ctx.fillText(line,m,y);y+=size*1.2*z*1.333;n++;line=wd;}else line=test;}
    return y;};
  let y=12+11*z*1.333;y=run('Previous Paragraph',y,11,GHOST,400,2);y+=19*z*1.333;y=run('Sample Text',y,11,'#1b2533',400,3);y+=3*z*1.333;run('Following Paragraph',y,11,GHOST,400,20);ctx.restore();}
const painters={
  fnt_preview:(c,W,H)=>{round(c,0,0,W,H,5,'#fff',STRONG);c.font='26px "Linux Libertine G",Georgia,serif';const t='Linux Libertine G',w=c.measureText(t).width;
    c.strokeStyle=STRONG;c.beginPath();c.moveTo(10,H/2+9);c.lineTo((W-w)/2-6,H/2+9);c.moveTo((W+w)/2+6,H/2+9);c.lineTo(W-10,H/2+9);c.stroke();text(c,t,W/2,H/2,'26px "Linux Libertine G",Georgia,serif',INK,'center');},
  par_preview:(c,W,H)=>{round(c,0,0,W,H,5,'#fff',STRONG);para(c,W,H);},
  bdr_setting:(c,W,H)=>{round(c,0,0,W,H,5,'#fff',LINE);tiles(c,W,H,5,1,42,(x,y,w,h,i)=>{const ix=x+6,iy=y+5,iw=28,ih=h-10;c.fillStyle='#fff';c.strokeStyle=LINE;c.strokeRect(ix+.5,iy+.5,iw,ih);lines(c,ix+5,iy+2,iw-10,ih-4,3);
    c.strokeStyle=INK;if(i==1||i==3)c.strokeRect(ix+2.5,iy+2.5,iw-4,ih-4);if(i==2){c.fillStyle=GHOST;c.fillRect(ix+5,iy+5,iw-4,ih-4);c.fillStyle='#fff';c.fillRect(ix+2,iy+2,iw-6,ih-6);c.strokeRect(ix+2.5,iy+2.5,iw-6,ih-6);}
    text(c,['None','Box','Shadow','3-D','Custom'][i],ix+iw+8,y+h/2,'12px "Segoe UI"');},1);},
  bdr_style_list:(c,W,H)=>{round(c,0,0,W,H,5,'#fff',LINE);const styles=[[],[1,2],[4,2],[6,2,1,2],[6,2,1,2,1,2],'d','tt','ttk','w'];
    tiles(c,W,H,9,1,14,(x,y,w,h,i)=>{c.strokeStyle=INK;c.lineWidth=1;const s=styles[i],yy=y+h/2+.5;c.setLineDash(Array.isArray(s)?s:[]);c.beginPath();
      if(s=='d'||s=='tt'||s=='ttk'){c.lineWidth=s=='tt'?2:1;c.moveTo(x+8,yy-2);c.lineTo(x+w-8,yy-2);c.stroke();c.beginPath();c.lineWidth=s=='ttk'?2:1;c.moveTo(x+8,yy+2);c.lineTo(x+w-8,yy+2);}
      else if(s=='w'){for(let t=0;t<w-16;t+=3)c.lineTo(x+8+t,yy+((t/3)%2?2:-1));}else{c.moveTo(x+8,yy);c.lineTo(x+w-8,yy);}c.stroke();c.setLineDash([]);c.lineWidth=1;},0);},
  bdr_preview:(c,W,H)=>{round(c,0,0,W,H,5,'#fff',STRONG);const m=22,mid=H/2;lines(c,m+8,m,W-2*m-16,mid-m-4,3);lines(c,m+8,mid+2,W-2*m-16,H-mid-m-4,3);
    c.strokeStyle=GHOST;c.beginPath();for(const[x,y,dx,dy]of[[m,m,-6,-6],[W-m,m,6,-6],[m,H-m,-6,6],[W-m,H-m,6,6]]){c.moveTo(x+dx,y);c.lineTo(x,y);c.moveTo(x,y+dy);c.lineTo(x,y);}c.stroke();},
  shd_preview:(c,W,H)=>{round(c,0,0,W,H,5,'#fff',STRONG);c.strokeStyle=LINE;c.strokeRect(16.5,16.5,W-33,H-33);lines(c,26,20,W-52,H-40,5);},
  frm_wrapping:(c,W,H)=>{round(c,0,0,W,H,5,'#fff',LINE);tiles(c,W,H,2,2,80,(x,y,w,h,i)=>{const ix=x+14,iy=y+8,iw=w-28,ih=h-30,bw=iw/3,bx=ix+(iw-bw)/2,by=iy+ih/3;
    c.fillStyle=SOFT;c.fillRect(bx,by,bw,ih/3);c.strokeStyle=ACCENT;c.strokeRect(bx+.5,by+.5,bw,ih/3);c.fillStyle=STRONG;
    for(let r=0;r<6;r++){const ly=iy+r*ih/6+ih/12,beside=ly>=by&&ly<=by+ih/3;if(beside){if(i==1){c.fillRect(ix,ly,bx-ix-3,2);c.fillRect(bx+bw+3,ly,ix+iw-bx-bw-3,2);}}else c.fillRect(ix,ly,iw,2);}
    text(c,['None','Around'][i],x+w/2,y+h-12,'12px "Segoe UI"',INK,'center');},0);},
  num_bullets:(c,W,H)=>{round(c,0,0,W,H,5,'#fff',LINE);const b=['None','●','○','■','◆','➢','✓','–'];tiles(c,W,H,8,4,100,(x,y,w,h,i)=>{
    if(!i){text(c,'None',x+w/2,y+h/2,'14px "Segoe UI"',INK,'center');return;}for(let r=1;r<=3;r++){const cy=y+h/4*r;text(c,b[i],x+12,cy,'13px "Segoe UI"');c.fillStyle=STRONG;c.fillRect(x+38,cy-1,w-52,2);}},1);},
  num_numbers:(c,W,H)=>{round(c,0,0,W,H,5,'#fff',LINE);const n=[[],['1.','2.','3.'],['1)','2)','3)'],['I.','II.','III.'],['A.','B.','C.'],['a)','b)','c)'],['a.','b.','c.'],['i.','ii.','iii.']];
    tiles(c,W,H,8,4,100,(x,y,w,h,i)=>{if(!i){text(c,'None',x+w/2,y+h/2,'14px "Segoe UI"',INK,'center');return;}for(let r=1;r<=3;r++){const cy=y+h/4*r;text(c,n[i][r-1],x+12,cy,'13px "Segoe UI"');c.fillStyle=STRONG;c.fillRect(x+38,cy-1,w-52,2);}},1);},
  fx_preview:(c,W,H)=>{round(c,0,0,W,H,5,'#fff',STRONG);text(c,'AaBbCc',W/2,H/2,'bold 34px "Linux Libertine G",Georgia,serif',INK,'center');},
};
painters.fnt_preview_advanced=painters.fnt_preview;painters.par_preview_breaks=painters.par_preview;painters.fx_preview_effects=painters.fx_preview;
document.querySelectorAll('canvas.painted').forEach(cv=>{const r=cv.getBoundingClientRect(),d=devicePixelRatio;cv.width=r.width*d;cv.height=r.height*d;
  const ctx=cv.getContext('2d');ctx.scale(d,d);(painters[cv.dataset.paint]||((c,W,H)=>round(c,0,0,W,H,5,'#fff',LINE)))(ctx,r.width,r.height);});
// Edge buttons shown with the Box setting's four sides on.
['bdr_edge_top','bdr_edge_bottom','bdr_edge_left','bdr_edge_right'];
</script></body></html>
'''

if __name__ == '__main__':
    target = ROOT / 'design/format-dialogs.html'
    target.write_text(build(), encoding='utf-8')
    print(target)
