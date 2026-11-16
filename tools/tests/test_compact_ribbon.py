#!/usr/bin/env python3
"""Execute resource/geometry contracts; audit native-painter wiring.

These tests do not execute Rust or Windows. The actual face-pixel tests live
in ribbon/face.rs; editor/shaping/layout tests run through check.bat.
"""
import json
from pathlib import Path
import re
import unittest
from support import rust_source
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / 'crates/gui'
UI = json.loads((APP / 'ui/ribbon-manifest.json').read_text())
BY_NAME = {b['name']: b for b in UI['buttons']}
XML = ET.parse(APP / 'ui/main.xrc').getroot()
PARENTS = {child: parent for parent in XML.iter() for child in parent}
NODES = {node.get('name'): node for node in XML.iter('object') if node.get('name')}


class CompactRibbonTests(unittest.TestCase):
    def test_every_segment_has_a_known_role(self):
        self.assertTrue(all(b['segment'] == 'single' for b in UI['buttons']))

    def test_segment_sequences_are_adjacent_children_of_the_same_sizer(self):
        shell = NODES['ribbon_panel_content']
        pages = [item.find('object') for item in shell.find('object').findall('object')]
        # The Table, Header & Footer and Picture tools replace the Insert
        # page while open; the Header & Footer tools add five groups and the
        # Picture tools four.
        contextual = {'table_context_tools', 'header_context_tools', 'picture_context_tools'}
        pages = [page for page in pages if page.get('name') not in contextual]
        self.assertEqual(len(pages), 8)
        self.assertTrue(all(page.get('class') == 'wxScrolledWindow' for page in pages))
        self.assertEqual(len(UI['groups']), 41 + 5)

    def test_formatting_and_style_contract_is_compact(self):
        for spec in UI['buttons']:
            if spec['kind'] in ('icon', 'quick'):
                self.assertGreaterEqual(spec['width_dip'], spec['art_width_dip'] + 4)
        # The visible style gallery is one painted control (ribbon/style_gallery.rs).
        self.assertEqual(NODES['style_gallery'].get('class'), 'wxPanel')
        self.assertIsNone(NODES['style_gallery'].find('object'))
        generator = (ROOT / 'tools/build_ribbon_ui.py').read_text()
        self.assertIn('PAGE_HEIGHT = 118', generator)
        self.assertIn('RIBBON_HEIGHT = 150', generator)

    def test_whole_surface_state_is_not_an_icon_variant(self):
        art = rust_source(APP / 'src/ribbon/art.rs')
        ui = rust_source(APP / 'src/ribbon/mod.rs')
        self.assertNotIn('selected_rgba', art)
        self.assertNotIn('art.selected', ui)
        self.assertIn('skin::selected(&spec.button, selected)', ui)
        self.assertIn('skin::install(spec)', ui)

    def test_public_typography_menu_controls_both_spacing_and_ligatures(self):
        button = next(b for b in UI['buttons'] if b['action'] == 'Typography')
        self.assertFalse(button['disabled'])
        self.assertEqual(button['icon'], 'kern')
        self.assertEqual(button['kind'], 'typography-menu')
        self.assertEqual(button['width_dip'], 132)
        ui = rust_source(APP / 'src/ribbon/mod.rs')
        self.assertIn('editor.set_ligature_mode(mode)', ui)
        self.assertIn('editor.set_kerning_mode(mode)', ui)
        self.assertIn('format.ligatures', ui)
        self.assertIn('format.kerning_mode', ui)
        self.assertIn('show_typography_menu', ui)

    def test_ligature_command_preserves_other_opentype_features(self):
        code = rust_source(ROOT / 'crates/editor/src/formatting.rs')
        self.assertIn('self.commit("Ligatures",mutations,Some(selection),None)', code)
        self.assertIn('typography.ligatures=mode;run.properties.typography=Some(typography)', code)
        self.assertIn('styles.resolve_run(paragraph_style,run.style.as_ref(),&run.properties)', code)
        self.assertNotRegex(code, r'replace\([^\n]*(?:ﬁ|ﬂ|ﬃ)')

    def test_line_layout_shapes_continuous_runs_and_protects_clusters(self):
        code = rust_source(ROOT / 'crates/layout/src/lib.rs')
        self.assertIn('normalized_shaping_properties(styles.resolve_run', code)
        self.assertIn('if props!=next_props {break;}', code)
        self.assertIn('shaped.glyphs.iter().any(|g| g.cluster==*p)', code)
        self.assertIn('shaped_slice(shaped,previous,point)', code)
        self.assertIn('reshape_line_text(&tokens[start..end],fonts)?', rust_source(ROOT / 'crates/layout/src/line_fitting.rs'))
        self.assertLess(code.index('apply_bidi_levels(&mut hard_lines'), code.index('apply_hyphenation(&mut hard_lines'))
        self.assertIn('if source_start==source_end', code)

    def test_rust_tests_use_original_in_memory_fixture_not_installed_fonts(self):
        fixture = ROOT / 'test-support/ligature_font.rs'
        self.assertTrue(fixture.is_file())
        for name in ['layout/src/lib.rs', 'typography/src/shaping.rs']:
            self.assertIn('test-support/ligature_font.rs', rust_source(ROOT / 'crates' / name))
        self.assertNotRegex(fixture.read_text(), r'include_bytes!|load_system_fonts')
        # Neo Aegean compatibility tests intentionally carry the user's family.
        # The synthetic layout/ligature fixture must remain independent of it.
        family_fixtures = ROOT / 'crates/typography/tests/fixtures/neo-aegean'
        self.assertFalse([p for p in ROOT.rglob('*') if 'target' not in p.relative_to(ROOT).parts
                          and family_fixtures not in p.parents
                          and p.suffix.lower() in ('.ttf', '.otf', '.ttc', '.woff', '.woff2')])


if __name__ == '__main__':
    unittest.main(verbosity=2)
