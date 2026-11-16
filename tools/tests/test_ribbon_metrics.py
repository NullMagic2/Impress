#!/usr/bin/env python3
"""Host-side regression checks for Ribbon resources and sizing contracts.

Requires Pillow; these are NOT native widget or Rust execution tests.
Run `python tools/tests/test_ribbon_metrics.py` from an extracted source package.
"""
import json
from pathlib import Path
import re
import unittest
from support import rust_source
import xml.etree.ElementTree as ET
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / 'crates/gui'
UI = json.loads((APP / 'ui/ribbon-manifest.json').read_text())
ASSETS = json.loads((APP / 'assets/ribbon/manifest.json').read_text())
SCALES = (100, 125, 150, 175, 200, 250, 300)
INSET = 2


def scaled(dip, percent):
    return max(1, (dip * percent + 50) // 100)


def source_pixels(name):
    with Image.open(APP / f'assets/ribbon/{name}.png') as image:
        return image.convert('RGBA').tobytes()


def has_art(button):
    return button['art_width_dip'] > 0 or button['art_height_dip'] > 0


def alpha_bounds(raw, width, threshold=8):
    points = [i for i, a in enumerate(raw[3::4]) if a > threshold]
    if not points:
        return None
    return (min(i % width for i in points), min(i // width for i in points),
            max(i % width for i in points) + 1, max(i // width for i in points) + 1)


class RibbonMetricsTests(unittest.TestCase):
    def test_source_assets_have_unpainted_outer_pixels(self):
        for asset in ASSETS:
            with self.subTest(asset=asset['name']):
                raw = source_pixels(asset['name'])
                w, h = asset['width'], asset['height']
                self.assertEqual(len(raw), w * h * 4)
                alpha = raw[3::4]
                border = alpha[:w] + alpha[-w:] + alpha[::w] + alpha[w-1::w]
                self.assertEqual(max(border), 0)
                self.assertIsNotNone(alpha_bounds(raw, w))

    def test_action_icon_ink_is_contained_and_has_optical_padding(self):
        icons = [asset for asset in ASSETS if not asset['name'].startswith('style_')]
        self.assertGreaterEqual(len(icons), 100)
        for asset in icons:
            with self.subTest(asset=asset['name']):
                self.assertEqual((asset['width'], asset['height']), (64, 64))
                raw = source_pixels(asset['name'])
                x0, y0, x1, y1 = alpha_bounds(raw, 64)
                self.assertGreaterEqual(min(x0, y0, 64-x1, 64-y1), 1)

    def test_gallery_ink_has_room_on_all_sides(self):
        styles = [asset for asset in ASSETS if asset['name'].startswith('style_')]
        self.assertEqual(len(styles), 3)
        for asset in styles:
            with self.subTest(asset=asset['name']):
                self.assertEqual((asset['width'], asset['height']), (224, 72))
                raw = source_pixels(asset['name'])
                x0, y0, x1, y1 = alpha_bounds(raw, 224)
                self.assertGreaterEqual(min(x0, 224-x1), 16)
                self.assertGreaterEqual(min(y0, 72-y1), 10)

    def test_logical_metrics_are_consistent_for_each_button_kind(self):
        for button in UI['buttons']:
            self.assertTrue(button['width_dip'] > 0 and button['height_dip'] > 0)
            if not has_art(button):
                continue  # The gallery's More strip paints a mark, not an icon.
            self.assertTrue(button['art_width_dip'] > 0 and button['art_height_dip'] > 0)
            self.assertGreaterEqual(button['width_dip'], button['art_width_dip'] + 4)
            self.assertGreaterEqual(button['height_dip'], button['art_height_dip'] + 4)

    def test_icon_guard_band_fits_button_at_all_seven_scales(self):
        for button in UI['buttons']:
            if button['kind'] == 'tab' or not has_art(button):
                continue  # Painted text tabs and the More strip have no icon.
            for percent in SCALES:
                with self.subTest(button=button['name'], scale=percent):
                    aw = scaled(button['art_width_dip'] + 2 * INSET, percent)
                    ah = scaled(button['art_height_dip'] + 2 * INSET, percent)
                    inset = scaled(INSET, percent)
                    self.assertGreater(aw, 2 * inset)
                    self.assertGreater(ah, 2 * inset)
                    self.assertGreaterEqual(scaled(button['width_dip'], percent), aw)
                    self.assertGreaterEqual(scaled(button['height_dip'], percent), ah)

    def test_large_buttons_reserve_label_line_and_padding(self):
        # A conservative 16-DIP text-line estimate, not a native font measurement.
        # Runtime measures text and grows the control if the actual font needs more room.
        # The Windows painter owns padding; fallback native buttons use their best size.
        for button in UI['buttons']:
            if button['kind'] not in ('gallery', 'large', 'wide'):
                continue
            with self.subTest(button=button['name']):
                required = button['art_height_dip'] + 4 + 16 + 2 * 5
                self.assertGreaterEqual(button['height_dip'], required)

    def test_icon_only_native_labels_are_empty_but_specs_retain_names(self):
        tree = ET.parse(APP / 'ui/main.xrc')
        nodes = {node.get('name'): node for node in tree.iter('object') if node.get('name')}
        for button in UI['buttons']:
            # Icon-only buttons name themselves through their tooltip, which
            # is also their accessible description.
            self.assertTrue(button['label'] or button['tip'])
            if button['kind'] in ('icon', 'quick'):
                self.assertEqual(nodes[button['name']].findtext('label') or '', '')

    def test_group_captions_have_reserved_height(self):
        tree = ET.parse(APP / 'ui/main.xrc')
        nodes = {node.get('name'): node for node in tree.iter('object') if node.get('name')}
        for name in UI['groups']:
            with self.subTest(group=name):
                self.assertNotIn('wxBORDER_SIMPLE', nodes[name].findtext('style'))
                self.assertEqual(nodes[name + '_caption'].findtext('minsize'), '-1,18')

    def test_runtime_uses_bundle_and_measures_content_not_pre_scaled_bitmaps(self):
        source = rust_source(APP / 'src/ribbon/mod.rs')
        faces = rust_source(APP / 'src/ribbon/skin.rs')
        artwork = rust_source(APP / 'src/ribbon/art.rs')
        for code in (source, faces):
            self.assertNotRegex(code, r'\.set_bitmap\(')
            self.assertNotRegex(code, r'\.set_bitmap_bundle\(')
        self.assertIn('bitmap_button::set_bundle(', faces)
        self.assertIn('button.get_text_extent(actual_label)', source)
        self.assertIn('button.get_best_size()', source)
        self.assertIn('art::raster(self.icon, art_width, art_height, state.disabled)', faces)
        self.assertIn('BitmapBundle::from_bitmaps(&normal)', artwork)
        percentages = re.search(r'const SCALE_PERCENT:.*?=\s*\[([^]]+)\]', artwork).group(1)
        self.assertEqual(tuple(map(int, percentages.split(','))), SCALES)
        self.assertIn('pub const INSET_DIP: u32 = 2;', artwork)

    def test_generators_have_the_same_mockup_asset_and_button_contract(self):
        embedded = rust_source(APP / 'src/ribbon/embedded.rs')
        generated = rust_source(APP / 'src/ribbon/generated.rs')
        self.assertEqual(embedded.count('=> Some('), len(ASSETS))
        self.assertEqual(generated.count('ButtonSpec {'), len(UI['buttons']))


if __name__ == '__main__':
    unittest.main(verbosity=2)
