#!/usr/bin/env python3
"""Host-side regression checks for the two reported Ribbon startup contracts.

Standard library only. XML tests exercise the real generator and validator.
The FFI tests audit Rust source and native flag values; they DO NOT execute
Rust, wxWidgets, or a Windows GUI. Native-direction Rust unit tests are in
crates/gui/src/ribbon/bitmap_button.rs and run with check.bat.
"""
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from support import rust_source
import xml.etree.ElementTree as ET

import support  # noqa: F401  (puts tools/ on the import path)
from xrc_sizer_checks import alignment_errors, center_for_sizer, require_valid_alignment

ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / 'crates/gui'


def fixture(orientation, flags, nested=None):
    root = ET.Element('resource')
    sizer = ET.SubElement(root, 'object', {'class': 'wxBoxSizer'})
    ET.SubElement(sizer, 'orient').text = orientation
    item = ET.SubElement(sizer, 'object', {'class': 'sizeritem'})
    ET.SubElement(item, 'flag').text = flags
    child = ET.SubElement(item, 'object', {'class': 'wxBoxSizer' if nested else 'wxButton', 'name': 'test_child'})
    if nested:
        ET.SubElement(child, 'orient').text = nested
    return root, sizer


class XrcAlignmentTests(unittest.TestCase):
    def test_all_generated_box_sizers_use_valid_flags(self):
        root = ET.parse(APP / 'ui/main.xrc').getroot()
        self.assertEqual(alignment_errors(root), [])

    def test_original_cut_and_copy_flags_are_detected(self):
        # Reintroduce the exact defect into the complete current XRC tree.
        root = ET.parse(APP / 'ui/main.xrc').getroot()
        for item in root.iter('object'):
            child = item.find('object')
            if item.get('class') == 'sizeritem' and child is not None and child.get('name') in ('cut_button', 'copy_button'):
                item.find('flag').text = 'wxALL|wxALIGN_CENTER_VERTICAL'
        problems = alignment_errors(root)
        self.assertEqual(len(problems), 2)
        self.assertTrue(any('cut_button:' in p for p in problems))
        self.assertTrue(any('copy_button:' in p for p in problems))
        with self.assertRaises(ValueError):
            require_valid_alignment(root)

    def test_centering_helper_uses_the_containing_sizer(self):
        for orientation, expected in [('wxVERTICAL', 'wxALIGN_CENTER_HORIZONTAL'),
                                      ('wxHORIZONTAL', 'wxALIGN_CENTER_VERTICAL')]:
            _, sizer = fixture(orientation, '')
            self.assertEqual(center_for_sizer(sizer), expected)

    def test_centering_helper_rejects_non_sizer_parents(self):
        with self.assertRaises(ValueError):
            center_for_sizer(ET.Element('object', {'class': 'wxPanel'}))

    def test_cross_axis_alignment_is_accepted(self):
        for orientation, flags in [('wxVERTICAL', 'wxALL|wxALIGN_CENTER_HORIZONTAL'),
                                   ('wxHORIZONTAL', 'wxALL|wxALIGN_CENTER_VERTICAL')]:
            root, _ = fixture(orientation, flags)
            self.assertEqual(alignment_errors(root), [])

    def test_same_axis_alignment_is_rejected_in_both_orientations(self):
        for orientation, flags in [('wxVERTICAL', 'wxALIGN_CENTER_VERTICAL'),
                                   ('wxHORIZONTAL', 'wxALIGN_CENTER_HORIZONTAL'),
                                   ('wxVERTICAL', 'wxALIGN_BOTTOM'),
                                   ('wxHORIZONTAL', 'wxALIGN_RIGHT')]:
            root, _ = fixture(orientation, flags)
            self.assertTrue(alignment_errors(root), (orientation, flags))

    def test_nested_sizer_orientation_does_not_override_its_parent(self):
        root, _ = fixture('wxHORIZONTAL', 'wxALIGN_CENTER_VERTICAL', nested='wxVERTICAL')
        self.assertEqual(alignment_errors(root), [])
        root, _ = fixture('wxVERTICAL', 'wxALIGN_CENTER_VERTICAL', nested='wxHORIZONTAL')
        self.assertTrue(alignment_errors(root))

    def test_centre_aliases_and_generic_center(self):
        root, _ = fixture('wxVERTICAL', 'wxALIGN_CENTRE_VERTICAL')
        self.assertTrue(alignment_errors(root))
        for orientation in ('wxHORIZONTAL', 'wxVERTICAL'):
            for flag in ('wxALIGN_CENTER', 'wxALIGN_CENTRE'):
                root, _ = fixture(orientation, flag)
                self.assertEqual(alignment_errors(root), [])

    def test_margin_directions_are_not_alignment_flags(self):
        root, _ = fixture('wxVERTICAL', 'wxALL|wxTOP|wxBOTTOM|wxLEFT|wxRIGHT|wxEXPAND')
        self.assertEqual(alignment_errors(root), [])

    def test_expansion_and_cross_axis_alignment_conflict(self):
        for orientation, flags in [('wxVERTICAL', 'wxEXPAND|wxALIGN_CENTER_HORIZONTAL'),
                                   ('wxHORIZONTAL', 'wxGROW|wxALIGN_CENTER_VERTICAL')]:
            root, _ = fixture(orientation, flags)
            self.assertTrue(any('expansion conflicts' in p for p in alignment_errors(root)))

    def test_conflicting_alignment_flags_are_rejected(self):
        root, _ = fixture('wxVERTICAL', 'wxALIGN_RIGHT|wxALIGN_CENTER_HORIZONTAL')
        self.assertTrue(any('conflicting' in p for p in alignment_errors(root)))

    def test_generator_reproduces_checked_in_output(self):
        with tempfile.TemporaryDirectory(prefix='impress-xrc-test-') as name:
            temp = Path(name)
            (temp / 'tools').mkdir()
            (temp / 'design').mkdir()
            app = temp / 'crates/gui'
            (app / 'ui').mkdir(parents=True)
            (app / 'src').mkdir()
            for script in ('build_ribbon_ui.py', 'adopt_mockup.py', 'xrc_sizer_checks.py', 'localization_kit.py'):
                shutil.copyfile(ROOT / 'tools' / script, temp / 'tools' / script)
            shutil.copyfile(ROOT / 'design/interface-tokens.json', temp / 'design/interface-tokens.json')
            shutil.copyfile(ROOT / 'design/interface-layout.json', temp / 'design/interface-layout.json')
            subprocess.run([sys.executable, str(temp / 'tools/build_ribbon_ui.py')],
                           check=True, capture_output=True, text=True)
            for resource in ('ui/main.xrc', 'ui/ribbon-manifest.json', 'src/ribbon/generated.rs'):
                self.assertEqual((app / resource).read_bytes(), (APP / resource).read_bytes(), resource)


