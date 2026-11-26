from pathlib import Path
import unittest
from support import rust_source
ROOT=Path(__file__).resolve().parents[2]

def text(rel): return rust_source(ROOT/rel) if rel.endswith('.rs') else (ROOT/rel).read_text(encoding='utf-8')

class Structures(unittest.TestCase):
    def test_table_layout_is_not_placeholder(self):
        s=text('crates/layout/src/lib.rs')
        self.assertIn('fn prepare_table(',s); self.assertIn('fn paginate_table(',s); self.assertIn('fn render_table_row(',s)
        self.assertNotIn('table.rows.len().max(1) as f32) * 24.0',s)
    def test_columns_are_real_flow(self):
        s=text('crates/layout/src/lib.rs')
        self.assertIn('struct SectionGeometry',s); self.assertIn('fn advance_column(',s); self.assertIn('PreparedBlock::ColumnBreak',s)
    def test_lists_have_semantic_and_visual_paths(self):
        e=text('crates/editor/src/formatting.rs'); l=text('crates/layout/src/lib.rs')
        self.assertIn('pub fn toggle_list',e); self.assertIn('fn compute_list_markers',l); self.assertIn('fn format_list_number',l)
    def test_headers_footers_and_fields_render(self):
        l=text('crates/layout/src/lib.rs')
        self.assertIn('fn render_header_footer_for_page',l); self.assertIn('FieldKind::PageNumber => format_page_number(context.page_number',l)
    def test_images_keep_original_and_render_preview(self):
        e=text('crates/editor/src/formatting.rs'); l=text('crates/layout/src/lib.rs')
        self.assertRegex(e, r'Resource \{[^}]*id: resource_id\.clone\(\), media_type, original_name, data: data\.into\(\)')
        self.assertIn('fn raster_preview_commands',l); self.assertIn('decode_raster_cached',l)
        self.assertIn('DrawCommand::Image',l)
        self.assertNotIn('let max_axis = 96u32',l)
        renderer=text('crates/render/src/raster.rs')
        self.assertIn('reader.limits(limits)',renderer)
        self.assertIn('RasterImage::from_rgba(width, height',renderer)
    def test_ribbon_commands_are_enabled(self):
        g=text('crates/gui/src/ribbon/generated.rs')
        for action in ['Action::Bullets','Action::Numbering','Action::InsertTable','Action::InsertImage','Action::PageNumber','Action::Columns','Action::ColumnBreak']:
            self.assertIn(action,g)
    def test_image_caret_uses_utf8_object_marker_width(self):
        e=text('crates/editor/src/formatting.rs')
        self.assertIn('pos.byte_offset + INLINE_OBJECT_MARKER.len()',e)

if __name__=='__main__': unittest.main(verbosity=2)
