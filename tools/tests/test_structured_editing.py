from pathlib import Path
import re
import unittest
from support import rust_source

ROOT = Path(__file__).resolve().parents[2]
EDITOR = rust_source(ROOT/'crates/editor/src/formatting.rs')
EDITOR_LIB = rust_source(ROOT/'crates/editor/src/lib.rs')
HEADER_FOOTER = rust_source(ROOT/'crates/editor/src/header_footer.rs')
LAYOUT = rust_source(ROOT/'crates/layout/src/lib.rs')
CORE = rust_source(ROOT/'crates/core/src/document.rs')
APP = rust_source(ROOT/'crates/gui/src/main.rs')
RIBBON = rust_source(ROOT/'crates/gui/src/ribbon/mod.rs')
GEN = rust_source(ROOT/'crates/gui/src/ribbon/generated.rs')

class StructuredEditing(unittest.TestCase):
    def test_table_cell_navigation_is_live(self):
        for name in ['table_cell_context', 'move_table_cell', 'insert_table_row', 'delete_table_row',
                     'insert_table_column', 'delete_table_column', 'merge_table_cell_right',
                     'merge_table_cell_down', 'split_table_cell']:
            self.assertIn(f'pub fn {name}', EDITOR)
        self.assertIn('state.editor.move_table_cell(!shift)', APP)

    def test_nested_paragraphs_use_normal_editor_path(self):
        self.assertIn('find_paragraph_in_blocks', EDITOR_LIB)
        self.assertIn('replace_paragraph_in_blocks', EDITOR_LIB)
        self.assertIn('insert_paragraph_after_in_section', EDITOR_LIB)
        self.assertIn('Block::Table(table)', EDITOR_LIB)

    def test_headers_and_footers_are_editable_streams(self):
        self.assertIn('pub fn enter_header', HEADER_FOOTER)
        self.assertIn('pub fn enter_footer', HEADER_FOOTER)
        self.assertIn('pub fn exit_header_footer', HEADER_FOOTER)
        self.assertIn('pub fn remove_header_footer', HEADER_FOOTER)
        self.assertIn('WXK_ESCAPE if state.editor.is_editing_header_footer()', APP)
        self.assertIn('Action::HeaderFooter(header)', RIBBON)

    def test_lists_promote_and_demote_with_indent(self):
        self.assertIn('pub fn adjust_list_level', EDITOR)
        self.assertIn('e.adjust_indent(direction.signum() * step)', RIBBON)

    def test_section_breaks_are_semantic_and_undoable(self):
        self.assertIn('pub fn insert_section_break', EDITOR)
        self.assertIn('Mutation::SectionInserted', EDITOR_LIB)
        self.assertIn('Action::SectionBreak', RIBBON)
        self.assertIn('section_break_button', GEN)

    def test_paragraph_page_rules_are_exposed(self):
        for name in ['set_keep_with_next','set_keep_lines_together','set_page_break_before','set_widow_control']:
            self.assertIn(f'pub fn {name}', EDITOR)
        for field in ['keep_with_next','keep_lines_together','page_break_before','widow_control']:
            self.assertIn(field, LAYOUT)

    def test_section_pagination_has_continuous_and_parity_paths(self):
        self.assertIn('SectionStart::Continuous', LAYOUT)
        self.assertIn('SectionStart::OddPage', LAYOUT)
        self.assertIn('SectionStart::EvenPage', LAYOUT)
        self.assertIn('compatible_page', LAYOUT)
        self.assertIn('strip_page_background', LAYOUT)

    def test_table_rows_support_splitting(self):
        self.assertIn('allow_break_across_pages', CORE)
        self.assertIn('split_prepared_table_row', LAYOUT)

    def test_column_balancing_is_live(self):
        self.assertIn('balanced_column_bottom', LAYOUT)


    def test_nested_stream_selection_is_isolated(self):
        self.assertIn('paragraph_stream_for_node', EDITOR_LIB)

    def test_vertical_row_spans_have_editor_and_layout_paths(self):
        self.assertIn('pub fn merge_table_cell_down', EDITOR)
        self.assertIn('span_group_end', LAYOUT)
        self.assertIn('row_span', LAYOUT)

    def test_keep_with_next_follows_chains(self):
        self.assertIn('minimum_keep_chain_height', LAYOUT)
        self.assertIn('Follow the complete keep chain', LAYOUT)

if __name__ == '__main__':
    unittest.main(verbosity=2)