class BitmapDirectionContractTests(unittest.TestCase):
    def test_native_mapping_is_explicit_and_exhaustive(self):
        source = rust_source(APP / 'src/ribbon/bitmap_button.rs')
        mappings = dict(re.findall(r'ButtonBitmapPosition::(\w+)\s*=>\s*ffi::(WXD_\w+)\s+as ffi::wxd_Direction_t', source))
        self.assertEqual(mappings, {'Left': 'WXD_LEFT', 'Right': 'WXD_RIGHT',
                                    'Top': 'WXD_TOP', 'Bottom': 'WXD_BOTTOM'})

    def test_flag_values_satisfy_wxwidgets_but_old_ordinals_do_not(self):
        # Documented native constants, not a GUI simulation or a Rust execution.
        directions = (0x10, 0x20, 0x40, 0x80)
        mask = 0xf0
        valid = lambda value: (value & ~mask) == 0 and (value & mask).bit_count() == 1
        self.assertTrue(all(valid(value) for value in directions))
        self.assertTrue(all(not valid(value) for value in range(4)))
        self.assertFalse(valid(directions[0] | directions[2]))

    def test_both_initial_and_state_update_paths_use_the_bridge(self):
        # Ribbon faces are installed and updated in one place, ButtonFace::update.
        source = rust_source(APP / 'src/ribbon/skin.rs')
        self.assertEqual(source.count('bitmap_button::set_bundle('), 1)
        self.assertEqual(source.count('set_bitmap_bundle_disabled('), 1)
        for path in (APP / 'src').rglob('*.rs'):
            self.assertNotRegex(path.read_text(), r'\.set_bitmap_bundle\s*\(', str(path))

    def test_bridge_uses_typed_widget_pointer_and_native_direction(self):
        source = rust_source(APP / 'src/ribbon/bitmap_button.rs')
        self.assertIn('button.handle_ptr()', source)
        self.assertIn('window.is_null() || !bundle.is_ok()', source)
        self.assertIn('window.cast::<ffi::wxd_Button_t>()', source)
        self.assertIn('bundle.as_ptr()', source)
        self.assertIn('native_direction(position),', source)
        self.assertNotIn('transmute', source)
        self.assertNotIn('.get_handle()', source)

    def test_sizing_uses_installed_artwork_not_placeholder_native_getter(self):
        source = rust_source(APP / 'src/ribbon/mod.rs')
        self.assertIn('self.fit_button(spec, true)', source)
        self.assertIn('skin::label_size(spec)', source)
        self.assertNotIn('button.get_bitmap()', source)

    def test_existing_ribbon_command_and_artwork_contract_is_preserved(self):
        ui = json.loads((APP / 'ui/ribbon-manifest.json').read_text())
        generated = rust_source(APP / 'src/ribbon/generated.rs')
        self.assertEqual(generated.count('ButtonSpec {'), len(ui['buttons']))
        assets = json.loads((APP / 'assets/ribbon/manifest.json').read_text())
        self.assertEqual(rust_source(APP / 'src/ribbon/embedded.rs').count('=> Some('), len(assets))
        source = rust_source(APP / 'src/ribbon/art.rs')
        self.assertIn('BitmapBundle::from_bitmaps(&normal)', source)
        self.assertIn('[100, 125, 150, 175, 200, 250, 300]', source)


if __name__ == '__main__':
    unittest.main(verbosity=2)
