#!/usr/bin/env python3
"""Contracts for the Modify Style Format ▾ dialogs (no wxWidgets needed).

- the generators reproduce the checked-in XRC exactly;
- every control the Rust code uses exists in its dialog's XRC;
- every dialog is embedded in main.rs and has the shared chrome (card, OK);
- the Format menu lists Word's nine items in Word's order;
- sizer flags are valid (xrc_sizer_checks).
"""
import re
import sys
import tempfile
import unittest
from support import rust_source
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / 'crates/gui'
sys.path.insert(0, str(ROOT / 'tools'))
import dialog_kit  # noqa: E402
import support  # noqa: F401  (puts tools/ on the import path)
from xrc_sizer_checks import alignment_errors  # noqa: E402

DIALOGS = {'fnt': 'format_font.xrc', 'par': 'format_paragraph.xrc', 'tab': 'format_tabs.xrc', 'bdr': 'format_borders.xrc',
           'shd': 'format_borders.xrc', 'lng': 'format_language.xrc', 'frm': 'format_frame.xrc', 'num': 'format_numbering.xrc',
           'key': 'format_shortcut.xrc', 'fx': 'format_text_effects.xrc'}


def names(filename):
    root = ET.parse(APP / 'ui' / filename).getroot()
    return {o.get('name') for o in root.iter('object') if o.get('name')}


class FormatDialogTests(unittest.TestCase):
    def test_generators_reproduce_checked_in_xrc(self):
        import importlib
        with tempfile.TemporaryDirectory() as temp:
            original = dialog_kit.UI
            dialog_kit.UI = Path(temp)
            try:
                builder = importlib.import_module('build_format_dialogs')
                for fn in ('font_dialog', 'paragraph_dialog', 'tabs_dialog', 'borders_dialog', 'language_dialog',
                           'frame_dialog', 'numbering_dialog', 'shortcut_dialog', 'text_effects_dialog'):
                    getattr(builder, fn)()
            finally:
                dialog_kit.UI = original
            for generated in Path(temp).glob('format_*.xrc'):
                self.assertEqual(generated.read_bytes(), (APP / 'ui' / generated.name).read_bytes(), generated.name)

    def test_rust_uses_only_existing_controls(self):
        source = rust_source(APP / 'src/ribbon/format_dialogs.rs')
        for prefix, filename in DIALOGS.items():
            available = names(filename)
            for used in set(re.findall(rf'\bui\.({prefix}_[a-z0-9_]+)', source)):
                self.assertIn(used, available, f'{used} missing from {filename}')

    def test_dialogs_are_embedded_with_shared_chrome(self):
        main = rust_source(APP / 'src/main.rs')
        for prefix, filename in DIALOGS.items():
            if prefix == 'shd':
                continue
            self.assertIn(f'"../ui/{filename}"', main)
            available = names(filename)
            for part in ('dialog', 'card', 'ok'):
                self.assertIn(f'{prefix}_{part}', available)
            root = ET.parse(APP / 'ui' / filename).getroot()
            self.assertEqual(alignment_errors(root), [], filename)

    def test_format_menu_follows_word(self):
        source = rust_source(APP / 'src/ribbon/format_dialogs.rs')
        labels = re.findall(r'FormatItem::\w+ => "([^"]+…)"', source)
        self.assertEqual(labels, ['Font…', 'Paragraph…', 'Tabs…', 'Border…', 'Language…', 'Frame…', 'Numbering…',
                                  'Shortcut key…', 'Text Effects…'])

    def test_modify_style_has_no_inline_format_panels(self):
        available = names('style_editor.xrc')
        for gone in ('style_format_panel', 'style_font_panel', 'style_paragraph_panel', 'style_tab_stops'):
            self.assertNotIn(gone, available)


if __name__ == '__main__':
    unittest.main(verbosity=2)
