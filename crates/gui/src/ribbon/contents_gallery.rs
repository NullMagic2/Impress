//! Render the built-in contents designs with the same paragraphs used by the editor.
use super::*;
use model::{Block, Document, HeaderFooter, Inline, Paragraph};
use render::{Affine2D, VectorPage};
use std::sync::atomic::AtomicBool;
use wxdragon::{
    bitmap_bundle::BitmapBundle, sizers::FlexGridSizer,
    widgets::button::ButtonBitmapPosition, Bitmap,
};

const PAPER_WIDTH: u32 = 170;
const PAPER_HEIGHT: u32 = 110;
const TEXT_WIDTH_MM: f32 = 68.0;

fn sample_editor() -> EditorState {
    let mut doc = Document::default();
    let section = &mut doc.sections[0];
    section.properties.page_width_mm = TEXT_WIDTH_MM
        + section.properties.margin_left_mm + section.properties.margin_right_mm;
    section.blocks = [("The landscape", 1), ("Rivers and valleys", 2), ("Field notes", 1)]
        .into_iter().map(|(label, level)| {
            let mut paragraph = Paragraph::plain(&text(label));
            paragraph.properties.outline_level = Some(level);
            Block::Paragraph(paragraph)
        }).collect();
    let first = model::document_paragraphs(&doc)[0].id.clone();
    let mut editor = EditorState::new(doc);
    editor.set_caret(first, 0);
    editor
}

fn sample_fragment(editor: &mut EditorState, template: ContentsTemplate) -> Option<render::VectorFragment> {
    let mut options = ContentsOptions { title: text("Contents"), ..Default::default() };
    options.apply_template(template);
    editor.update_contents_options(options).ok()?;
    let mut stream = HeaderFooter::default();
    let paragraphs = model::document_paragraphs(&editor.document);
    for (index, id) in editor.document.review.contents_paragraphs.iter().enumerate() {
        let mut paragraph = (*paragraphs.iter().find(|p| &p.id == id)?).clone();
        if let Some(Inline::Hyperlink(link)) = paragraph.content.first_mut() {
            if let Some(Inline::Field(field)) = link.content.last_mut() {
                field.display_text = Some([1, 3, 17][index - 1].to_string());
            }
        }
        stream.blocks.push(Block::Paragraph(paragraph));
    }
    layout::LayoutEngine::default()
        .layout_header_footer_preview(&stream, TEXT_WIDTH_MM, &editor.document).ok()
}

fn thumbnail(fragment: &render::VectorFragment) -> Option<BitmapBundle> {
    let mut page = VectorPage::new(fragment.size);
    page.commands = fragment.commands.clone();
    let zoom = ((PAPER_WIDTH - 16) as f32 / page.size.width)
        .min((PAPER_HEIGHT - 16) as f32 / page.size.height.max(1.0));
    let left = (PAPER_WIDTH as f32 - page.size.width * zoom) * 0.5;
    let matrix = Affine2D::scale(zoom, zoom).then(Affine2D::translation(left, 8.0));
    let mut renderer = output::raster::BandRenderer::default();
    let mut bitmaps = Vec::new();
    for scale in [1, 2] {
        let pixels = scale as f32;
        let width = PAPER_WIDTH * scale;
        let height = PAPER_HEIGHT * scale;
        let mut rgba = renderer.render(&page, width, height, matrix.then(Affine2D::scale(pixels, pixels)), &AtomicBool::new(false)).ok()?;
        for pixel in rgba.chunks_exact_mut(4) { pixel.swap(0, 2); }
        bitmaps.push(Bitmap::from_rgba(&rgba, width, height)?);
    }
    Some(BitmapBundle::from_bitmaps(&bitmaps))
}

pub(super) fn choose(ribbon: &Ribbon, options: &ContentsOptions, insert: bool) -> Option<ContentsTemplate> {
    let dialog = DialogBuilder::new(&ribbon.ui.main_frame, &text("Contents gallery")).build();
    let gap = dialog.from_dip_int(8);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let columns = if ribbon.ui.main_frame.get_size().width >= dialog.from_dip_int(800) { 4 } else { 3 };
    let grid = FlexGridSizer::builder(0, columns).with_gap(Size::new(gap, gap)).build();
    let choice = Rc::new(Cell::new(None));
    let mut editor = sample_editor();
    let mut initial = None;
    for (template, name) in TEMPLATES {
        let button = Button::builder(&dialog).with_label(&text(name)).build();
        if let Some(bitmap) = sample_fragment(&mut editor, template).as_ref().and_then(thumbnail) {
            button.set_bitmap_bundle(&bitmap, ButtonBitmapPosition::Top);
        }
        let choice = Rc::clone(&choice);
        button.on_click(move |_| {
            choice.set(Some(template));
            dialog.end_modal(ID_OK);
        });
        if template == options.template && !insert { initial = Some(button); }
        grid.add(&button, 0, SizerFlag::Expand, 0);
    }
    root.add_sizer(&grid, 0, SizerFlag::Expand | SizerFlag::All, gap * 2);
    let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label(&text("Cancel")).build();
    cancel.on_click(move |_| dialog.end_modal(ID_CANCEL));
    root.add(&cancel, 0, SizerFlag::AlignCenterHorizontal | SizerFlag::Bottom, gap * 2);
    dialog.set_escape_id(ID_CANCEL);
    dialog.set_sizer_and_fit(root, true);
    if let Some(button) = initial { button.set_focus(); }
    crate::placement::centre(&dialog);
    dialog.show_modal();
    dialog.destroy();
    choice.get()
}
