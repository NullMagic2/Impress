#!/usr/bin/env python3
"""Static Ribbon integration checks; not a replacement for cargo check/test.
Uses Python's standard library. Add --lexical for a Pygments delimiter audit.
"""
from pathlib import Path
import argparse
import json
import re
import struct
import tomllib
import xml.etree.ElementTree as ET
from xrc_sizer_checks import require_valid_alignment

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / 'crates/gui'


def validate(lexical=False):
    results = []
    manifests = list(ROOT.rglob('Cargo.toml'))
    parsed = {path: tomllib.loads(path.read_text(encoding='utf-8')) for path in manifests}
    workspace = parsed[ROOT/'Cargo.toml']['workspace']
    for member in workspace['members']:
        assert (ROOT/member/'Cargo.toml').is_file(), member
    for path, manifest in parsed.items():
        for name, dependency in manifest.get('dependencies', {}).items():
            if isinstance(dependency, dict) and 'path' in dependency:
                assert (path.parent/dependency['path']/'Cargo.toml').is_file(), (path, name)
    wx = workspace['dependencies']['wxdragon']
    assert wx['version'] == '=0.9.22' and wx['features'] == ['xrc'], wx
    results.append(f'{len(manifests)} Cargo manifests parse; local members/dependencies exist; wxDragon 0.9.22 XRC pinned')

    xml = (APP/'ui/main.xrc').read_text()
    tree = ET.fromstring(xml)
    require_valid_alignment(tree)
    results.append("Box-sizer flags checked against the containing sizer orientation")
    named = [obj for obj in tree.iter('object') if 'name' in obj.attrib]
    fields = {obj.attrib['name']: obj.attrib['class'] for obj in named}
    assert len(named) == len(fields), 'duplicate XRC object names'
    assert not re.search(r'<object\b[^>]*name="[^"]+"[^>]*/>', xml), 'named self-closing object'
    supported = {'wxFrame','wxPanel','wxButton','wxStaticText','wxComboBox','wxBitmapComboBox','wxScrollBar',
                 'wxScrolledWindow','wxNotebook','wxListBox','wxTextCtrl','wxSlider','wxCheckBox'}
    assert set(fields.values()) <= supported
    assert fields['font_family_combo'] == 'wxPanel' and fields['font_family_input'] == 'wxTextCtrl'
    assert all('wxTE_PROCESS_ENTER' not in obj.findtext('style', '')
               for obj in tree.iter('object') if obj.get('class') == 'wxBitmapComboBox')
    captions = [obj for obj in named if obj.get('name', '').endswith('_group_caption')]
    assert len(captions) == 42 and all(obj.findtext('minsize') == '-1,18' for obj in captions)
    assert 'wxStyledTextCtrl' not in xml
    tabs = ['File','Home','Insert','Layout','References','Review','View','Typst']
    assert fields['ribbon_notebook'] == 'wxPanel' and fields['ribbon_tab_strip'] == 'wxPanel'
    assert fields['ribbon_panel_content'] == 'wxPanel'
    assert all(fields[f'ribbon_{tab.lower()}_tab'] == 'wxButton' for tab in tabs)
    assert all(fields[f'ribbon_{tab.lower()}'] == 'wxScrolledWindow' for tab in tabs)
    results.append(f'{len(fields)} unique typed XRC fields; 8 painted tabs and matched native ribbon pages')

    ui_manifest = json.loads((APP/'ui/ribbon-manifest.json').read_text())
    buttons = ui_manifest['buttons']
    names = {b['name'] for b in buttons}
    assert names == {name for name, cls in fields.items() if cls == 'wxButton'}
    generated = (APP/'src/ribbon/generated.rs').read_text()
    assert generated.count('ButtonSpec {') == len(buttons)
    rust_files = [path for path in ROOT.rglob('*.rs') if 'target' not in path.relative_to(ROOT).parts]
    dialog_fields = {}
    for dialog in (APP/'ui').glob('*.xrc'):
        if dialog.name == 'main.xrc':
            continue
        dialog_xml = ET.parse(dialog).getroot()
        require_valid_alignment(dialog_xml)
        dialog_fields[dialog.stem] = {obj.attrib['name'] for obj in dialog_xml.iter('object') if 'name' in obj.attrib}
    all_fields = fields.keys() | set().union(*dialog_fields.values())
    for rust in (APP/'src').rglob('*.rs'):
        for field in re.findall(r'\bui\.([A-Za-z_]\w*)', rust.read_text(encoding='utf-8')):
            assert field in all_fields, (rust.name, field)
    for group in ui_manifest['groups']:
        node = next(obj for obj in named if obj.attrib['name'] == group)
        assert 'wxBORDER_SIMPLE' not in node.findtext('style', '')
    results.append(f'{len(buttons)} named buttons match typed command specs; {len(ui_manifest["groups"])} flat groups; no dangling MainUi field references')
    enabled = sum(not b['disabled'] for b in buttons)
    results.append(f'{enabled} enabled button instances; {len(buttons)-enabled} visibly disabled backend-pending instances')

    assets = json.loads((APP/'assets/ribbon/manifest.json').read_text())
    assets_by_name = {asset['name']: asset for asset in assets}
    assert all(b['icon'] in assets_by_name for b in buttons)
    embedded = (APP/'src/ribbon/embedded.rs').read_text()
    for asset in assets:
        name = asset['name']
        png = (APP/f'assets/ribbon/{name}.png').read_bytes()
        assert png.startswith(b'\x89PNG\r\n\x1a\n')
        assert struct.unpack('>II', png[16:24]) == (asset['width'], asset['height'])
        assert f'"{name}" => Some(' in embedded
    for rust in rust_files:
        for path in re.findall(r'include_(?:bytes|str)!\(\s*"([^"]+)"', rust.read_text(encoding='utf-8')):
            assert (rust.parent/path).is_file(), (rust, path)
    assert not [f for f in ROOT.rglob('*') if 'target' not in f.relative_to(ROOT).parts and f.suffix.lower() in ('.ttf','.otf','.woff','.woff2','.ttc')]
    results.append(f'{len(assets)} embedded PNG assets have valid headers and dimensions; no font binaries in source package')

    main = (APP/'src/main.rs').read_text()
    assert 'include_xrc!("../ui/main.xrc", MainUi)' in main
    assert 'MainUi::new(None, false)' in main
    assert 'Ribbon::prepare(&ui)' in main and 'Ribbon::new(' in main
    assert re.search(r'drop\(state\);\s*if close_ready \{timer_frame.close\(false\);return;\}\s*timer_ribbon.poll\(\)', main)
    assert 'bind_ribbon_commands' not in main and 'view_modes' not in main
    assert (APP/'src/ribbon/art.rs').read_text().count('#[path = "embedded.rs"]') == 1
    results.append('Startup prepares new hierarchy; timer releases editor borrow before Ribbon updates; old scaffold binding removed')

    if lexical:
        from pygments import lex
        from pygments.lexers import RustLexer
        from pygments.token import Comment, String
        pairs = {')':'(', ']':'[', '}':'{'}
        for rust in rust_files:
            stack = []
            for kind, text in lex(rust.read_text(), RustLexer()):
                if (kind in Comment and kind not in Comment.Preproc) or kind in String:
                    continue
                # RustLexer is not a Rust parser. Lifetimes and newer syntax may
                # be generic tokens, but delimiters outside literals still pair.
                for char in text:
                    if char in '([{': stack.append(char)
                    elif char in ')]}':
                        assert stack and stack.pop() == pairs[char], (rust, char)
            assert not stack, (rust, stack)
        results.append(f'{len(rust_files)} Rust files pass lexical delimiter checks (NOT compilation or type checking)')
    return results


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lexical', action='store_true')
    args = parser.parse_args()
    for line in validate(args.lexical):
        print('PASS:', line)
    print('NOT RUN: Cargo check/test or native Windows UI verification.')
