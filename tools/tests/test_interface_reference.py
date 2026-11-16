"""Keep connected HTML tabs and native Rust pages in sync."""
from html.parser import HTMLParser
from pathlib import Path
import json
import unittest
from support import rust_source
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
TABS = ['File', 'Home', 'Insert', 'Layout', 'References', 'Review', 'View', 'Typst']

class Reference(HTMLParser):
    def __init__(self):
        super().__init__(); self.tabs=[]; self.rows=[]; self.controls=set()
    def handle_starttag(self, tag, attrs):
        data=dict(attrs)
        if tag=='button' and data.get('role')=='tab': self.tabs.append(data.get('data-tab'))
        if 'ribbon-row' in data.get('class','').split(): self.rows.append(data.get('data-tab'))
        if data.get('data-control'): self.controls.add(data['data-control'])

class InterfaceReferenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.html=(ROOT/'design/interface-reference.html').read_text(encoding='utf-8')
        cls.reference=Reference(); cls.reference.feed(cls.html)
        cls.xrc=ET.parse(ROOT/'crates/gui/ui/main.xrc').getroot()
        cls.layout=json.loads((ROOT/'design/interface-layout.json').read_text(encoding='utf-8'))

    def test_one_connected_tab_per_ribbon_row(self):
        self.assertEqual(self.reference.tabs,TABS)
        self.assertEqual(self.reference.rows,TABS)
        self.assertIn('border-bottom: 0;',self.html)
        self.assertIn('margin-bottom: -1px;',self.html)
        self.assertIn('border-top-color: transparent;',self.html)
        self.assertIn('border-top: 0;',self.html)
        self.assertIn('background: var(--ribbon); border-color: var(--line);',self.html)
        self.assertIn('border-radius: 5px 5px 0 0;',self.html)
        self.assertIn('border-radius: 5px; margin-bottom: -1px;',self.html)
        self.assertNotIn('.tabs button.active::before',self.html)
        self.assertNotIn('.tabs button.active::after',self.html)
        self.assertIn('.ribbon-row { display: flex; flex-wrap: nowrap;',self.html)

    def test_native_pages_match_reference(self):
        nodes={node.get('name'):node for node in self.xrc.iter('object') if node.get('name')}
        self.assertEqual(nodes['ribbon_notebook'].get('class'),'wxPanel')
        self.assertEqual(nodes['ribbon_tab_strip'].get('class'),'wxPanel')
        # The tab strip paints the names from the manifest; the native buttons have none.
        manifest=json.loads((ROOT/'crates/gui/ui/ribbon-manifest.json').read_text(encoding='utf-8'))
        labels={b['name']:b['label'] for b in manifest['buttons'] if b['kind']=='tab'}
        self.assertEqual([labels[f'ribbon_{tab.lower()}_tab'] for tab in TABS],TABS)
        self.assertTrue(all(nodes[f'ribbon_{tab.lower()}'].get('class')=='wxScrolledWindow' for tab in TABS))
        self.assertEqual([tab['title'] for tab in self.layout],TABS)
        self.assertEqual(sum(len(tab['groups']) for tab in self.layout),41)

    def test_native_details_match_reference(self):
        nodes={node.get('name'):node for node in self.xrc.iter('object') if node.get('name')}
        self.assertEqual(nodes['app_logo_button'].get('class'),'wxButton')
        self.assertEqual(nodes['style_gallery'].get('class'),'wxPanel')
        self.assertEqual(nodes['typography_menu_button'].get('class'),'wxButton')
        self.assertNotIn('kerning_mode_combo',nodes)
        self.assertNotIn('ligatures_button',nodes)
        self.assertIn('id="typography-menu" role="menu"',self.html)
        self.assertIn('data-setting="kerning"',self.html)
        self.assertIn('data-setting="ligatures"',self.html)
        self.assertIn('.typography-picker > .small-button { width: 132px; }',self.html)
        self.assertNotIn('style_gallery_frame',nodes)
        self.assertNotIn('ribbon_panel_border',nodes)
        self.assertIn('.gallery { display: flex; gap: 5px;',self.html)
        self.assertIn('width: 1px; background: var(--group-divider);',self.html)
        for name in ('cut_button','copy_button','paste_plain_button'):
            self.assertEqual(nodes[name].findtext('label'),'')
        self.assertIn('face::render_tab(',rust_source(ROOT/'crates/gui/src/ribbon/skin.rs'))

    def test_mock_controls_exist_in_native_interface(self):
        names={node.get('name') for node in self.xrc.iter('object')}
        citation=ET.parse(ROOT/'crates/gui/ui/citation_dialog.xrc').getroot()
        names.update(node.get('name') for node in citation.iter('object'))
        self.assertGreater(len(self.reference.controls),140)
        self.assertFalse(self.reference.controls-names)

    def test_palette_and_document_font(self):
        tokens=json.loads((ROOT/'design/interface-tokens.json').read_text(encoding='utf-8'))
        for name in ('root','title','ribbon','workspace','canvas','context','status'):
            self.assertIn(f'--{name}: {tokens[name]}',self.html)
        self.assertIn('Linux Libertine G',self.html)

    def test_supplied_clipboard_icons_have_native_assets(self):
        manifest=json.loads((ROOT/'crates/gui/ui/ribbon-manifest.json').read_text(encoding='utf-8'))
        buttons={button['name']:button for button in manifest['buttons']}
        for name in ('paste_button','cut_button','copy_button','paste_plain_button'):
            icon=buttons[name]['icon']
            self.assertIn(f'data-control="{name}"',self.html)
            self.assertTrue((ROOT/f'crates/gui/assets/ribbon/{icon}.png').is_file())

if __name__=='__main__': unittest.main()
