#!/usr/bin/env python3
"""Embed the native toolbar PNGs into the standalone HTML reference."""
from pathlib import Path
import base64

ROOT=Path(__file__).resolve().parents[1]
HTML=ROOT/'design/interface-reference.html'
ASSETS=ROOT/'crates/gui/assets/ribbon'
ICONS=[
    'bold','italic','underline',
    'style-align-left','style-align-center','style-align-right','style-align-justify',
    'style-line-single','style-line-onehalf','style-line-double',
    'style-spacing-increase','style-spacing-decrease','style-indent-decrease','style-indent-increase',
]
start='  /* STYLE_TOOLBAR_ICON_DATA_START */'
end='  /* STYLE_TOOLBAR_ICON_DATA_END */'
html=HTML.read_text(encoding='utf-8')
assert html.count(start)==html.count(end)==1
rules=[]
for name in ICONS:
    data=base64.b64encode((ASSETS/f'{name}.png').read_bytes()).decode('ascii')
    rules.append(f'  .sd-tool[data-sd-icon="{name}"] {{ background-image: url("data:image/png;base64,{data}"); }}')
prefix,rest=html.split(start,1)
_,suffix=rest.split(end,1)
HTML.write_text(prefix+start+'\n'+'\n'.join(rules)+'\n'+end+suffix,encoding='utf-8')
