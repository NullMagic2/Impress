#!/usr/bin/env python3
"""Audit literal application messages, without treating document/font data as UI.

The resource audit covers XRC. This additionally follows literal GUI captions,
explicit localization calls, and errors returned by the editing/format crates.
It deliberately excludes the separate English CLI, logging, generated data,
Unicode names, font names, and test fixtures. Dynamic messages still need review.
"""
import json
import re
from pathlib import Path

LITERAL = r'"(?:[^"\\]|\\.)*"'
CALL = re.compile(r'(?:\bErr|\bok_or(?:_else)?|i18n::text|with_label|with_title|set_label|with_message)\s*\(\s*(?:\|\s*\|\s*)?(?:&\s*)?(?:\(\s*)*(?:format!\s*\(\s*)?(?P<literal>' + LITERAL + r')')

def without_comments(source):
    # Preserve literals intact while removing both nested block and line comments.
    out=[]; i=0
    while i < len(source):
        if source.startswith('//',i):
            end=source.find('\n',i); i=len(source) if end<0 else end
        elif source.startswith('/*',i):
            depth=1; i+=2
            while i<len(source) and depth:
                if source.startswith('/*',i):depth+=1;i+=2
                elif source.startswith('*/',i):depth-=1;i+=2
                else:i+=1
            out.append(' ')
        elif source[i]=='"':
            match=re.match(LITERAL,source[i:])
            if match:out.append(match[0]);i+=len(match[0])
            else:out.append(source[i]);i+=1
        else:out.append(source[i]);i+=1
    return ''.join(out)

def runtime_messages(root):
    messages={}
    for path in (root/'crates').glob('*/src/**/*.rs'):
        rel=path.relative_to(root).as_posix()
        if '/bin/' in rel or path.name in {'generated.rs','embedded.rs','symbol_table.rs','widgets.rs','i18n.rs'}:continue
        source=without_comments(path.read_text()).split('#[cfg(test)]',1)[0]
        for match in CALL.finditer(source):
            try:key=json.loads(match['literal'])
            except ValueError:continue
            if not re.search(r'[A-Za-z]{3}',re.sub(r'\{[^{}]*\}', '', key)) or not re.search(r'\s',key):continue
            messages.setdefault(key,set()).add(rel)
    # Impress-specific native file-dialog controls share the embedded catalog.
    cpp=root/'vendor/wxdragon-sys/cpp/src/file_dialog.cpp'
    if cpp.exists():
        for match in re.finditer(r'wxGetTranslation\(\s*('+LITERAL+')',cpp.read_text()):
            messages.setdefault(json.loads(match[1]),set()).add(cpp.relative_to(root).as_posix())
    # Ribbon captions are painted from this manifest, rather than read from
    # native XRC labels. Disabled commands still need translated captions.
    manifest=root/'crates/gui/ui/ribbon-manifest.json'
    if manifest.exists():
        for button in json.loads(manifest.read_text()).get('buttons',[]):
            for field in ['label','tip']:
                key=button.get(field,'')
                if key and key not in {'Impress','Typst','PDF','PNG','SVG','HTML'}:
                    messages.setdefault(key,set()).add(manifest.relative_to(root).as_posix())
    return messages

def missing_messages(root,catalog,derive=lambda key:None):
    return {key:sorted(paths) for key,paths in runtime_messages(root).items()
            if key not in catalog and derive(key) is None}


# Coverage and display routing are separate checks: a catalog entry can exist
# while a custom button setter still passes English directly to wxWidgets.
LABEL_CALL = re.compile(r'(?P<method>\.(?:with_label|with_title|with_message|set_label|set_title|set_tooltip|set_hint|set_yes_no_labels|set_ok_cancel_labels)|MessageDialog::builder|DialogBuilder::new)\s*\(')

def arguments(source,start):
    """Split one Rust call without splitting format macros or quoted commas."""
    args=[];begin=start;i=start;stack=[]
    while i<len(source):
        if source[i]=='"':
            match=re.match(LITERAL,source[i:])
            if match:i+=len(match[0]);continue
        char=source[i]
        if char in '([{':stack.append(char)
        elif char in ')]}':
            if not stack:
                if char==')':args.append(source[begin:i]);return args
                return []
            stack.pop()
        elif char==',' and not stack:args.append(source[begin:i]);begin=i+1
        i+=1
    return []

def direct_caption(argument):
    value=argument.strip()
    while value.startswith('&'):value=value[1:].strip()
    while value.startswith('(') and value.endswith(')'):value=value[1:-1].strip()
    if re.fullmatch(LITERAL,value):return json.loads(value)
    match=re.match(r'format!\s*\(\s*('+LITERAL+')',value)
    return json.loads(match[1]) if match else None

def unlocalized_labels(root):
    bypasses=[]
    for path in (root/'crates/gui/src').rglob('*.rs'):
        if path.name in {'generated.rs','embedded.rs','widgets.rs'}:continue
        source=without_comments(path.read_text()).split('#[cfg(test)]',1)[0]
        for call in LABEL_CALL.finditer(source):
            method=call['method'];args=arguments(source,call.end())
            indices=([0,1] if method.endswith(('_labels',)) else
                     [1,2] if method=='MessageDialog::builder' else
                     [1] if method=='DialogBuilder::new' else [0])
            for index in indices:
                if index>=len(args):continue
                caption=direct_caption(args[index])
                if caption and re.search(r'[A-Za-z]{3}',re.sub(r'\{[^{}]*\}','',caption)):
                    bypasses.append({'path':path.relative_to(root).as_posix(),
                                     'method':method,'argument':index,'text':caption})
    return bypasses

if __name__=='__main__':
    root=Path(__file__).resolve().parents[1]
    catalog=json.loads((root/'crates/gui/assets/locales/translations.json').read_text())
    missing=missing_messages(root,catalog)
    bypasses=unlocalized_labels(root)
    print(json.dumps({'missing_messages':missing,'unlocalized_labels':bypasses},ensure_ascii=False,indent=2))
    raise SystemExit(bool(missing or bypasses))
