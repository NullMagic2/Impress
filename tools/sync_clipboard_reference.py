#!/usr/bin/env python3
"""Embed the native Python-drawn Clipboard PNGs in the standalone HTML reference."""
from pathlib import Path
import base64
import re

ROOT = Path(__file__).resolve().parents[1]
HTML = ROOT / 'design/interface-reference.html'
ART = ROOT / 'crates/gui/assets/ribbon'
content = HTML.read_text(encoding='utf-8')

for name in ('paste', 'cut', 'copy', 'paste_plain'):
    png = (ART / f'{name}.png').read_bytes()
    data_url = 'data:image/png;base64,' + base64.b64encode(png).decode('ascii')
    pattern = rf'(<img\b[^>]*\bdata-art="{name}"[^>]*\bsrc=")[^"]*(")'
    content, count = re.subn(pattern, lambda match: match[1] + data_url + match[2], content)
    if count != 1:
        raise SystemExit(f'Expected one HTML image for {name}; found {count}')

HTML.write_text(content, encoding='utf-8')
print('Embedded four native Clipboard icons in the HTML reference')
