//! Multicore paginated layout from the semantic document model to vector pages.
//!
//! Expensive block preparation (OpenType shaping, mixed-run line breaking and
//! Typst math compilation) is parallel. Pagination itself remains ordered and
//! deterministic. File-format adapters never participate in layout.

mod scheduler;
mod display_recovery;
mod preparation_cache;
mod incremental;
pub use incremental::LayoutWork;
pub use preparation_cache::cache_totals;
mod object_scene;
mod references;
mod floating;
mod line_fitting;
mod table_pagination;
mod field_wrapping;
pub use scheduler::*;

use model::{
    Block, Border, BorderSet, BorderStyle, CellVerticalAlignment, CharacterProperties, Document,
    FieldKind, HeaderFooter, ImageAlignment, Inline, ListDefinition, ListFormat,
    NodeId, Paragraph, ParagraphAlignment, Resource, ResourceId, Section, StyleSheet, Table, TableAlignment, TableCell,
};
use render::{
    Affine2D, CaretStop, Color, DrawCommand, GlyphVector, Paint,
    Rect, Size, ObjectHitRegion, ObjectKind, TableCellHit, TextHitRegion, TextSource, VectorDocument, VectorFragment, VectorPage, VectorTextRun,
};
use typography::{
    bidi_levels, compose, direction_for_level, hyphenation_points, visual_order,
    FontCatalog, LineItem, MicrotypeOptions, ShapeError, ShapeOptions, ShapedRun, TextDirection,
};
use typst_engine::{TypesetError, TypstTypesetter};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Arc,atomic::{AtomicU64, Ordering}},
};
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

/// Save a copy of the resolved labels for readers of the exported document.
/// The scene and document must represent the same revision.
pub fn cache_page_field_labels(document: &mut Document, scene: &VectorDocument) {
    references::update_fields(document, Some(scene));
}

const MM_TO_PT: f32 = 72.0 / 25.4;
const DEFAULT_FONT_FAMILY: &str = "Linux Libertine G";
const DEFAULT_FONT_SIZE_PT: f32 = 11.0;
const DEFAULT_LINE_HEIGHT_EM: f32 = 1.2;
const DEFAULT_SPACE_STRETCH: f32 = 0.50;
const DEFAULT_SPACE_SHRINK: f32 = 0.33;
const INLINE_OBJECT_MARKER_LEN: usize = '\u{FFFC}'.len_utf8();
/// Space kept between a tall header or footer and the body it pushes away.
const HEADER_FOOTER_BODY_GAP_PT: f32 = 6.0;
/// Headers and footers never squeeze the body below this height.
const MIN_BODY_HEIGHT_PT: f32 = 72.0;
/// Headers never start closer than this to the top edge of the paper.
const MIN_HEADER_TOP_PT: f32 = 2.0;
/// Most columns a section can have; more are merged into the last one.
const MAX_COLUMNS: usize = 16;
/// Width of the "Line between" rule separating columns.
const COLUMN_SEPARATOR_PT: f32 = 0.5;


#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutOptions {
    pub show_page_background: bool,
    pub worker_threads: usize,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        let available = std::thread::available_parallelism().map(usize::from).unwrap_or(2);
        Self {
            show_page_background: true,
            // Reserve a hardware thread for wxDragon input and painting.
            worker_threads: available.saturating_sub(1).clamp(1,8),
        }
    }
}

#[derive(Debug)]
pub struct LayoutEngine {
    pub options: LayoutOptions,
}

impl Default for LayoutEngine {
    fn default() -> Self { Self { options: LayoutOptions::default() } }
}

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("font family not found: {0}")]
    FontNotFound(String),
    #[error("font data unavailable: {0}")]
    FontDataUnavailable(String),
    #[error("text shaping failed: {0}")]
    Shape(#[from] ShapeError),
    #[error("font outline extraction failed: {0}")]
    Outline(String),
    #[error("equation typesetting failed: {0}")]
    Typeset(#[from] TypesetError),
    #[error("layout worker pool failed: {0}")]
    WorkerPool(String),
    #[error("render preparation failed: {0}")]
    RenderPreparation(String),
    #[error("layout cancelled because a newer document revision exists")]
    Cancelled,
}

impl LayoutEngine {
    /// Lay out a template stream with the same shaping and decoration as a page.
    pub fn layout_header_footer_preview(&self, stream: &HeaderFooter, width_mm: f32, document: &Document) -> Result<VectorFragment, LayoutError> {
        const PREVIEW_PAGE_NUMBER: usize = 1;
        const PREVIEW_PAGE_COUNT: usize = 5;
        let width = width_mm * MM_TO_PT;
        let context = FieldContext { page_number: PREVIEW_PAGE_NUMBER, page_count: PREVIEW_PAGE_COUNT,
            section_number: 1, page_number_format: Default::default() };
        let fragment = prepare_header_footer_fragment(stream, width, &document.styles, &document.resources, &context)?;
        Ok(VectorFragment { size: Size::new(width, fragment.height), baseline_pt: 0.0, commands: fragment.commands })
    }

    pub fn layout_document(&self, document: &Document) -> Result<VectorDocument, LayoutError> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.options.worker_threads)
            .thread_name(|i| format!("layout-{i}"))
            .build()
            .map_err(|e| LayoutError::WorkerPool(e.to_string()))?;
        pool.install(|| self.layout_document_in_pool(document))
    }

    pub(crate) fn layout_document_in_pool(&self, document: &Document) -> Result<VectorDocument, LayoutError> {
        self.layout_document_in_pool_impl(document, None, &mut incremental::LayoutCache::default())
    }

    pub(crate) fn layout_document_cached(&self, document:&Document, expected_revision:u64, generation:&AtomicU64, cache:&mut incremental::LayoutCache)->Result<VectorDocument,LayoutError> {
        cache.begin_request();
        self.layout_document_in_pool_impl(document,Some((generation,expected_revision)),cache)
    }

    fn layout_document_in_pool_impl(&self, document:&Document, cancel:Option<(&AtomicU64,u64)>, cache:&mut incremental::LayoutCache)->Result<VectorDocument,LayoutError> {
        if !references::has_dynamic_fields(document) { cache.clear_fields(); return self.layout_document_pass(document,cancel,cache,false); }
        let inputs=incremental::LayoutCache::field_inputs(document,self.options.show_page_background);
        let mut draft=document.clone();references::append_endnotes(&mut draft,&mut cache.derived_notes)?;references::update_fields(&mut draft,None);
        cache.seed_fields(&mut draft,inputs.as_ref());
        for _ in 0..8 {
            check_cancel(cancel)?;
            let pages=self.layout_document_pass(&draft,cancel,cache,false)?;
            if !references::update_fields(&mut draft,Some(&pages)) { cache.remember_fields(&draft,inputs); return Ok(pages); }
        }
        Err(LayoutError::RenderPreparation("Page-reference fields did not settle within eight layout passes".into()))
    }

    fn layout_document_pass(
        &self,
        document: &Document,
        cancel: Option<(&AtomicU64, u64)>,
        cache: &mut incremental::LayoutCache,
        recover_display: bool,
    ) -> Result<VectorDocument, LayoutError> {
        cache.work.field_passes += 1;
        let mut pages = Vec::new();
        // Continuous sections may share their first body page with the previous
        // section; in that case the previous section retains ownership of the
        // shared page's header/footer and this section starts H/F rendering on
        // its next physical page.
        let mut section_ranges = Vec::new();
        let mut next_display_number = 1usize;
        let mut current_page_format = model::PageNumberFormat::Decimal;
        let mut previous_flow: Option<(SectionGeometry, FlowCursor)> = None;
        for (section_index, section) in document.sections.iter().enumerate() {
            check_cancel(cancel)?;
            let properties = &section.properties;
            let margin_rect = Rect::new(
                properties.margin_left_mm * MM_TO_PT,
                properties.margin_top_mm * MM_TO_PT,
                properties.text_width_mm() * MM_TO_PT,
                (properties.page_height_mm - properties.margin_top_mm - properties.margin_bottom_mm) * MM_TO_PT,
            );

            // Word-style odd/even section starts may require an intentionally
            // blank page before the section. Continuous sections currently
            // retain a page boundary when page geometry changes; the body
            // within every section nevertheless uses a real multi-column flow.
            let next_page_number = pages.len() + 1;
            let needs_blank = match section.properties.start_on {
                model::SectionStart::OddPage => next_page_number % 2 == 0,
                model::SectionStart::EvenPage => next_page_number % 2 == 1,
                _ => false,
            };
            if needs_blank {
                let width = section.properties.page_width_mm * MM_TO_PT;
                let height = section.properties.page_height_mm * MM_TO_PT;
                let mut blank = new_page(width, height, self.options.show_page_background);
                blank.margin_rect = Some(margin_rect);
                blank.page_number_label = Some(format_page_number(next_display_number, current_page_format));
                pages.push(blank);
                next_display_number = next_display_number.saturating_add(1);
            }

            let extents = match HeaderFooterExtents::measure(section, document) {
                Ok(extents) => extents,
                Err(_) if recover_display => HeaderFooterExtents::default(),
                Err(error) => return Err(error),
            };
            let geometry = SectionGeometry::with_extents(section, extents);
            let has_notes = references::section_has_notes(section);
            let next_column = previous_flow.and_then(|(_, cursor)| {
                (matches!(section.properties.start_on, model::SectionStart::NewColumn)
                    && cursor.column_index + 1 < geometry.column_count).then_some(cursor.column_index + 1)
            });
            let can_share = !has_notes && (section_index==0 || !references::section_has_notes(&document.sections[section_index-1]))
                && (matches!(section.properties.start_on, model::SectionStart::Continuous) || next_column.is_some())
                && !pages.is_empty()
                && previous_flow.is_some_and(|(previous, _)| previous.compatible_page(&geometry));
            let initial = if !can_share { None } else if let Some(column) = next_column {
                Some((column, geometry.content_top))
            } else {
                previous_flow.map(|(previous, cursor)| {
                    // A continuous section after columns starts at the left,
                    // below them; one-column flow simply continues.
                    let column = if previous.column_count == 1 && geometry.column_count == 1 { cursor.column_index } else { 0 };
                    (column.min(geometry.column_count.saturating_sub(1)), cursor.y.max(geometry.content_top))
                })
            };

            let global_page_start = if can_share { pages.len() - 1 } else { pages.len() };
            let page_format = section.properties.page_number_format.unwrap_or(current_page_format);
            let display_start = section.properties.page_number_start.map(|value|value as usize)
                .unwrap_or(if can_share {next_display_number.saturating_sub(1)} else {next_display_number});
            let (mut section_pages, final_cursor) = match cache.layout_section(
                self, section, document, section_index, global_page_start, initial, extents, cancel,
            ) {
                Ok(result) => result,
                Err(LayoutError::Cancelled) => return Err(LayoutError::Cancelled),
                Err(_) if recover_display => display_recovery::section(
                    document, section, &geometry, initial, self.options.show_page_background,
                    &FieldContext { page_number: display_start, page_count: display_start,
                        section_number: section_index + 1, page_number_format: page_format }, cancel,
                )?,
                Err(error) => return Err(error),
            };
            for (index, page) in section_pages.iter_mut().enumerate() {
                page.margin_rect = Some(margin_rect);
                page.page_number_label = Some(format_page_number(display_start.saturating_add(index), page_format));
            }
            next_display_number = display_start.saturating_add(section_pages.len());
            current_page_format = page_format;

            if can_share {
                let mut first = section_pages.remove(0);
                strip_page_background(&mut first, &geometry);
                let shared = pages.last_mut().expect("continuous section requires a previous page");
                shared.commands.extend(first.commands);
                shared.hit_regions.extend(first.hit_regions);
                shared.object_regions.extend(first.object_regions);
                let new_pages = section_pages.len();
                pages.extend(section_pages);
                // The shared page keeps the previous section header/footer.
                if new_pages > 0 {
                    section_ranges.push(SectionPageRange {
                        section_index, first_page: global_page_start + 1, page_count: new_pages,
                        local_page_offset: 1, display_start, page_format, extents,
                    });
                }
            } else {
                let page_count = section_pages.len();
                pages.extend(section_pages);
                section_ranges.push(SectionPageRange {
                    section_index, first_page: global_page_start, page_count,
                    local_page_offset: 0, display_start, page_format, extents,
                });
            }
            previous_flow = Some((geometry, final_cursor));
        }

        // Header/footer fields are resolved only after every section has paginated.
        // This makes NUMPAGES the true document-wide count rather than a count
        // truncated at the current section.
        let total_pages = pages.len();
        for range in section_ranges {
            let section = &document.sections[range.section_index];
            let geometry = SectionGeometry::with_extents(section, range.extents);
            for rendered_page in 0..range.page_count {
                let local_page = range.local_page_offset + rendered_page;
                let context = FieldContext {
                    page_number: range.display_start + local_page,
                    page_count: total_pages,
                    section_number: range.section_index + 1,
                    page_number_format: range.page_format,
                };
                let result = render_header_footer_for_page(
                    &mut pages[range.first_page + rendered_page], section, document, local_page, &geometry, &context,
                );
                if !recover_display { result?; }
            }
        }
        object_scene::annotate_inline_objects(document, &mut pages);
        let text_order = model::document_paragraphs(document).iter().enumerate().map(|(i, p)| (p.id.0.to_string(), i)).collect();
        Ok(VectorDocument { pages, text_order: Arc::new(text_order) })
    }

    fn layout_section(
        &self,
        section: &Section,
        document: &Document,
        section_index: usize,
        global_page_start: usize,
        initial_flow: Option<(usize, f32)>,
        extents: HeaderFooterExtents,
        cancel: Option<(&AtomicU64, u64)>,
    ) -> Result<(Vec<VectorPage>, FlowCursor), LayoutError> {
        let geometry = SectionGeometry::with_extents(section, extents);
        let list_markers = compute_list_markers(section, &document.lists, &document.styles);
        let field_context = FieldContext {
            page_number: global_page_start + 1,
            page_count: global_page_start + 1,
            section_number: section_index + 1,
            page_number_format: section.properties.page_number_format.unwrap_or_default(),
        };

        // Anchored pictures are placed separately; everything else flows.
        let flow_blocks: Vec<&Block> = section.blocks.iter()
            .filter(|b|!matches!(b,Block::Image(i)if i.anchor.is_some()&&i.wrap!=model::ImageWrap::Inline))
            .collect();
        let prepared_results: Vec<Result<Arc<PreparedBlock>, LayoutError>> = flow_blocks
            .par_iter()
            .map(|&block| {
                check_cancel(cancel)?;
                let marker = block_node_id(block).and_then(|id| list_markers.get(id));
                preparation_cache::prepare(block, geometry.column_width, &document.styles,
                    &document.resources, marker, &field_context)
            })
            .collect();
        let prepared: Vec<Arc<PreparedBlock>> = prepared_results.into_iter().collect::<Result<_, _>>()?;
        // Columns of different widths: a paragraph that starts or continues in
        // a column narrower or wider than the first is re-fitted to it.
        let refit = |index: usize, offset: usize, width: f32| -> Result<Vec<PreparedLine>, LayoutError> {
            let Some(Block::Paragraph(paragraph)) = flow_blocks.get(index).copied() else {
                return Err(LayoutError::RenderPreparation("Only paragraphs are re-fitted to a column".into()));
            };
            let mut rest = paragraph.clone();
            let marker = if offset == 0 { list_markers.get(&paragraph.id) } else {
                let (_, after) = model::split_content(&paragraph.content, offset).map_err(LayoutError::RenderPreparation)?;
                rest.content = after.into();
                // The continuation is not the paragraph's first line.
                rest.properties.first_line_indent_mm = Some(0.0);
                rest.properties.space_before_pt = Some(0.0);
                None
            };
            let block = Block::Paragraph(rest);
            match &*preparation_cache::prepare(&block, width, &document.styles, &document.resources, marker, &field_context)? {
                PreparedBlock::Paragraph { lines, .. } => Ok(lines.clone()),
                _ => Err(LayoutError::RenderPreparation("A paragraph prepared as another block".into())),
            }
        };
        let refit: Option<&Refit> = if geometry.equal_columns() { None } else { Some(&refit) };

        let notes=references::PreparedNotes::new(section,document,&geometry,&field_context)?;
        let mut reserves=Vec::<f32>::new();
        for _ in 0..16 {
            let mut note_geometry=geometry;note_geometry.note_reserves=&reserves;
            let (mut pages,mut cursor)=if floating::has_floats(section){
                floating::paginate(section,document,&prepared,&note_geometry,initial_flow,self.options.show_page_background,&field_context,cancel)?
            }else{paginate_prepared_with(&prepared,&note_geometry,initial_flow,self.options.show_page_background,cancel,refit)?};
            let assignment=notes.assign(&pages)?;
            let plan=notes.plan(&assignment,&geometry)?;
            let required=&plan.reserves;
            let stable=required.iter().enumerate().all(|(i,h)|*h <= reserves.get(i).copied().unwrap_or(0.0)+0.05);
            if stable {
                let old_len=pages.len();while pages.len()<plan.pages.len(){pages.push(new_page(geometry.page_width,geometry.page_height,self.options.show_page_background));}
                if pages.len()!=old_len{cursor.page_index=pages.len()-1;cursor.column_index=geometry.column_count-1;cursor.y=geometry.content_bottom;}
                // Before the footnotes, so the rule ends at the body text.
                draw_column_separators(&mut pages, &geometry, initial_flow.map(|(_, y)| y));
                notes.paint(&mut pages,&plan,&geometry)?;
                return Ok((pages,cursor));
            }
            reserves.resize(required.len().max(reserves.len()),0.0);
            for (i,height) in required.iter().copied().enumerate(){reserves[i]=reserves[i].max(height);}
        }
        Err(LayoutError::RenderPreparation("Footnotes did not settle within sixteen pagination passes".into()))
    }
}

/// Word's "Line between": a rule in the middle of each gap between columns,
/// from the top of the section's text on the page down to its lowest line.
fn draw_column_separators(pages: &mut [VectorPage], geometry: &SectionGeometry, first_page_top: Option<f32>) {
    if !geometry.column_separator { return; }
    for (index, page) in pages.iter_mut().enumerate() {
        let top = if index == 0 { first_page_top.unwrap_or(geometry.content_top) } else { geometry.content_top };
        let bottom = page.hit_regions.iter().map(|region| region.rect.max_y())
            .filter(|y| *y > top).fold(top, f32::max).min(geometry.content_bottom);
        if bottom <= top { continue; }
        for column in 1..geometry.column_count {
            let previous_right = geometry.column_left(column - 1) + geometry.width_of(column - 1);
            let x = (previous_right + geometry.column_left(column)) / 2.0 - COLUMN_SEPARATOR_PT / 2.0;
            page.commands.push(DrawCommand::FillRect {
                rect: Rect::new(x, top, COLUMN_SEPARATOR_PT, bottom - top), paint: Paint::Solid(Color::BLACK),
            });
        }
    }
}

/// Re-prepares a paragraph's lines for a column of a different width, given
/// the block's index, the byte offset of the first line still to be placed
/// (0 for the whole paragraph) and the new width. Used only by sections with
/// columns of unequal width.
type Refit<'r> = dyn Fn(usize, usize, f32) -> Result<Vec<PreparedLine>, LayoutError> + 'r;

#[cfg(test)]
fn paginate_prepared<T:AsRef<PreparedBlock>>(prepared:&[T],geometry:&SectionGeometry,initial_flow:Option<(usize,f32)>,show_page_background:bool,cancel:Option<(&AtomicU64,u64)>)->Result<(Vec<VectorPage>,FlowCursor),LayoutError> {
    paginate_prepared_with(prepared, geometry, initial_flow, show_page_background, cancel, None)
}

fn paginate_prepared_with<T:AsRef<PreparedBlock>>(prepared:&[T],geometry:&SectionGeometry,initial_flow:Option<(usize,f32)>,show_page_background:bool,cancel:Option<(&AtomicU64,u64)>,refit:Option<&Refit>)->Result<(Vec<VectorPage>,FlowCursor),LayoutError> {
    let (pages, cursor) = flow_prepared(prepared, geometry, initial_flow, show_page_background, cancel, refit)?;
    let (pages, mut cursor) = match balance_last_page(prepared, geometry, initial_flow, &pages, &cursor, show_page_background, cancel, refit)? {
        Some(balanced) => balanced,
        None => (pages, cursor),
    };
    // What follows a multi-column section starts below its lowest column:
    // the last column's cursor may sit above the first column's text.
    if geometry.column_count > 1 {
        let top = if cursor.page_index == 0 { initial_flow.map_or(geometry.content_top, |(_, y)| y) } else { geometry.content_top };
        if let Some(page) = pages.get(cursor.page_index) {
            let lowest = page.hit_regions.iter().map(|r| r.rect.max_y()).chain(page.object_regions.iter().map(|r| r.rect.max_y()))
                .filter(|y| *y > top).fold(cursor.y, f32::max);
            cursor.y = lowest.min(geometry.content_bottom);
        }
    }
    Ok((pages, cursor))
}

/// Word balances the columns of a section's last page, so text set in
/// columns between full-width text ends level instead of filling the first
/// column to the bottom. The section is laid out again with the last page's
/// bottom raised to an even share of its text, and that is kept only if
/// nothing moves to another page. Explicit breaks are author intent and
/// footnotes have their own reserves, so those sections are left as they are.
#[allow(clippy::too_many_arguments)]
fn balance_last_page<T:AsRef<PreparedBlock>>(prepared:&[T],geometry:&SectionGeometry,initial_flow:Option<(usize,f32)>,pages:&[VectorPage],cursor:&FlowCursor,
    show_page_background:bool,cancel:Option<(&AtomicU64,u64)>,refit:Option<&Refit>)->Result<Option<(Vec<VectorPage>,FlowCursor)>,LayoutError> {
    let count = geometry.column_count;
    if count <= 1 || !geometry.note_reserves.is_empty() || !geometry.equal_columns() { return Ok(None); }
    if prepared.iter().any(|b| matches!(b.as_ref(), PreparedBlock::PageBreak | PreparedBlock::ColumnBreak)) { return Ok(None); }
    let last = cursor.page_index;
    // A one-page section starting at the top is balanced in flow_prepared.
    if last == 0 && initial_flow.is_none() { return Ok(None); }
    let top = if last == 0 { initial_flow.map_or(geometry.content_top, |(_, y)| y.max(geometry.content_top)) } else { geometry.content_top };
    let column = (geometry.content_bottom - top).max(1.0);
    let used = cursor.column_index as f32 * column + (cursor.y - top).max(0.0);
    if used <= 0.0 { return Ok(None); }
    // Already level: the last column is (nearly) as full as the others.
    if cursor.column_index + 1 == count && cursor.y >= geometry.content_bottom - 24.0 { return Ok(None); }
    let mut reserves = vec![0.0f32; last + 1];
    let share = used / count as f32;
    // Lines do not split, so allow a little more than an exact share.
    for slack in [6.0f32, 14.0, 24.0, 36.0] {
        let bottom = (top + share + slack).min(geometry.content_bottom);
        reserves[last] = geometry.content_bottom - bottom;
        if reserves[last] < 1.0 { return Ok(None); }
        let mut lowered = *geometry;
        lowered.note_reserves = &reserves;
        let (balanced, end) = flow_prepared(prepared, &lowered, initial_flow, show_page_background, cancel, refit)?;
        if balanced.len() == pages.len() && end.page_index == last { return Ok(Some((balanced, end))); }
    }
    Ok(None)
}

fn flow_prepared<T:AsRef<PreparedBlock>>(prepared:&[T],geometry:&SectionGeometry,initial_flow:Option<(usize,f32)>,show_page_background:bool,cancel:Option<(&AtomicU64,u64)>,refit:Option<&Refit>)->Result<(Vec<VectorPage>,FlowCursor),LayoutError> {
    let geometry=*geometry;
        let mut flow_geometry = geometry;
        // Balancing measures heights at the first column's width, so it only
        // applies when every column has that width.
        if initial_flow.is_none() && geometry.note_reserves.is_empty() && geometry.equal_columns() {
            if let Some(bottom) = balanced_column_bottom(&prepared, &geometry) {
                flow_geometry.content_bottom = bottom;
            }
        }

        let mut pages = vec![new_page(
            geometry.page_width,
            geometry.page_height,
            show_page_background,
        )];
        let (initial_column, initial_y) = initial_flow.unwrap_or((0, geometry.content_top));
        let mut cursor = FlowCursor {
            page_index: 0,
            column_index: initial_column.min(flow_geometry.column_count.saturating_sub(1)),
            y: initial_y.clamp(flow_geometry.content_top, flow_geometry.content_bottom),
        };

        for block_index in 0..prepared.len() {
            check_cancel(cancel)?;
            paginate_block_with(prepared, block_index, &mut pages, &mut cursor, &flow_geometry, show_page_background, refit)?;
        }

    Ok((pages,cursor))
}

/// One deterministic block placement. Shared by cold layout and checkpoint resume;
/// both paths must use exactly the same keep/widow/table behavior.
fn paginate_block<T:AsRef<PreparedBlock>>(prepared:&[T],block_index:usize,pages:&mut Vec<VectorPage>,
    cursor:&mut FlowCursor,flow_geometry:&SectionGeometry,show_page_background:bool)->Result<(),LayoutError> {
    paginate_block_with(prepared, block_index, pages, cursor, flow_geometry, show_page_background, None)
}

fn paginate_block_with<T:AsRef<PreparedBlock>>(prepared:&[T],block_index:usize,pages:&mut Vec<VectorPage>,
    cursor:&mut FlowCursor,flow_geometry:&SectionGeometry,show_page_background:bool,refit:Option<&Refit>)->Result<(),LayoutError> {
            let block = prepared[block_index].as_ref();

            // Keep-with-next is a placement constraint, not a shaping concern.
            // Follow the complete keep chain so Heading 1 -> Heading 2 -> body
            // cannot be separated merely because only the first pair was
            // considered. The last block contributes only its minimum visible
            // portion; every paragraph carrying keep-with-next contributes its
            // complete height.
            if matches!(block, PreparedBlock::Paragraph { keep_with_next: true, .. }) {
                if let Some(chain_height) = minimum_keep_chain_height(&prepared, block_index) {
                    let remaining = (flow_geometry.bottom(cursor.page_index) - cursor.y).max(0.0);
                    if chain_height <= flow_geometry.height_at(cursor.page_index)
                        && chain_height > remaining
                        && cursor.y > flow_geometry.content_top
                    {
                        advance_column(pages, cursor, flow_geometry, show_page_background);
                    }
                }
            }

            match block {
                PreparedBlock::Paragraph {
                    lines, space_before, space_after, left_indent, marker, decoration,
                    keep_lines_together, page_break_before, widow_control, ..
                } => {
                    if *page_break_before && (cursor.page_index > 0 || cursor.column_index > 0 || cursor.y > flow_geometry.content_top) {
                        new_flow_page(pages, cursor, flow_geometry, show_page_background);
                    }
                    paginate_paragraph_with(
                        lines, *space_before, *space_after, *left_indent, marker.as_ref(), decoration.as_ref(),
                        *keep_lines_together, *widow_control,
                        pages, cursor, flow_geometry, show_page_background,
                        refit.map(|refit| ColumnRefit { refit, block_index, prepared_width: flow_geometry.column_width }),
                    )?;
                }
                PreparedBlock::Equation { id, fragment, before, after } => {
                    cursor.y += *before;
                    ensure_vertical_space(pages, cursor, flow_geometry, fragment.size.height, show_page_background);
                    let x = flow_geometry.column_left(cursor.column_index)
                        + ((flow_geometry.column_width - fragment.size.width) * 0.5).max(0.0);
                    let current = &mut pages[cursor.page_index];
                    current.commands.extend(fragment.commands.iter().map(|command| command.translated(x, cursor.y)));
                    current.object_regions.push(ObjectHitRegion { node_id: id.clone(), rect: Rect::new(x, cursor.y, fragment.size.width, fragment.size.height), kind: ObjectKind::Equation { source: None } });
                    cursor.y += fragment.size.height + *after;
                }
                PreparedBlock::Table(table) => {
                    paginate_table(
                        table,
                        pages,
                        cursor,
                        flow_geometry,
                        show_page_background,
                    )?;
                }
                PreparedBlock::Image(image) => {
                    ensure_vertical_space(pages, cursor, flow_geometry, image.height, show_page_background);
                    let col_left = flow_geometry.column_left(cursor.column_index);
                    let x = match image.alignment {
                        ImageAlignment::Left => col_left,
                        ImageAlignment::Center => col_left + ((flow_geometry.column_width - image.width) * 0.5).max(0.0),
                        ImageAlignment::Right => col_left + (flow_geometry.column_width - image.width).max(0.0),
                    };
                    let current = &mut pages[cursor.page_index];
                    current.commands.extend(image.commands.iter().map(|c| c.translated(x, cursor.y)));
                    current.object_regions.push(ObjectHitRegion { node_id: image.id.clone(), rect: Rect::new(x, cursor.y, image.width, image.height), kind: ObjectKind::Picture { source: None } });
                    cursor.y += image.height + 4.0;
                }
                PreparedBlock::PageBreak => {
                    new_flow_page(pages, cursor, flow_geometry, show_page_background);
                }
                PreparedBlock::ColumnBreak => {
                    advance_column(pages, cursor, flow_geometry, show_page_background);
                }
                PreparedBlock::Foreign { height } => {
                    ensure_vertical_space(pages, cursor, flow_geometry, *height, show_page_background);
                    cursor.y += *height;
                }
            }

    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct FieldContext {
    page_number: usize,
    page_count: usize,
    section_number: usize,
    page_number_format: model::PageNumberFormat,
}

#[derive(Clone, Copy, Debug)]
struct SectionGeometry<'a> {
    note_reserves: &'a [f32],
    page_width: f32,
    page_height: f32,
    content_left: f32,
    content_top: f32,
    content_bottom: f32,
    column_count: usize,
    /// Width of the first column; every column's when they are equal.
    column_width: f32,
    /// Left edge of each column relative to `content_left`, and its width.
    /// Only the first `column_count` entries are used.
    column_lefts: [f32; MAX_COLUMNS],
    column_widths: [f32; MAX_COLUMNS],
    /// Draw a rule between columns (Word's "Line between").
    column_separator: bool,
}

/// Pages of one section that receive its header and footer streams.
struct SectionPageRange {
    section_index: usize,
    first_page: usize,
    page_count: usize,
    /// 1 when the section's first body page is shared with the previous
    /// section, which keeps that page's header and footer.
    local_page_offset: usize,
    display_start: usize,
    page_format: model::PageNumberFormat,
    extents: HeaderFooterExtents,
}

/// Heights of a section's tallest header and tallest footer stream, in points.
/// A tall header pushes the body text down, and a tall footer pushes the
/// body's bottom edge up, so they never overlap it.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
struct HeaderFooterExtents {
    header: f32,
    footer: f32,
}

impl HeaderFooterExtents {
    fn measure(section: &Section, document: &Document) -> Result<Self, LayoutError> {
        let width = header_footer_width(section);
        // Field results (page numbers, counts) only change the width of a line,
        // not its height, so a representative context is enough here.
        let context = FieldContext {
            page_number: 1,
            page_count: 1,
            section_number: 1,
            page_number_format: section.properties.page_number_format.unwrap_or_default(),
        };
        let mut extents = Self::default();
        for header_footer in section.header_footers() {
            let fragment = prepare_header_footer_fragment(
                header_footer.stream, width, &document.styles, &document.resources, &context,
            )?;
            let tallest = if header_footer.header { &mut extents.header } else { &mut extents.footer };
            *tallest = tallest.max(fragment.height);
        }
        Ok(extents)
    }
}

impl<'a> SectionGeometry<'a> {
    /// Geometry from the section margins alone, ignoring header/footer height.
    #[cfg(test)]
    fn new(section: &Section) -> Self {
        Self::with_extents(section, HeaderFooterExtents::default())
    }

    /// Geometry whose body area makes room for the measured header and footer.
    fn with_extents(section: &Section, extents: HeaderFooterExtents) -> Self {
        let page_width = section.properties.page_width_mm * MM_TO_PT;
        let page_height = section.properties.page_height_mm * MM_TO_PT;
        let content_left = section.properties.margin_left_mm * MM_TO_PT;
        let content_right = page_width - section.properties.margin_right_mm * MM_TO_PT;
        let (content_top, content_bottom) = body_band(section, page_height, extents);
        let mut bands = section.properties.column_bands();
        if bands.len() > MAX_COLUMNS {
            // Merge the surplus columns into the last one that fits.
            let last = bands[bands.len() - 1];
            bands.truncate(MAX_COLUMNS);
            let kept = bands[MAX_COLUMNS - 1];
            bands[MAX_COLUMNS - 1].width_mm = last.left_mm + last.width_mm - kept.left_mm;
        }
        let mut column_lefts = [0.0; MAX_COLUMNS];
        let mut column_widths = [0.0; MAX_COLUMNS];
        let text_width = content_right - content_left;
        for (index, band) in bands.iter().enumerate() {
            column_lefts[index] = (band.left_mm * MM_TO_PT).min(text_width);
            column_widths[index] = (band.width_mm * MM_TO_PT).max(1.0);
        }
        Self {
            note_reserves: &[], page_width, page_height, content_left, content_top, content_bottom,
            column_count: bands.len().max(1), column_width: column_widths[0], column_lefts, column_widths,
            column_separator: section.properties.column_separator && bands.len() > 1,
        }
    }

    /// One full-width column; for tests that build geometry by hand.
    #[cfg(test)]
    fn single_column(page_width: f32, page_height: f32, content_left: f32, content_top: f32, content_bottom: f32, width: f32) -> Self {
        let mut column_widths = [0.0; MAX_COLUMNS];
        column_widths[0] = width;
        Self { note_reserves: &[], page_width, page_height, content_left, content_top, content_bottom,
            column_count: 1, column_width: width, column_lefts: [0.0; MAX_COLUMNS], column_widths, column_separator: false }
    }

    fn column_index(&self, index: usize) -> usize { index.min(self.column_count.saturating_sub(1)) }

    fn column_left(&self, index: usize) -> f32 {
        self.content_left + self.column_lefts[self.column_index(index)]
    }

    fn width_of(&self, index: usize) -> f32 { self.column_widths[self.column_index(index)] }

    /// Whether every column has the first column's width.
    fn equal_columns(&self) -> bool {
        self.column_widths[..self.column_count].iter().all(|width| (width - self.column_width).abs() < 0.05)
    }

    /// Width from the first column's left edge to the last column's right edge.
    fn text_width(&self) -> f32 {
        let last = self.column_count - 1;
        self.column_lefts[last] + self.column_widths[last]
    }

    fn bottom(&self,page:usize)->f32{self.content_bottom-self.note_reserves.get(page).copied().unwrap_or(0.0)}
    fn height_at(&self,page:usize)->f32{(self.bottom(page)-self.content_top).max(1.0)}

    fn column_height(&self) -> f32 { (self.content_bottom - self.content_top).max(1.0) }

    fn compatible_page(&self, other: &Self) -> bool {
        (self.page_width - other.page_width).abs() < 0.05
            && (self.page_height - other.page_height).abs() < 0.05
    }
}

fn strip_page_background(page: &mut VectorPage, geometry: &SectionGeometry) {
    let remove = page.commands.first().is_some_and(|command| match command {
        DrawCommand::FillRect { rect, paint: Paint::Solid(color) } => {
            rect.x.abs() < 0.01 && rect.y.abs() < 0.01
                && (rect.width - geometry.page_width).abs() < 0.05
                && (rect.height - geometry.page_height).abs() < 0.05
                && color.r > 0.99 && color.g > 0.99 && color.b > 0.99
        }
        _ => false,
    });
    if remove { page.commands.remove(0); }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FlowCursor {
    page_index: usize,
    column_index: usize,
    y: f32,
}

fn new_flow_page(
    pages: &mut Vec<VectorPage>,
    cursor: &mut FlowCursor,
    geometry: &SectionGeometry,
    show_page_background: bool,
) {
    pages.push(new_page(geometry.page_width, geometry.page_height, show_page_background));
    cursor.page_index = pages.len() - 1;
    cursor.column_index = 0;
    cursor.y = geometry.content_top;
}

fn advance_column(
    pages: &mut Vec<VectorPage>,
    cursor: &mut FlowCursor,
    geometry: &SectionGeometry,
    show_page_background: bool,
) {
    if cursor.column_index + 1 < geometry.column_count {
        cursor.column_index += 1;
        cursor.y = geometry.content_top;
    } else {
        new_flow_page(pages, cursor, geometry, show_page_background);
    }
}

fn ensure_vertical_space(
    pages: &mut Vec<VectorPage>,
    cursor: &mut FlowCursor,
    geometry: &SectionGeometry,
    required: f32,
    show_page_background: bool,
) {
    if cursor.y > geometry.content_top && cursor.y + required > geometry.bottom(cursor.page_index) {
        advance_column(pages, cursor, geometry, show_page_background);
    }
}

fn minimum_keep_chain_height<T:AsRef<PreparedBlock>>(blocks: &[T], start: usize) -> Option<f32> {
    let mut index = start;
    let mut total = 0.0f32;
    loop {
        let block = blocks.get(index)?.as_ref();
        let PreparedBlock::Paragraph { keep_with_next, .. } = block else {
            total += minimum_block_height(block);
            return total.is_finite().then_some(total);
        };
        if *keep_with_next {
            total += full_block_height(block)?;
            index += 1;
            if index >= blocks.len() { return None; }
        } else {
            total += minimum_block_height(block);
            return total.is_finite().then_some(total);
        }
    }
}

fn minimum_block_height(block: &PreparedBlock) -> f32 {
    match block {
        PreparedBlock::Paragraph { lines, space_before, space_after, keep_lines_together, .. } => {
            let line_height = if *keep_lines_together {
                lines.iter().map(|line| line.height).sum::<f32>()
            } else {
                lines.first().map(|line| line.height).unwrap_or(0.0)
            };
            *space_before + line_height + *space_after
        }
        PreparedBlock::Equation { id: _, fragment, before, after } => *before + fragment.size.height + *after,
        PreparedBlock::Table(table) => table.rows.first().map(|row| row.height).unwrap_or(0.0),
        PreparedBlock::Image(image) => image.height + 4.0,
        PreparedBlock::PageBreak | PreparedBlock::ColumnBreak => f32::INFINITY,
        PreparedBlock::Foreign { height } => *height,
    }
}

fn full_block_height(block: &PreparedBlock) -> Option<f32> {
    match block {
        PreparedBlock::Paragraph { lines, space_before, space_after, .. } => {
            Some(*space_before + lines.iter().map(|line| line.height).sum::<f32>() + *space_after)
        }
        PreparedBlock::Equation { id: _, fragment, before, after } => Some(*before + fragment.size.height + *after),
        PreparedBlock::Table(table) => Some(table.rows.iter().map(|row| row.height).sum::<f32>() + 4.0),
        PreparedBlock::Image(image) => Some(image.height + 4.0),
        PreparedBlock::Foreign { height } => Some(*height),
        PreparedBlock::PageBreak | PreparedBlock::ColumnBreak => None,
    }
}

/// Balance the final page of an ordinary multi-column section by lowering the
/// temporary flow bottom when the complete section can fit on one physical
/// page. Explicit page/column breaks opt out because they are author intent.
fn balanced_column_bottom<T:AsRef<PreparedBlock>>(blocks: &[T], geometry: &SectionGeometry) -> Option<f32> {
    if geometry.column_count <= 1 { return None; }
    let mut total = 0.0f32;
    for block in blocks { total += full_block_height(block.as_ref())?; }
    let capacity = geometry.column_height() * geometry.column_count as f32;
    if total <= 0.0 || total > capacity { return None; }
    let target = (total / geometry.column_count as f32).ceil() + 6.0;
    Some((geometry.content_top + target).min(geometry.content_bottom))
}

#[allow(clippy::too_many_arguments)]
fn paginate_paragraph(
    lines: &[PreparedLine],
    space_before: f32,
    space_after: f32,
    left_indent: f32,
    marker: Option<&PreparedMarker>,
    decoration: Option<&ParagraphDecoration>,
    keep_lines_together: bool,
    widow_control: bool,
    pages: &mut Vec<VectorPage>,
    cursor: &mut FlowCursor,
    geometry: &SectionGeometry,
    show_page_background: bool,
) -> Result<(),LayoutError> {
    paginate_paragraph_with(lines, space_before, space_after, left_indent, marker, decoration, keep_lines_together,
        widow_control, pages, cursor, geometry, show_page_background, None)
}

/// How a paragraph in a section of unequal columns gets its lines re-fitted.
#[derive(Clone, Copy)]
struct ColumnRefit<'r> {
    refit: &'r Refit<'r>,
    block_index: usize,
    /// Width the paragraph's lines were first prepared for.
    prepared_width: f32,
}

#[allow(clippy::too_many_arguments)]
fn paginate_paragraph_with(
    lines: &[PreparedLine],
    space_before: f32,
    space_after: f32,
    left_indent: f32,
    marker: Option<&PreparedMarker>,
    decoration: Option<&ParagraphDecoration>,
    keep_lines_together: bool,
    widow_control: bool,
    pages: &mut Vec<VectorPage>,
    cursor: &mut FlowCursor,
    geometry: &SectionGeometry,
    show_page_background: bool,
    column_refit: Option<ColumnRefit>,
) -> Result<(),LayoutError> {
    let mut refitted: Option<Vec<PreparedLine>> = None;
    let mut fitted_width = column_refit.map_or(geometry.column_width, |refit| refit.prepared_width);
    let total_lines_height = lines.iter().map(|line| line.height).sum::<f32>();
    let total_height = space_before + total_lines_height + space_after;
    let remaining = (geometry.bottom(cursor.page_index) - cursor.y).max(0.0);
    if keep_lines_together && !lines.iter().any(|line|line.pagination_break.is_some())
        && total_height <= geometry.height_at(cursor.page_index)
        && total_height > remaining
        && cursor.y > geometry.content_top
    {
        advance_column(pages, cursor, geometry, show_page_background);
    }

    cursor.y += space_before;
    let mut index = 0usize;
    let mut placed_lines = 0usize;
    let mut first_visual_line = true;
    loop {
        let current = refitted.as_deref().unwrap_or(lines);
        if let Some(kind) = current.get(index).and_then(|line|line.pagination_break) {
            match kind {
                model::PaginationBreakKind::Page => new_flow_page(pages, cursor, geometry, show_page_background),
                model::PaginationBreakKind::Column => advance_column(pages, cursor, geometry, show_page_background),
            }
            index += 1;
            continue;
        }
        // Unequal columns: re-fit the remaining text to this column's width.
        if let Some(refit) = column_refit {
            let column_width = geometry.width_of(cursor.column_index);
            let current = refitted.as_deref().unwrap_or(lines);
            if index < current.len() && (column_width - fitted_width).abs() > 0.05 {
                let offset = if index == 0 && refitted.is_none() { Some(0) } else { line_start_offset(&current[index]) };
                if let Some(offset) = offset {
                    let mut fresh = (refit.refit)(refit.block_index, offset, column_width)?;
                    shift_line_sources(&mut fresh, offset, placed_lines);
                    refitted = Some(fresh);
                    index = 0;
                    fitted_width = column_width;
                }
            }
        }
        let lines = refitted.as_deref().unwrap_or(lines);
        if index >= lines.len() { break; }
        let decoration_width_change = fitted_width - column_refit.map_or(fitted_width, |refit| refit.prepared_width);
        let remaining_height = (geometry.bottom(cursor.page_index) - cursor.y).max(0.0);
        let mut fit = 0usize;
        let mut used = 0.0f32;
        for line in &lines[index..] {
            if line.pagination_break.is_some() { break; }
            // EMU and millimetre conversions can disagree by a fraction of a point.
            if used + line.height > remaining_height + 0.001 { break; }
            used += line.height;
            fit += 1;
        }

        if fit == 0 {
            if cursor.y<=geometry.content_top+0.01 && lines[index].height>geometry.height_at(cursor.page_index) {
                return Err(LayoutError::RenderPreparation("A line or inline object is taller than the available page body".into()));
            }
            advance_column(pages, cursor, geometry, show_page_background);
            continue;
        }

        let left = lines.len() - index;
        if widow_control && fit < left && lines[index..index+fit+1].iter().all(|line|line.pagination_break.is_none()) {
            // Require two lines on both sides of an automatic break whenever
            // that is geometrically possible. A three-line paragraph cannot
            // be split 2+1 without an orphan, so move it intact to a fresh
            // column if the remaining paragraph fits there.
            if fit < 2 && cursor.y > geometry.content_top {
                advance_column(pages, cursor, geometry, show_page_background);
                continue;
            }
            let trailing = left - fit;
            if trailing < 2 {
                let move_to_next = 2 - trailing;
                if fit.saturating_sub(move_to_next) >= 2 {
                    fit -= move_to_next;
                } else {
                    let remaining_paragraph_height = lines[index..].iter().map(|line| line.height).sum::<f32>();
                    if cursor.y > geometry.content_top && remaining_paragraph_height <= geometry.height_at(cursor.page_index) {
                        advance_column(pages, cursor, geometry, show_page_background);
                        continue;
                    }
                }
            }
        }

        if let Some(decoration) = decoration {
            let decoration_width = decoration.width + decoration_width_change;
            let height = lines[index..index + fit].iter().map(|line| line.height).sum::<f32>();
            let x = geometry.column_left(cursor.column_index) + left_indent;
            let y = cursor.y;
            let page = &mut pages[cursor.page_index];
            append_paragraph_decoration(&mut page.commands, decoration,
                Rect::new(x, y, decoration_width, height), index == 0, index + fit == lines.len());
        }
        for line in &lines[index..index + fit] {
            let baseline_y = cursor.y + line.ascent;
            let dx = geometry.column_left(cursor.column_index) + left_indent;
            let current = &mut pages[cursor.page_index];
            if first_visual_line {
                if let Some(marker) = marker {
                    let marker_x = (dx - marker.width - marker.gap).max(geometry.column_left(cursor.column_index));
                    current.commands.extend(marker.commands.iter().map(|c| c.translated(marker_x, baseline_y)));
                }
                first_visual_line = false;
            }
            let clip=Rect::new(geometry.column_left(cursor.column_index),geometry.content_top,
                geometry.width_of(cursor.column_index),geometry.height_at(cursor.page_index));
            current.commands.extend(line.commands.iter().map(|c| c.translated(dx, baseline_y).clipped(clip)));
            current.hit_regions.extend(line.hit_regions.iter().filter_map(|r| r.translated(dx, baseline_y).clipped(clip)));
            cursor.y += line.height;
        }
        index += fit;
        placed_lines += fit;
        if index < lines.len() && lines[index].pagination_break.is_none() { advance_column(pages, cursor, geometry, show_page_background); }
    }
    cursor.y += space_after;
    Ok(())
}

/// Byte offset in its paragraph of a line's first character.
fn line_start_offset(line: &PreparedLine) -> Option<usize> {
    line.hit_regions.iter().map(|region| region.byte_range.start).min()
}

/// Lines prepared from a paragraph's text after `offset` report offsets from
/// 0 and visual lines from 0; move both to the whole paragraph's numbering
/// so the caret, selection and search map back to the right characters.
fn shift_line_sources(lines: &mut [PreparedLine], offset: usize, visual_offset: usize) {
    if offset == 0 { return; }
    for line in lines {
        for region in &mut line.hit_regions {
            region.byte_range = region.byte_range.start + offset..region.byte_range.end + offset;
            region.visual_line += visual_offset;
            for stop in &mut region.caret_stops { stop.byte_offset += offset; }
        }
        for command in &mut line.commands { shift_command_source(command, offset); }
    }
}

fn shift_command_source(command: &mut DrawCommand, offset: usize) {
    match command {
        DrawCommand::Clipped { command, .. } => shift_command_source(Arc::make_mut(command), offset),
        DrawCommand::TextRun(run) => {
            if let Some(source) = &mut Arc::make_mut(run).source {
                source.byte_range = source.byte_range.start + offset..source.byte_range.end + offset;
            }
        }
        _ => {}
    }
}

fn block_node_id(block: &Block) -> Option<&NodeId> {
    match block {
        Block::Paragraph(p) => Some(&p.id),
        Block::Equation(e) => Some(&e.id),
        Block::Table(t) => Some(&t.id),
        Block::Image(i) => Some(&i.id),
        Block::PageBreak { id } | Block::ColumnBreak { id } => Some(id),
        Block::Foreign(f) => Some(&f.id),
    }
}

fn compute_list_markers(section: &Section, lists: &BTreeMap<NodeId, ListDefinition>, styles: &StyleSheet) -> BTreeMap<NodeId, String> {
    let mut result = BTreeMap::new();
    let mut counters: HashMap<NodeId, Vec<i32>> = HashMap::new();
    let mut style_counters: HashMap<model::StyleId, i32> = HashMap::new();
    let mut previous_numbered_style: Option<model::StyleId> = None;
    for block in &section.blocks {
        let Block::Paragraph(paragraph) = block else { continue; };
        let properties = styles.resolve_paragraph_with_direct(paragraph.style.as_ref(), &paragraph.properties);
        let style_id = paragraph.style.as_ref().cloned().unwrap_or_else(|| model::StyleId::from(model::STYLE_NORMAL));
        let options = styles.resolve_formatting(Some(&style_id));
        if options.get("par_suppress_numbers").is_some_and(|value| value == "true") {
            previous_numbered_style = None;
            continue;
        }
        let Some(list_id) = properties.list_id.as_ref() else {
            // The Numbering Format dialog configures a paragraph style. Its
            // marker is calculated for every paragraph using that style, even
            // when the paragraph has no direct list definition.
            let bullet = options.get("num_bullets").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            let number = options.get("num_numbers").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            if bullet > 0 {
                const BULLETS: [&str; 8] = ["", "●", "○", "■", "◆", "➢", "✓", "–"];
                let marker = if bullet == BULLETS.len() { options.get("num_custom_bullet").map(String::as_str).unwrap_or("★") }
                    else { BULLETS.get(bullet).copied().unwrap_or("") };
                if !marker.is_empty() { result.insert(paragraph.id.clone(), marker.to_owned()); }
                previous_numbered_style = None;
            } else if number > 0 {
                let start = options.get("num_start").and_then(|v| v.parse::<i32>().ok()).unwrap_or(1).max(1);
                if options.get("num_restart").is_some_and(|v| v == "true") && previous_numbered_style.as_ref() != Some(&style_id) {
                    style_counters.remove(&style_id);
                }
                let counter = style_counters.entry(style_id).or_insert(start - 1);
                *counter = counter.saturating_add(1);
                let (format, suffix) = match number {
                    1 => (ListFormat::Decimal, "."), 2 => (ListFormat::Decimal, ")"),
                    3 => (ListFormat::UpperRoman, "."), 4 => (ListFormat::UpperLetter, "."),
                    5 => (ListFormat::LowerLetter, ")"), 6 => (ListFormat::LowerLetter, "."),
                    7 => (ListFormat::LowerRoman, "."), _ => (ListFormat::Decimal, "."),
                };
                let value = format_list_number(*counter, format);
                let marker = if number == 8 { options.get("num_custom_number").map(|template|template.replace("%1", &value)).unwrap_or_else(||format!("{value}.")) }
                    else { format!("{value}{suffix}") };
                result.insert(paragraph.id.clone(), marker);
                previous_numbered_style = paragraph.style.as_ref().cloned().or_else(|| Some(model::StyleId::from(model::STYLE_NORMAL)));
            } else {
                previous_numbered_style = None;
            }
            continue;
        };
        previous_numbered_style = None;
        let Some(definition) = lists.get(list_id) else { continue; };
        let level_index = usize::from(properties.list_level.unwrap_or(0));
        if level_index >= 9 { continue; } // Untrusted native/DOCX metadata must not index past counters.
        let Some(level) = definition.levels.iter().find(|l| usize::from(l.level) == level_index)
            .or_else(|| definition.levels.get(level_index)) else { continue; };
        let list_counters = counters.entry(list_id.clone()).or_insert_with(|| vec![0; 9]);
        if list_counters[level_index] == 0 { list_counters[level_index] = level.start_at; }
        else { list_counters[level_index] = list_counters[level_index].saturating_add(1); }
        for deeper in list_counters.iter_mut().skip(level_index + 1) { *deeper = 0; }

        let marker = match level.format {
            ListFormat::Bullet => if level.text.trim().is_empty() { "•".to_owned() } else { level.text.clone() },
            _ => {
                let mut text = level.text.clone();
                if text.is_empty() { text = format!("%{}.", level_index + 1); }
                for (idx, value) in list_counters.iter().enumerate().take(level_index + 1) {
                    let formatted = format_list_number(*value, definition.levels.get(idx).map(|l| l.format).unwrap_or(ListFormat::Decimal));
                    text = text.replace(&format!("%{}", idx + 1), &formatted);
                }
                text
            }
        };
        result.insert(paragraph.id.clone(), marker);
    }
    result
}

fn format_list_number(value: i32, format: ListFormat) -> String {
    match format {
        ListFormat::Bullet => "•".into(),
        ListFormat::Decimal => value.to_string(),
        ListFormat::LowerLetter => alphabetic_number(value, false),
        ListFormat::UpperLetter => alphabetic_number(value, true),
        ListFormat::LowerRoman => roman_number(value, false),
        ListFormat::UpperRoman => roman_number(value, true),
    }
}

fn format_page_number(value: usize, format: model::PageNumberFormat) -> String {
    let Ok(value) = i32::try_from(value) else { return value.to_string(); };
    let format = match format {
        model::PageNumberFormat::Decimal => ListFormat::Decimal,
        model::PageNumberFormat::UpperRoman => ListFormat::UpperRoman,
        model::PageNumberFormat::LowerRoman => ListFormat::LowerRoman,
        model::PageNumberFormat::UpperLetter => ListFormat::UpperLetter,
        model::PageNumberFormat::LowerLetter => ListFormat::LowerLetter,
    };
    format_list_number(value, format)
}

fn alphabetic_number(value: i32, upper: bool) -> String {
    if value <= 0 { return value.to_string(); }
    let mut n = value as u32;
    let mut chars = Vec::new();
    while n > 0 {
        n -= 1;
        let base = if upper { b'A' } else { b'a' };
        chars.push((base + (n % 26) as u8) as char);
        n /= 26;
    }
    chars.into_iter().rev().collect()
}

fn roman_number(value: i32, upper: bool) -> String {
    if !(1..=3999).contains(&value) { return value.to_string(); }
    let mut n = value;
    let mut out = String::new();
    for (amount, numeral) in [
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
        (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ] {
        while n >= amount { out.push_str(numeral); n -= amount; }
    }
    if upper { out } else { out.to_ascii_lowercase() }
}

fn paginate_table(
    table: &PreparedTable,
    pages: &mut Vec<VectorPage>,
    cursor: &mut FlowCursor,
    geometry: &SectionGeometry,
    show_page_background: bool,
) -> Result<(),LayoutError> {
    let table_left = |cursor: &FlowCursor| geometry.column_left(cursor.column_index) + table.x_offset;
    let mut pending: VecDeque<(usize, PreparedTableRow)> = table.rows.iter().cloned().enumerate().collect();
    let mut active_span_group_end: Option<usize> = None;

    while let Some((row_index, row)) = pending.pop_front() {
        if pages.len()>10_000 {return Err(LayoutError::RenderPreparation("Table pagination exceeds 10,000 pages".into()));}
        let header_height=table.rows.iter().take(table.repeating_header_rows).map(|r|r.height).sum::<f32>();
        if table.repeating_header_rows>0 && row_index>=table.repeating_header_rows && header_height+18.0>geometry.height_at(cursor.page_index) {
            return Err(LayoutError::RenderPreparation("Repeated table headers leave no room for table content; reduce the header or disable repetition".into()));
        }
        if active_span_group_end.is_some_and(|end| row_index > end) { active_span_group_end = None; }

        if row.span_group_end > row_index {
            let end = row.span_group_end.min(table.rows.len().saturating_sub(1));
            let group_height = table.rows[row_index..=end].iter().map(|candidate| candidate.height).sum::<f32>();
            if group_height>geometry.height_at(cursor.page_index)+0.01 {
                return Err(LayoutError::RenderPreparation("A vertically merged table group is taller than the page; split the merge or reduce its content".into()));
            }
            let remaining = (geometry.bottom(cursor.page_index) - cursor.y).max(0.0);
            if cursor.y > geometry.content_top && group_height > remaining && group_height <= geometry.height_at(cursor.page_index) {
                advance_column(pages, cursor, geometry, show_page_background);
                if table.repeating_header_rows > 0 && row_index >= table.repeating_header_rows {
                    render_repeating_table_headers(table, pages, cursor, geometry, show_page_background);
                }
            }
            if cursor.y+group_height>geometry.bottom(cursor.page_index)+0.01 {
                return Err(LayoutError::RenderPreparation("A merged table group cannot fit below the repeated headers".into()));
            }
            active_span_group_end = Some(end);
        }

        let in_vertical_span_group = active_span_group_end.is_some_and(|end| row_index <= end);
        let mut available = (geometry.bottom(cursor.page_index) - cursor.y).max(0.0);
        if !in_vertical_span_group && row.height>available+0.01 {
            let mut split=if row.allow_break_across_pages {split_prepared_table_row(&row,available)} else {None};
            if split.is_none() && cursor.y>geometry.content_top+0.01 {
                advance_column(pages,cursor,geometry,show_page_background);
                if table.repeating_header_rows>0 && row_index>=table.repeating_header_rows {
                    render_repeating_table_headers(table,pages,cursor,geometry,show_page_background);
                }
                available=(geometry.bottom(cursor.page_index)-cursor.y).max(0.0);
                if row.allow_break_across_pages && row.height>available+0.01 {split=split_prepared_table_row(&row,available);}
            }
            if let Some((mut top,mut bottom))=split {
                top.span_group_end=row_index;bottom.span_group_end=row_index;
                render_table_row(&mut pages[cursor.page_index],&top,table_left(cursor),cursor.y);
                cursor.y+=top.height;
                advance_column(pages,cursor,geometry,show_page_background);
                if table.repeating_header_rows>0 && row_index>=table.repeating_header_rows {
                    render_repeating_table_headers(table,pages,cursor,geometry,show_page_background);
                }
                pending.push_front((row_index,bottom));continue;
            }
            if row.height>available+0.01 {
                return Err(LayoutError::RenderPreparation("A table row cannot fit or split on the page; allow row breaks, reduce its content, or split nested/indivisible objects".into()));
            }
        }

        render_table_row(&mut pages[cursor.page_index], &row, table_left(cursor), cursor.y);
        cursor.y += row.height;
    }
    cursor.y += 4.0;
    Ok(())
}

fn render_repeating_table_headers(
    table: &PreparedTable,
    pages: &mut Vec<VectorPage>,
    cursor: &mut FlowCursor,
    geometry: &SectionGeometry,
    show_page_background: bool,
) {
    for header in table.rows.iter().take(table.repeating_header_rows) {
        if cursor.y + header.height > geometry.bottom(cursor.page_index) {
            advance_column(pages, cursor, geometry, show_page_background);
        }
        let x = geometry.column_left(cursor.column_index) + table.x_offset;
        render_table_row(&mut pages[cursor.page_index], header, x, cursor.y);
        cursor.y += header.height;
    }
}

/// Room kept between a cell's text and its border: the cell padding, also on
/// both sides of a page break inside a row, as Word keeps it.
const TABLE_CELL_PADDING: f32 = 4.0;

/// Splits a row at a line boundary for a page break. As in Word, each part is
/// a closed row: the first part keeps the cells' bottom padding and borders on
/// its page, the rest starts with their top padding and borders on the next.
fn split_prepared_table_row(row: &PreparedTableRow, available: f32) -> Option<(PreparedTableRow, PreparedTableRow)> {
    if available < 18.0 { return None; }
    let mut candidates = Vec::new();
    for cell in &row.cells {
        for hit in &cell.hit_regions {
            // Adjacent line hit boxes meet at their baseline spacing. Adding
            // padding here would cut into the next line and make a long
            // single-paragraph cell impossible to split.
            let boundary = hit.rect.y + hit.rect.height;
            if boundary >= 12.0 && boundary + TABLE_CELL_PADDING <= available { candidates.push(boundary); }
        }
    }
    candidates.sort_by(|a, b| a.total_cmp(b));
    candidates.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    let Some(boundary) = candidates.into_iter().rev().find(|boundary| {
        row.cells.iter().all(|cell| cell_can_split_at(cell, *boundary))
    }) else { return table_pagination::split_unaligned_row(row, available); };
    if row.height - boundary < 8.0 { return None; }

    let mut top_cells = Vec::with_capacity(row.cells.len());
    let mut bottom_cells = Vec::with_capacity(row.cells.len());
    for cell in &row.cells {
        let (top, bottom) = split_prepared_cell(cell, boundary)?;
        top_cells.push(top);
        bottom_cells.push(bottom);
    }
    Some((
        PreparedTableRow { height: boundary + TABLE_CELL_PADDING, cells: top_cells, allow_break_across_pages: row.allow_break_across_pages, span_group_end: 0 },
        PreparedTableRow { height: row.height - boundary + TABLE_CELL_PADDING, cells: bottom_cells, allow_break_across_pages: row.allow_break_across_pages, span_group_end: 0 },
    ))
}

fn cell_can_split_at(cell: &PreparedTableCell, boundary: f32) -> bool {
    cell.object_regions.is_empty() && cell.commands.iter().all(|command| match command_vertical_bounds(command) {
        Some((top, bottom)) => bottom <= boundary + 0.1 || top >= boundary - 0.1,
        None => false,
    }) && cell.hit_regions.iter().all(|hit| {
        let top = hit.rect.y;
        let bottom = hit.rect.y + hit.rect.height;
        bottom <= boundary + 0.1 || top >= boundary - 0.1
    })
}

fn split_prepared_cell(cell: &PreparedTableCell, boundary: f32) -> Option<(PreparedTableCell, PreparedTableCell)> {
    if !cell_can_split_at(cell, boundary) { return None; }
    // The rest moves up to start one padding below the next page's border.
    let shift = TABLE_CELL_PADDING - boundary;
    let mut top_commands = Vec::new();
    let mut bottom_commands = Vec::new();
    for command in &cell.commands {
        let (top, bottom) = command_vertical_bounds(command)?;
        if bottom <= boundary + 0.1 { top_commands.push(command.clone()); }
        else if top >= boundary - 0.1 { bottom_commands.push(command.translated(0.0, shift)); }
        else { return None; }
    }
    let mut top_hits = Vec::new();
    let mut bottom_hits = Vec::new();
    for hit in &cell.hit_regions {
        let top = hit.rect.y;
        let bottom = hit.rect.y + hit.rect.height;
        if bottom <= boundary + 0.1 { top_hits.push(hit.clone()); }
        else if top >= boundary - 0.1 { bottom_hits.push(hit.translated(0.0, shift)); }
        else { return None; }
    }
    let (top_height, bottom_height) = (boundary + TABLE_CELL_PADDING, (cell.height - boundary).max(0.0) + TABLE_CELL_PADDING);
    let mut top_region = cell.own_region.clone();
    let mut bottom_region = cell.own_region.clone();
    top_region.rect.height = top_height;
    bottom_region.rect.height = bottom_height;
    if let ObjectKind::TableCell(info) = &mut top_region.kind { info.can_resize_row = false; }
    if let ObjectKind::TableCell(info) = &mut bottom_region.kind { info.can_resize_row = false; }
    // Both parts keep the cell's borders, so the table is closed at the foot
    // of the page and opened again at the top of the next.
    Some((
        PreparedTableCell { own_region: top_region, object_regions: Vec::new(), x: cell.x, width: cell.width, height: top_height, content_bottom: cell.content_bottom.min(boundary), commands: top_commands, hit_regions: top_hits, shading: cell.shading, borders: cell.borders.clone() },
        PreparedTableCell { own_region: bottom_region, object_regions: Vec::new(), x: cell.x, width: cell.width, height: bottom_height, content_bottom: (cell.content_bottom - boundary).max(0.0) + TABLE_CELL_PADDING, commands: bottom_commands, hit_regions: bottom_hits, shading: cell.shading, borders: cell.borders.clone() },
    ))
}

fn command_vertical_bounds(command: &DrawCommand) -> Option<(f32, f32)> {
    match command {
        DrawCommand::Clipped {rect,command} => command_vertical_bounds(command).map(|(top,bottom)|(top.max(rect.y),bottom.min(rect.max_y()))),
        DrawCommand::FillRect { rect, .. } | DrawCommand::Image { rect, .. } => Some((rect.y, rect.y + rect.height)),
        DrawCommand::TextRun(run) => Some((run.bounds.y, run.bounds.y + run.bounds.height)),
        // Typst/vector paths do not carry a cheap page-space bounding box in
        // the scene graph. Keep those rows atomic rather than risk clipping.
        DrawCommand::Path(_) => None,
    }
}

fn render_table_row(page: &mut VectorPage, row: &PreparedTableRow, x: f32, y: f32) {
    for cell in &row.cells {
        let cell_x = x + cell.x;
        // Line fitting already reserves cell padding. Allow ordinary glyph
        // overhang/hanging punctuation in that padding, but never across the
        // cell boundary into the neighboring column.
        let clip=Rect::new(cell_x,y,cell.width,cell.height);
        if let Some(color) = cell.shading {
            page.commands.push(DrawCommand::FillRect {
                rect: Rect::new(cell_x, y, cell.width, cell.height),
                paint: Paint::Solid(color),
            });
        }
        page.commands.extend(cell.commands.iter().map(|c| c.translated(cell_x, y).clipped(clip)));
        page.hit_regions.extend(cell.hit_regions.iter().filter_map(|r| r.translated(cell_x, y).clipped(clip)));
        page.object_regions.push(cell.own_region.translated(cell_x, y));
        page.object_regions.extend(cell.object_regions.iter().filter_map(|r| {let mut r=r.translated(cell_x,y);r.rect=r.rect.intersection(clip);(!r.rect.is_empty()).then_some(r)}));
        append_cell_borders(&mut page.commands, cell_x, y, cell.width, cell.height, &cell.borders);
    }
}

/// Paint the same paragraph decoration in paginated bodies and nested streams.
fn append_paragraph_decoration(
    commands: &mut Vec<DrawCommand>,
    decoration: &ParagraphDecoration,
    rect: Rect,
    first: bool,
    last: bool,
) {
    let Rect {
        x,
        y,
        width,
        height,
    } = rect;
    if decoration.preset == 2 || decoration.preset == 3 {
        let offset = if decoration.preset == 2 { 2.5 } else { 1.5 };
        let shadow = if decoration.preset == 2 {
            Color::rgba(0.0, 0.0, 0.0, 0.24)
        } else {
            Color::rgb8(99, 112, 132)
        };
        commands.push(DrawCommand::FillRect {
            rect: Rect::new(x + width, y + offset, offset, height),
            paint: Paint::Solid(shadow),
        });
        commands.push(DrawCommand::FillRect {
            rect: Rect::new(x + offset, y + height, width, offset),
            paint: Paint::Solid(shadow),
        });
    }
    if let Some(color) = decoration.shading {
        commands.push(DrawCommand::FillRect {
            rect: Rect::new(x, y, width, height),
            paint: Paint::Solid(color),
        });
    }
    if let Some((pattern, color)) = &decoration.pattern {
        let step = if pattern.starts_with("Dk") { 3.0 } else { 6.0 };
        if pattern.contains("Horizontal") || pattern.contains("Grid") {
            let mut offset = step;
            while offset < height {
                commands.push(DrawCommand::FillRect {
                    rect: Rect::new(x, y + offset, width, 0.45),
                    paint: Paint::Solid(*color),
                });
                offset += step;
            }
        }
        if pattern.contains("Vertical") || pattern.contains("Grid") {
            let mut offset = step;
            while offset < width {
                commands.push(DrawCommand::FillRect {
                    rect: Rect::new(x + offset, y, 0.45, height),
                    paint: Paint::Solid(*color),
                });
                offset += step;
            }
        }
    }
    if let Some(borders) = &decoration.borders {
        let [top, bottom, left, right] = decoration.border_distance;
        if first {
            if let Some(border) = &borders.top {
                append_border(
                    commands,
                    Rect::new(x - left, y - top, width + left + right, 0.0),
                    border,
                    true,
                );
            }
        }
        if last {
            if let Some(border) = borders
                .bottom
                .as_ref()
                .or(borders.inside_horizontal.as_ref())
            {
                append_border(
                    commands,
                    Rect::new(x - left, y + height + bottom, width + left + right, 0.0),
                    border,
                    true,
                );
            }
        }
        if let Some(border) = &borders.left {
            append_border(
                commands,
                Rect::new(x - left, y - top, 0.0, height + top + bottom),
                border,
                false,
            );
        }
        if let Some(border) = &borders.right {
            append_border(
                commands,
                Rect::new(x + width + right, y - top, 0.0, height + top + bottom),
                border,
                false,
            );
        }
    }
}

fn append_cell_borders(commands: &mut Vec<DrawCommand>, x: f32, y: f32, width: f32, height: f32, borders: &BorderSet) {
    let default = Border { width_pt: 0.5, color: "#B7B7B7".into(), style: BorderStyle::Single };
    append_border(commands, Rect::new(x, y, width, 0.0), borders.top.as_ref().unwrap_or(&default), true);
    append_border(commands, Rect::new(x, y + height, width, 0.0), borders.bottom.as_ref().unwrap_or(&default), true);
    append_border(commands, Rect::new(x, y, 0.0, height), borders.left.as_ref().unwrap_or(&default), false);
    append_border(commands, Rect::new(x + width, y, 0.0, height), borders.right.as_ref().unwrap_or(&default), false);
}

fn append_border(commands: &mut Vec<DrawCommand>, rect: Rect, border: &Border, horizontal: bool) {
    if border.style == BorderStyle::None || border.width_pt <= 0.0 { return; }
    let color = parse_color(Some(&border.color)).unwrap_or(Color::BLACK);
    let thickness = border.width_pt.max(0.25);
    let line_rect = if horizontal {
        Rect::new(rect.x, rect.y - thickness * 0.5, rect.width, thickness)
    } else {
        Rect::new(rect.x - thickness * 0.5, rect.y, thickness, rect.height)
    };
    if matches!(border.style, BorderStyle::Dotted | BorderStyle::Dashed) {
        let length = if horizontal { rect.width } else { rect.height };
        let dash = thickness * if border.style == BorderStyle::Dotted { 1.0 } else { 4.0 };
        let gap = thickness * 2.0;
        let mut offset = 0.0;
        while offset < length {
            let segment = dash.min(length - offset);
            let part = if horizontal { Rect::new(line_rect.x + offset,line_rect.y,segment,thickness) }
                else { Rect::new(line_rect.x,line_rect.y + offset,thickness,segment) };
            commands.push(DrawCommand::FillRect { rect:part, paint:Paint::Solid(color) });
            offset += dash + gap;
        }
        return;
    }
    commands.push(DrawCommand::FillRect { rect: line_rect, paint: Paint::Solid(color) });
    if border.style == BorderStyle::Double {
        let offset = thickness * 1.8;
        let second = if horizontal {
            Rect::new(rect.x, rect.y + offset, rect.width, thickness)
        } else {
            Rect::new(rect.x + offset, rect.y, thickness, rect.height)
        };
        commands.push(DrawCommand::FillRect { rect: second, paint: Paint::Solid(color) });
    }
}

/// Top and bottom of the body band: the margins, moved inwards where a
/// header or footer is taller than the room the margin leaves for it.
fn body_band(section: &Section, page_height: f32, extents: HeaderFooterExtents) -> (f32, f32) {
    let properties = &section.properties;
    let mut top = properties.margin_top_mm * MM_TO_PT;
    let mut bottom = page_height - properties.margin_bottom_mm * MM_TO_PT;
    if extents.header > 0.0 {
        let header_bottom = header_top(section) + extents.header;
        top = top.max(header_bottom + HEADER_FOOTER_BODY_GAP_PT);
    }
    if extents.footer > 0.0 {
        let footer_top = page_height - properties.footer_distance_mm * MM_TO_PT - extents.footer;
        bottom = bottom.min(footer_top - HEADER_FOOTER_BODY_GAP_PT);
    }
    // Margins alone may already leave less than the minimum; only the growth
    // caused by headers and footers is limited here.
    let margin_height = page_height - (properties.margin_top_mm + properties.margin_bottom_mm) * MM_TO_PT;
    let minimum = MIN_BODY_HEIGHT_PT.min(margin_height.max(1.0));
    if bottom - top < minimum {
        let shortfall = minimum - (bottom - top);
        let top_growth = (top - properties.margin_top_mm * MM_TO_PT).max(0.0);
        let bottom_growth = (page_height - properties.margin_bottom_mm * MM_TO_PT - bottom).max(0.0);
        let total_growth = (top_growth + bottom_growth).max(f32::EPSILON);
        top -= shortfall * top_growth / total_growth;
        bottom += shortfall * bottom_growth / total_growth;
    }
    (top, bottom)
}

fn header_top(section: &Section) -> f32 {
    (section.properties.header_distance_mm * MM_TO_PT).max(MIN_HEADER_TOP_PT)
}

fn header_footer_width(section: &Section) -> f32 {
    let properties = &section.properties;
    (properties.page_width_mm - properties.margin_left_mm - properties.margin_right_mm) * MM_TO_PT
}

fn render_header_footer_for_page(
    page: &mut VectorPage,
    section: &Section,
    document: &Document,
    local_page: usize,
    geometry: &SectionGeometry,
    context: &FieldContext,
) -> Result<(), LayoutError> {
    let available_width = header_footer_width(section);
    let x = section.properties.margin_left_mm * MM_TO_PT;
    let variant = model::HeaderFooterVariant::for_page(&section.properties, local_page).index();
    for (header, y, height) in [(true, 0.0, geometry.content_top), (false, geometry.content_bottom, geometry.page_height - geometry.content_bottom)] {
        page.object_regions.push(ObjectHitRegion { node_id: section.id.0.clone(), rect: Rect::new(x, y, available_width, height.max(1.0)),
            kind: ObjectKind::HeaderFooter { section_id: section.id.0.clone(), header, variant } });
    }

    if let Some(header) = section.headers.for_page(&section.properties, local_page) {
        let fragment = prepare_header_footer_fragment(header, available_width, &document.styles, &document.resources, context)?;
        let y = header_top(section);
        page.commands.extend(fragment.commands.iter().map(|c| c.translated(x, y)));
        page.hit_regions.extend(fragment.hit_regions.iter().map(|r| r.translated(x, y)));
        page.object_regions.extend(fragment.object_regions.iter().map(|r| r.translated(x, y)));
    }
    if let Some(footer) = section.footers.for_page(&section.properties, local_page) {
        let fragment = prepare_header_footer_fragment(footer, available_width, &document.styles, &document.resources, context)?;
        let distance = section.properties.footer_distance_mm * MM_TO_PT;
        let y = (geometry.page_height - distance - fragment.height).max(geometry.content_bottom + 2.0);
        page.commands.extend(fragment.commands.iter().map(|c| c.translated(x, y)));
        page.hit_regions.extend(fragment.hit_regions.iter().map(|r| r.translated(x, y)));
        page.object_regions.extend(fragment.object_regions.iter().map(|r| r.translated(x, y)));
    }
    Ok(())
}

fn prepare_header_footer_fragment(
    header_footer: &HeaderFooter,
    width: f32,
    styles: &StyleSheet,
    resources: &BTreeMap<ResourceId, Resource>,
    context: &FieldContext,
) -> Result<PreparedCellFragment, LayoutError> {
    preparation_cache::with_context(resources,|ctx| prepare_block_fragment(&header_footer.blocks, width, styles, ctx, resources, context))
}

fn check_cancel(cancel: Option<(&AtomicU64, u64)>) -> Result<(), LayoutError> {
    if let Some((generation, expected)) = cancel {
        if generation.load(Ordering::Acquire) != expected { return Err(LayoutError::Cancelled); }
    }
    Ok(())
}

struct WorkerContext {
    fonts: FontCatalog,
    typst: TypstTypesetter,
}

#[derive(Clone)]
struct ParagraphDecoration { shading: Option<Color>, borders: Option<BorderSet>, width: f32, preset: usize,
    border_distance: [f32;4], pattern: Option<(String,Color)> }

#[derive(Clone)]
enum PreparedBlock {
    Paragraph {
        lines: Vec<PreparedLine>,
        space_before: f32,
        space_after: f32,
        left_indent: f32,
        marker: Option<PreparedMarker>,
        decoration: Option<ParagraphDecoration>,
        keep_with_next: bool,
        keep_lines_together: bool,
        page_break_before: bool,
        widow_control: bool,
    },
    Equation { id: String, fragment: VectorFragment, before: f32, after: f32 },
    Table(PreparedTable),
    Image(PreparedImage),
    PageBreak,
    ColumnBreak,
    Foreign { height: f32 },
}

impl AsRef<PreparedBlock> for PreparedBlock {
    fn as_ref(&self) -> &PreparedBlock { self }
}

#[derive(Clone)]
struct PreparedMarker {
    commands: Vec<DrawCommand>,
    width: f32,
    gap: f32,
}

#[derive(Clone)]
struct PreparedImage {
    id: String,
    width: f32,
    height: f32,
    alignment: ImageAlignment,
    commands: Vec<DrawCommand>,
}

#[derive(Clone)]
struct PreparedTable {
    x_offset: f32,
    rows: Vec<PreparedTableRow>,
    repeating_header_rows: usize,
}

#[derive(Clone)]
struct PreparedTableRow {
    height: f32,
    cells: Vec<PreparedTableCell>,
    allow_break_across_pages: bool,
    /// Inclusive last row of a vertical-span group that must stay on one
    /// physical column/page whenever the group fits there. For ordinary rows
    /// this equals the row's own index.
    span_group_end: usize,
}

#[derive(Clone)]
struct PreparedTableCell {
    own_region: ObjectHitRegion,
    object_regions: Vec<ObjectHitRegion>,
    x: f32,
    width: f32,
    /// Painted cell height. This may exceed the owning row height when the
    /// semantic cell has row_span > 1.
    height: f32,
    /// Natural content extent, independent of the shared painted row height.
    content_bottom: f32,
    commands: Vec<DrawCommand>,
    hit_regions: Vec<TextHitRegion>,
    shading: Option<Color>,
    borders: BorderSet,
}

#[derive(Clone)]
struct PreparedCellFragment {
    object_regions: Vec<ObjectHitRegion>,
    height: f32,
    commands: Vec<DrawCommand>,
    hit_regions: Vec<TextHitRegion>,
}

#[derive(Clone)]
struct PreparedLine {
    pagination_break: Option<model::PaginationBreakKind>,
    commands: Vec<DrawCommand>,
    hit_regions: Vec<TextHitRegion>,
    height: f32,
    ascent: f32,
}

#[derive(Clone)]
struct FieldFontPiece { text:String, properties:CharacterProperties, shaped:ShapedRun }

#[derive(Clone)]
enum InlineTokenKind {
    Field { pieces:Vec<FieldFontPiece>, properties:CharacterProperties, lift:f32 },
    Text {
        text: String,
        properties: CharacterProperties,
        shaped: ShapedRun,
    },
    Equation {
        fragment: VectorFragment,
    },
    Image {
        width: f32,
        height: f32,
        commands: Vec<DrawCommand>,
        hit_regions: Vec<TextHitRegion>,
    },
    /// Zero-width unless it terminates a composed line. In that case the
    /// stored hyphen glyph is painted and its width contributes to the break.
    HyphenOpportunity {
        properties: CharacterProperties,
        shaped: ShapedRun,
    },
    Space,
    TabLeader(model::TabLeader),
}

#[derive(Clone)]
struct InlineToken {
    start: usize,
    end: usize,
    whitespace: bool,
    width: f32,
    ascent: f32,
    descent: f32,
    bidi_level: u8,
    kind: InlineTokenKind,
}

struct HardLine {
    break_after: Option<model::PaginationBreakKind>,
    start_offset: usize,
    tokens: Vec<InlineToken>,
}

fn prepare_block(
    block: &Block,
    line_width: f32,
    styles: &StyleSheet,
    ctx: &mut WorkerContext,
    resources: &BTreeMap<ResourceId, Resource>,
    list_marker: Option<&String>,
    field_context: &FieldContext,
) -> Result<PreparedBlock, LayoutError> {
    match block {
        Block::Paragraph(paragraph) => prepare_paragraph(paragraph, line_width, styles, ctx, resources, list_marker.map(String::as_str), field_context),
        Block::Equation(equation) => {
            let fragment = ctx.typst.render_equation_vector(equation, 12.0)?;
            Ok(PreparedBlock::Equation { id: equation.id.0.clone(), fragment, before: 5.0, after: 7.0 })
        }
        Block::Table(table) => Ok(PreparedBlock::Table(prepare_table(table, line_width, styles, ctx, resources, field_context)?)),
        Block::Image(image) => Ok(PreparedBlock::Image(prepare_block_image(image, line_width, resources.get(&image.resource)))),
        Block::PageBreak { .. } => Ok(PreparedBlock::PageBreak),
        Block::ColumnBreak { .. } => Ok(PreparedBlock::ColumnBreak),
        Block::Foreign(_) => Ok(PreparedBlock::Foreign { height: 18.0 }),
    }
}


fn prepare_paragraph(
    paragraph: &Paragraph,
    available_width: f32,
    styles: &StyleSheet,
    ctx: &mut WorkerContext,
    resources: &BTreeMap<ResourceId, Resource>,
    list_marker: Option<&str>,
    field_context: &FieldContext,
) -> Result<PreparedBlock, LayoutError> {
    let mut para_props = styles.resolve_paragraph_with_direct(paragraph.style.as_ref(), &paragraph.properties);
    if let Some(leader) = para_props.contents_leader {
        para_props.tabs = Some(vec![model::TabStop {
            position_mm: available_width / MM_TO_PT,
            alignment: model::TabAlignment::Right,
            leader,
        }]);
    }
    if para_props.default_tab_mm.is_none() {
        para_props.default_tab_mm = styles.resolve_paragraph(Some(&model::StyleId::from(model::STYLE_NORMAL))).default_tab_mm;
    }
    let mut format = styles.resolve_formatting(paragraph.style.as_ref());
    // A foreground pattern can be selected without a background fill. In that
    // case the page white is the pattern's background, just as in the dialog.
    let shade_fill=para_props.shading.as_deref().or_else(||
        format.get("shd_pattern").filter(|pattern|*pattern!="Clear").map(|_|"#FFFFFF"));
    if let Some(fill)=shade_fill {format.insert("shd_effective_fill".into(),fill.to_owned());}
    // A blank paragraph's semantic empty run retains its insertion format.
    // Use it for the blank line/caret metrics, rather than reverting to 11 pt.
    let mark = if paragraph.content.iter().all(|i| model::inline_flat_len(i) == 0) {
        paragraph.content.iter().find_map(|i| if let Inline::Text(run) = i { Some(run) } else { None })
    } else { None };
    let base_char = if let Some(run) = mark {
        styles.resolve_run(paragraph.style.as_ref(), run.style.as_ref(), &run.properties)
    } else { styles.resolve_run(paragraph.style.as_ref(), None, &CharacterProperties::default()) };
    let base_size = base_char.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
    let line_rule = format.get("par_line_rule").map(String::as_str);
    let line_at = format.get("par_line_at_pt").and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite() && *v > 0.0);
    let base_line_height = if matches!(line_rule, Some("At least" | "Exactly")) {
        line_at.unwrap_or(base_size * DEFAULT_LINE_HEIGHT_EM)
    } else { base_size * para_props.line_spacing.unwrap_or(DEFAULT_LINE_HEIGHT_EM) };
    let before = para_props.space_before_pt.unwrap_or(0.0);
    let mut after = para_props.space_after_pt.unwrap_or(base_size * 0.3);
    let mut left_indent = para_props.left_indent_mm.unwrap_or(0.0) * MM_TO_PT;
    let right_indent = para_props.right_indent_mm.unwrap_or(0.0) * MM_TO_PT;
    if list_marker.is_some() {
        let level = format.get("num_level").and_then(|v| v.split_whitespace().last()).and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0).clamp(1.0, 9.0);
        left_indent = left_indent.max(18.0 + (level - 1.0) * 12.0);
    }
    para_props.left_indent_mm = Some(left_indent / MM_TO_PT);
    let hanging_indent = (-para_props.first_line_indent_mm.unwrap_or(0.0) * MM_TO_PT).max(0.0);
    // In OOXML list paragraphs, the hanging indent locates the marker; body
    // text starts at the left indent on every line. Applying it to the first
    // text line places the bullet on top of the first word.
    let first_indent = if list_marker.is_some() && hanging_indent > 0.0 { 0.0 }
        else { para_props.first_line_indent_mm.unwrap_or(0.0) * MM_TO_PT };
    let mut content_width = (available_width - left_indent - right_indent).max(1.0);
    if format.get("frm_width_rule").is_some_and(|rule| rule == "Exactly") {
        let fixed = style_number(&format, "frm_width_at", 0.0);
        if fixed > 1.0 {
            let width = fixed.min(content_width);
            let spare = content_width - width;
            left_indent += match format.get("frm_h_position").map(String::as_str) {
                Some("Right" | "Outside") => spare,
                Some("Center") => spare * 0.5,
                _ => 0.0,
            };
            content_width = width;
        }
    }
    let alignment = para_props.alignment.unwrap_or(ParagraphAlignment::Left);
    let max_expansion = para_props.glyph_expansion_per_mille.unwrap_or(15) as f32 / 1000.0;

    let mut hard_lines = collect_inline_tokens(paragraph, styles, ctx, resources, &base_char, field_context)?;
    // Resolve directions before discretionary breaks; a syllable boundary is
    // not a shaping-run boundary unless the compositor actually uses it.
    let source_text = paragraph_plain_text(paragraph);
    apply_bidi_levels(&mut hard_lines, &source_text, &ctx.fonts)?;
    if para_props.contents_leader.is_some() {
        // A contents entry has separate heading and numeric columns. Keep
        // the tab and page number on the right even for a Hebrew/Arabic title.
        for token in hard_lines.iter_mut().flat_map(|line| &mut line.tokens) {
            if source_text.get(token.start..token.end) == Some("\t") {
                token.bidi_level = 0;
                token.width = 0.0;
            } else if matches!(&token.kind, InlineTokenKind::Field { .. }) {
                token.bidi_level = 0;
            }
        }
    }
    if para_props.hyphenation.unwrap_or(false) {
        apply_hyphenation(&mut hard_lines, &ctx.fonts)?;
    }
    let mut prepared_lines = Vec::new();
    let mut visual_line_index = 0usize;

    for hard_line in hard_lines {
        let pagination_break = hard_line.break_after;
        if hard_line.tokens.is_empty() && pagination_break.is_none() {
            prepared_lines.push(empty_line(&paragraph.id.0, hard_line.start_offset, visual_line_index, base_line_height, base_size));
            visual_line_index += 1; continue;
        }
        let smallest_goal=(content_width-first_indent.max(0.0)).max(1.0);
        let tokens=line_fitting::split_overwide_text(hard_line.tokens,smallest_goal);
        let balanced=alignment==ParagraphAlignment::Justify && first_indent==0.0;
        let mut breaks=if balanced {line_fitting::balanced_breaks(&tokens,content_width,max_expansion)}else{Vec::new()};
        let mut start=0;
        while start<tokens.len() {
            let indent=if visual_line_index==0{first_indent}else{0.0};
            let preferred=breaks.iter().copied().find(|b|*b>start);
            let (end,line_tokens)=line_fitting::next_line(&tokens,start,&source_text,&para_props,content_width,indent,preferred,&ctx.fonts)?;
            // Reshaping can make a line end before its planned break. The rest
            // of the plan then assumed the wrong start and would strand the
            // word that moved down on a line of its own, so plan again from here.
            if balanced && preferred.is_some_and(|p|p!=end) && end<tokens.len() {
                breaks=line_fitting::balanced_breaks(&tokens[end..],content_width,max_expansion).into_iter().map(|b|b+end).collect();
            }
            let has_tab=line_tokens.iter().any(|t|matches!(t.kind,InlineTokenKind::TabLeader(_)));
            let goal=(content_width-indent).max(1.0);
            let (ratio,scale)=if !has_tab && alignment==ParagraphAlignment::Justify && end<tokens.len(){
                line_fitting::justification(&line_tokens,goal,max_expansion)
            }else{(0.0,1.0)};
            let mut line = vectorize_mixed_line(&line_tokens,&paragraph.id.0,visual_line_index,goal,
                ratio,scale,if has_tab{ParagraphAlignment::Left}else{alignment},base_line_height,indent,
                para_props.hanging_punctuation.unwrap_or(true),&format,&ctx.fonts)?;
            if line_rule == Some("Exactly") { line.height = base_line_height; }
            prepared_lines.push(line);
            visual_line_index+=1; start=end;
        }
        if let Some(kind) = pagination_break {
            prepared_lines.push(PreparedLine { pagination_break: Some(kind), commands: Vec::new(), hit_regions: Vec::new(), height: 0.0, ascent: 0.0 });
        }
    }

    if prepared_lines.is_empty() {
        prepared_lines.push(empty_line(&paragraph.id.0, 0, 0, base_line_height, base_size));
    }
    // Bar stops are paragraph rules, independent of tab characters.
    for line in &mut prepared_lines {
        for stop in para_props.tabs.iter().flatten().filter(|stop| stop.alignment == model::TabAlignment::Bar) {
            let x = stop.position_mm * MM_TO_PT - left_indent;
            if x >= 0.0 && x <= content_width {
                line.commands.push(DrawCommand::FillRect { rect: Rect::new(x, -line.ascent, 0.8, line.height), paint: Paint::Solid(Color::BLACK) });
            }
        }
    }
    let frame_height = style_number(&format, "frm_height_at", 0.0);
    if frame_height > 0.0 && matches!(format.get("frm_height_rule").map(String::as_str), Some("At least" | "Exactly")) {
        let text_height = prepared_lines.iter().map(|line| line.height).sum::<f32>();
        after += (frame_height - text_height).max(0.0);
    }

    let mut marker = list_marker
        .map(|text| prepare_list_marker(text, &base_char, &ctx.fonts))
        .transpose()?;
    if let Some(marker) = &mut marker {
        if hanging_indent > 0.0 { marker.gap = (hanging_indent - marker.width).max(4.0); }
    }

    let paragraph_fill = if format.get("shd_apply").is_some_and(|value|value=="Text") {None}
        else {shade_fill.and_then(|color|shading_color(color,&format))};
    let has_pattern=format.get("shd_pattern").is_some_and(|pattern|pattern.contains("Horizontal")||pattern.contains("Vertical")||pattern.contains("Grid"));
    let decoration = (paragraph_fill.is_some() || para_props.borders.is_some() || has_pattern).then(|| ParagraphDecoration {
        shading: paragraph_fill,
        borders: para_props.borders.clone(),
        width: content_width,
        preset: format.get("bdr_setting").and_then(|value|value.parse().ok()).unwrap_or(0),
        border_distance:["bdr_from_top","bdr_from_bottom","bdr_from_left","bdr_from_right"].map(|key|style_number(&format,key,0.0).clamp(0.0,48.0)),
        pattern:format.get("shd_pattern").filter(|pattern|pattern.contains("Horizontal")||pattern.contains("Vertical")||pattern.contains("Grid"))
            .map(|pattern|(pattern.clone(),format.get("shd_pattern_color").and_then(|value|parse_color(Some(value))).unwrap_or(Color::BLACK))),
    });

    Ok(PreparedBlock::Paragraph {
        lines: prepared_lines,
        space_before: before,
        space_after: after,
        left_indent,
        marker,
        decoration,
        keep_with_next: para_props.keep_with_next.unwrap_or(false),
        keep_lines_together: para_props.keep_lines_together.unwrap_or(false),
        page_break_before: para_props.page_break_before.unwrap_or(false),
        widow_control: para_props.widow_control.unwrap_or(true),
    })
}

fn resolve_field_text(kind: &FieldKind, context: &FieldContext) -> String {
    match kind {
        FieldKind::PageNumber => format_page_number(context.page_number, context.page_number_format),
        FieldKind::PageReference{..}|FieldKind::FootnoteReference{..} => "?".into(),
        FieldKind::PageCount => context.page_count.to_string(),
        FieldKind::SectionNumber => context.section_number.to_string(),
        FieldKind::Date { .. } => String::new(),
        FieldKind::Raw { instruction } => instruction.clone(),
    }
}

fn prepare_list_marker(
    text: &str,
    base: &CharacterProperties,
    fonts: &FontCatalog,
) -> Result<PreparedMarker, LayoutError> {
    let text = &*model::replace_symbol_font_chars(text);
    let mut props = normalized_shaping_properties(base.clone());
    props.bold = Some(false);
    let shaped = shape_token(text, &props, fonts)?;
    let width = shaped.advance_pt.max(1.0);
    let (command, _) = vectorize_text_token(
        text,
        0,
        0,
        "__list_marker__",
        0,
        false,
        &props,
        &shaped,
        0.0,
        1.0,
        width,
        props.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT) * 1.2,
        fonts,
    )?;
    Ok(PreparedMarker { commands: vec![command], width, gap: 4.0 })
}

fn prepare_block_image(image: &model::BlockImage, available_width: f32, resource: Option<&Resource>) -> PreparedImage {
    let width = (image.width_mm * MM_TO_PT).max(8.0).min(available_width.max(8.0));
    let natural_height = (image.height_mm * MM_TO_PT).max(8.0);
    let scale = if image.width_mm > 0.0 { width / (image.width_mm * MM_TO_PT).max(1.0) } else { 1.0 };
    let height = (natural_height * scale).max(8.0);
    PreparedImage {
        id: image.id.0.clone(),
        width,
        height,
        alignment: image.alignment,
        commands: resource.and_then(|r| raster_preview_commands(r, width, height, &image.effects)).unwrap_or_else(|| image_frame_commands(width, height)),
    }
}

fn raster_preview_commands(resource: &Resource, width: f32, height: f32, effects: &model::ImageEffects) -> Option<Vec<DrawCommand>> {
    // Runs on the layout workers. Cache exact source bytes, decode once, and
    // keep native resolution and premultiplied linear-light mipmaps for display.
    if effects.crop.is_none() && effects.outer_shadow.is_none() && effects.glow.is_none()
        && effects.soft_edge_mm.is_none() && effects.reflection.is_none()
        && effects.shape.preset == "rect" && effects.outline.is_none() {
        let image = render::decode_raster_cached(&resource.data).ok()?;
        return Some(vec![DrawCommand::Image { rect: Rect::new(0.0, 0.0, width, height), image }]);
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(resource.data.as_slice())).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(render::MAX_RASTER_BYTES as u64);
    reader.limits(limits);
    let mut rgba = reader.decode().ok()?.to_rgba8();
    if let Some(crop) = &effects.crop {
        let (w, h) = rgba.dimensions();
        let x = (crop.left.clamp(0.0, 1.0) * w as f32).round() as u32;
        let y = (crop.top.clamp(0.0, 1.0) * h as f32).round() as u32;
        let right = (crop.right.clamp(0.0, 1.0) * w as f32).round() as u32;
        let bottom = (crop.bottom.clamp(0.0, 1.0) * h as f32).round() as u32;
        rgba = image::imageops::crop_imm(&rgba, x.min(w - 1), y.min(h - 1),
            w.saturating_sub(x + right).max(1), h.saturating_sub(y + bottom).max(1)).to_image();
    }
    let geometry = render::picture_shapes::geometry(&effects.shape.preset, &effects.shape.adjustments, width, height)
        .or_else(|| render::picture_shapes::geometry("rect", &Default::default(), width, height))?;
    if effects.shape.preset != "rect" {
        let mask = render::picture_shapes::crop_mask(&geometry, rgba.width(), rgba.height(), width, height)?;
        for (pixel, alpha) in rgba.pixels_mut().zip(mask) {
            pixel[3] = ((u16::from(pixel[3]) * u16::from(alpha) + 127) / 255) as u8;
        }
    }
    let mut commands = Vec::new();
    if let Some(glow)=&effects.glow {
        let (w,h)=rgba.dimensions();
        let radius_px=(glow.radius_mm.max(0.0)*MM_TO_PT*w as f32/width.max(1.0)).min(100.0);
        let pad=(radius_px*3.0).ceil().min(256.0)as u32;
        let (gw,gh)=(w.saturating_add(2*pad),h.saturating_add(2*pad));
        let mut mask=image::GrayImage::new(gw,gh);
        for (x,y,p) in rgba.enumerate_pixels(){mask.put_pixel(x+pad,y+pad,image::Luma([p[3]]));}
        if radius_px>0.1 {mask=image::imageops::blur(&mask,radius_px);}
        let color=u32::from_str_radix(glow.color.trim_start_matches('#'),16).unwrap_or(0);
        let mut pixels=Vec::with_capacity(gw as usize*gh as usize*4);
        for p in mask.pixels(){pixels.extend_from_slice(&[(color>>16)as u8,(color>>8)as u8,color as u8,
            (p[0]as f32*glow.opacity.clamp(0.0,1.0)).round()as u8]);}
        if let Ok(image)=render::RasterImage::from_rgba(gw,gh,&pixels){
            let (px,py)=(pad as f32*width/w as f32,pad as f32*height/h as f32);
            commands.push(DrawCommand::Image{rect:Rect::new(-px,-py,width+2.0*px,height+2.0*py),image:std::sync::Arc::new(image)});
        }
    }
    if let Some(shadow) = &effects.outer_shadow {
        let (w, h) = rgba.dimensions();
        let sigma = (shadow.blur_mm.max(0.0) / 25.4 * w as f32 / (width / 72.0).max(0.01) / 2.0).min(100.0);
        let pad = (sigma * 3.0).ceil().min(256.0) as u32;
        let (padded_w, padded_h) = (w.saturating_add(2 * pad), h.saturating_add(2 * pad));
        let mut mask = image::GrayImage::new(padded_w, padded_h);
        for (x, y, pixel) in rgba.enumerate_pixels() { mask.put_pixel(x + pad, y + pad, image::Luma([pixel[3]])); }
        if sigma > 0.1 { mask = image::imageops::blur(&mask, sigma); }
        let hex = shadow.color.trim_start_matches('#');
        let rgb = if hex.len() == 6 { u32::from_str_radix(hex, 16).unwrap_or(0) } else { 0 };
        let color = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
        let mut pixels = Vec::with_capacity((padded_w as usize) * (padded_h as usize) * 4);
        for p in mask.pixels() { pixels.extend_from_slice(&[color[0], color[1], color[2],
            ((p[0] as f32) * shadow.opacity.clamp(0.0, 1.0)).round() as u8]); }
        if let Ok(image) = render::RasterImage::from_rgba(padded_w, padded_h, &pixels) {
            let angle = shadow.direction_degrees.to_radians();
            let distance = shadow.distance_mm * MM_TO_PT;
            let (pad_x, pad_y) = (pad as f32 * width / w as f32, pad as f32 * height / h as f32);
            commands.push(DrawCommand::Image { rect: Rect::new(angle.cos() * distance - pad_x,
                angle.sin() * distance - pad_y, width + 2.0 * pad_x, height + 2.0 * pad_y), image: std::sync::Arc::new(image) });
        }
    }
    if let Some(reflection)=&effects.reflection {
        let (w,h)=rgba.dimensions();
        let mut mirrored=image::RgbaImage::new(w,h);
        for y in 0..h {for x in 0..w {
            let mut pixel=*rgba.get_pixel(x,h-1-y);
            let fraction=if h>1{y as f32/(h-1)as f32}else{0.0};
            let opacity=reflection.start_opacity+(reflection.end_opacity-reflection.start_opacity)*fraction;
            pixel[3]=(pixel[3]as f32*opacity.clamp(0.0,1.0)).round()as u8;
            mirrored.put_pixel(x,y,pixel);
        }}
        let sigma=(reflection.blur_mm.max(0.0)*MM_TO_PT*w as f32/width.max(1.0)).min(100.0);
        if sigma>0.1 {mirrored=image::imageops::blur(&mirrored,sigma);}
        if let Ok(image)=render::RasterImage::from_rgba(w,h,mirrored.as_raw()){
            commands.push(DrawCommand::Image{rect:Rect::new(0.0,height+reflection.distance_mm.max(0.0)*MM_TO_PT,width,height),image:std::sync::Arc::new(image)});
        }
    }
    if let Some(radius)=effects.soft_edge_mm {
        let (w,h)=rgba.dimensions();
        let rx=(radius.max(0.0)*MM_TO_PT*w as f32/width.max(1.0)).max(0.1);
        let ry=(radius.max(0.0)*MM_TO_PT*h as f32/height.max(1.0)).max(0.1);
        for (x,y,pixel) in rgba.enumerate_pixels_mut(){
            let edge=((x.min(w-1-x)as f32/rx).min(y.min(h-1-y)as f32/ry)).clamp(0.0,1.0);
            pixel[3]=(pixel[3]as f32*edge).round()as u8;
        }
    }
    let image = std::sync::Arc::new(render::RasterImage::from_rgba(rgba.width(), rgba.height(), rgba.as_raw()).ok()?);
    commands.push(DrawCommand::Image { rect: Rect::new(0.0, 0.0, width, height), image });
    if let Some(outline) = effects.outline.as_ref().filter(|line| line.style != BorderStyle::None && line.width_pt > 0.0) {
        for path in geometry.into_iter().filter(|path| path.stroke) {
            commands.push(DrawCommand::Path(render::PathObject {
                path: path.path,
                fill: None,
                stroke: Some((Paint::Solid(parse_color(Some(&outline.color)).unwrap_or(Color::BLACK)), render::StrokeStyle { width_pt: outline.width_pt })),
                transform: Affine2D::IDENTITY,
            }));
        }
    }
    Some(commands)
}

fn image_frame_commands(width: f32, height: f32) -> Vec<DrawCommand> {
    let mut commands = vec![DrawCommand::FillRect {
        rect: Rect::new(0.0, 0.0, width, height),
        paint: Paint::Solid(Color { r: 0.965, g: 0.965, b: 0.965, a: 1.0 }),
    }];
    let border = Color { r: 0.55, g: 0.55, b: 0.55, a: 1.0 };
    let t = 0.65;
    commands.extend([
        DrawCommand::FillRect { rect: Rect::new(0.0, 0.0, width, t), paint: Paint::Solid(border) },
        DrawCommand::FillRect { rect: Rect::new(0.0, height - t, width, t), paint: Paint::Solid(border) },
        DrawCommand::FillRect { rect: Rect::new(0.0, 0.0, t, height), paint: Paint::Solid(border) },
        DrawCommand::FillRect { rect: Rect::new(width - t, 0.0, t, height), paint: Paint::Solid(border) },
    ]);
    commands
}

fn resolve_table_column_widths(
    table: &Table,
    placements: &[Vec<(usize, usize, usize)>],
    column_count: usize,
    total_width: f32,
) -> Vec<f32> {
    if column_count == 0 { return Vec::new(); }
    let explicit = table.properties.grid_column_widths_mm.iter().copied().take(column_count)
        .filter(|value| value.is_finite() && *value > 0.0).collect::<Vec<_>>();
    let mut logical_widths = if explicit.len() == column_count {
        explicit.into_iter().map(|mm| mm * MM_TO_PT).collect::<Vec<_>>()
    } else {
        let mut hints = vec![None::<f32>; column_count];
        for (row_index, row) in table.rows.iter().enumerate() {
            for (cell_index, cell) in row.cells.iter().enumerate() {
                let Some(mm) = cell.width_mm.filter(|v| v.is_finite() && *v > 0.0) else { continue; };
                let (start, span, _) = placements[row_index][cell_index];
                let per_column = mm * MM_TO_PT / span.max(1) as f32;
                for column in start..(start + span).min(column_count) {
                    if hints[column].is_none() { hints[column] = Some(per_column); }
                }
            }
        }
        let fallback = total_width / column_count as f32;
        hints.into_iter().map(|hint| hint.unwrap_or(fallback)).collect::<Vec<_>>()
    };
    let logical_total = logical_widths.iter().sum::<f32>();
    if logical_total > 0.0 {
        let scale = total_width / logical_total;
        for value in &mut logical_widths { *value *= scale; }
    } else {
        logical_widths.fill(total_width / column_count as f32);
    }
    logical_widths
}

fn prepare_table(
    table: &Table,
    available_width: f32,
    styles: &StyleSheet,
    ctx: &mut WorkerContext,
    resources: &BTreeMap<ResourceId, Resource>,
    field_context: &FieldContext,
) -> Result<PreparedTable, LayoutError> {
    // Resolve semantic cells onto a logical grid while respecting active
    // vertical spans. Continuation rows intentionally omit the occupied cell;
    // the top semantic cell owns the whole row-span and is painted once.
    let mut placements: Vec<Vec<(usize, usize, usize)>> = Vec::with_capacity(table.rows.len());
    let mut span_until: Vec<usize> = Vec::new();
    let mut column_count = 1usize;
    for (row_index, row) in table.rows.iter().enumerate() {
        let mut row_placements = Vec::with_capacity(row.cells.len());
        let mut cursor = 0usize;
        for cell in &row.cells {
            let span = usize::from(cell.column_span.max(1));
            loop {
                if span_until.len() < cursor + span { span_until.resize(cursor + span, 0); }
                if (cursor..cursor + span).all(|column| span_until[column] <= row_index) { break; }
                cursor += 1;
            }
            let row_span = usize::from(cell.row_span.max(1)).min(table.rows.len().saturating_sub(row_index).max(1));
            let end_row = row_index + row_span;
            for column in cursor..cursor + span { span_until[column] = span_until[column].max(end_row); }
            row_placements.push((cursor, span, row_span));
            cursor += span;
            column_count = column_count.max(cursor).max(span_until.len());
        }
        placements.push(row_placements);
    }

    let explicit_grid_mm = table.properties.grid_column_widths_mm.iter()
        .copied().take(column_count).filter(|value| value.is_finite() && *value > 0.0).collect::<Vec<_>>();
    let grid_width_mm = (explicit_grid_mm.len() == column_count)
        .then(|| explicit_grid_mm.iter().sum::<f32>())
        .filter(|sum| sum.is_finite() && *sum > 0.0);
    let requested_width = table.properties.preferred_width_mm
        .or(grid_width_mm)
        .map(|mm| mm * MM_TO_PT)
        .unwrap_or(available_width);
    // An authored table may extend into page margins. Shrinking it to the
    // text width changes column wrapping and pagination for DOCX files.
    let width = requested_width.max(1.0);

    // A stable logical grid is the source of truth for column sizing.  DOCX
    // `w:tblGrid` maps here, and merged cells simply sum the columns they cover.
    let logical_widths = resolve_table_column_widths(table, &placements, column_count, width);
    let mut logical_lefts = vec![0.0f32; column_count + 1];
    for column in 0..column_count { logical_lefts[column + 1] = logical_lefts[column] + logical_widths[column]; }

    let x_offset = match table.properties.alignment.unwrap_or(TableAlignment::Left) {
        TableAlignment::Left => table.properties.left_indent_mm.unwrap_or(0.0) * MM_TO_PT,
        TableAlignment::Center => (available_width - width) * 0.5,
        TableAlignment::Right => available_width - width,
    };
    let padding = model::TABLE_TEXT_PADDING_PT;

    // Prepare each cell once, then derive physical row heights. A spanning cell
    // contributes its minimum content height across all rows it occupies.
    let prepare_row = |ctx: &mut WorkerContext, row_index: usize|
        -> Result<Vec<(&TableCell, usize, usize, usize, PreparedCellFragment)>, LayoutError> {
        let row = &table.rows[row_index];
        let mut row_prepared = Vec::with_capacity(row.cells.len());
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let (start_column, span, row_span) = placements[row_index][cell_index];
            let cell_width = logical_widths[start_column..(start_column + span).min(column_count)].iter().sum::<f32>();
            let cell_padding = padding.min(((cell_width - 1.0) * 0.5).max(0.0));
            let inner_width = (cell_width - cell_padding * 2.0).max(1.0);
            let fragment = prepare_block_fragment(&cell.blocks, inner_width, styles, ctx, resources, field_context)?;
            row_prepared.push((cell, start_column, span, row_span, fragment));
        }
        Ok(row_prepared)
    };
    // A DOCX can contain almost its entire body in one table. Parallelizing
    // only top-level blocks leaves that document on one layout worker. Rows
    // have independent content; preserve their order when collecting results.
    let mut prepared_cells: Vec<Vec<(&TableCell, usize, usize, usize, PreparedCellFragment)>> =
        if table.rows.len() >= 4 {
            (0..table.rows.len()).into_par_iter().map_init(
                || WorkerContext { fonts: FontCatalog::for_resources(resources), typst: TypstTypesetter::new() },
                |local, row_index| prepare_row(local, row_index),
            ).collect::<Result<Vec<_>, _>>()?
        } else {
            (0..table.rows.len()).map(|row_index| prepare_row(ctx, row_index))
                .collect::<Result<Vec<_>, _>>()?
        };
    let mut row_heights: Vec<f32> = table.rows.iter().map(|row| row.height_mm.map(|mm| mm * MM_TO_PT).unwrap_or(18.0).max(18.0)).collect();
    let mut span_requirements: Vec<(usize, usize, f32)> = Vec::new();
    let mut protected_rows = vec![false; table.rows.len()];
    let mut intervals = Vec::<(usize, usize)>::new();

    for (row_index, row_prepared) in prepared_cells.iter().enumerate() {
        for (_cell, _start_column, _span, row_span, fragment) in row_prepared {
            let required = (fragment.height + padding * 2.0).max(18.0);
            if *row_span == 1 {
                row_heights[row_index] = row_heights[row_index].max(required);
            } else {
                let end = (row_index + row_span).min(table.rows.len());
                span_requirements.push((row_index, end, required));
                intervals.push((row_index, end));
                for protected in &mut protected_rows[row_index..end] { *protected = true; }
            }
        }
    }

    for (start, end, required) in &span_requirements {
        let current = row_heights[*start..*end].iter().sum::<f32>();
        if *required > current {
            let add = (*required - current) / (*end - *start).max(1) as f32;
            for height in &mut row_heights[*start..*end] { *height += add; }
        }
    }

    // Merge overlapping row-span intervals into indivisible pagination groups.
    intervals.sort_unstable();
    let mut groups = Vec::<(usize, usize)>::new();
    for (start, end) in intervals {
        if let Some(last) = groups.last_mut() {
            if start < last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        groups.push((start, end));
    }
    let mut group_end_at_start: Vec<usize> = (0..table.rows.len()).collect();
    for (start, end) in groups {
        if start < group_end_at_start.len() { group_end_at_start[start] = end.saturating_sub(1); }
    }

    let mut rows = Vec::with_capacity(table.rows.len());
    for (row_index, row) in table.rows.iter().enumerate() {
        let mut cells = Vec::with_capacity(row.cells.len());
        for (cell_index, (cell, start_column, span, row_span, fragment)) in prepared_cells[row_index].drain(..).enumerate() {
            let end = (row_index + row_span).min(row_heights.len());
            let cell_height = row_heights[row_index..end].iter().sum::<f32>();
            let inner_height = fragment.height;
            let vertical = match cell.vertical_alignment {
                CellVerticalAlignment::Top => padding,
                CellVerticalAlignment::Center => ((cell_height - inner_height) * 0.5).max(padding),
                CellVerticalAlignment::Bottom => (cell_height - inner_height - padding).max(padding),
            };
            let cell_width = logical_widths[start_column..(start_column + span).min(column_count)].iter().sum::<f32>();
            let cell_padding = padding.min(((cell_width - 1.0) * 0.5).max(0.0));
            let commands = fragment.commands.iter().map(|c| c.translated(cell_padding, vertical)).collect();
            let hit_regions = fragment.hit_regions.iter().map(|r| r.translated(cell_padding, vertical)).collect();
            cells.push(PreparedTableCell {
                own_region: ObjectHitRegion { node_id: cell.id.0.clone(), rect: Rect::new(0.0, 0.0, cell_width, cell_height),
                    kind: ObjectKind::TableCell(TableCellHit { table_id: table.id.0.clone(), row_index, cell_index,
                        column_index: start_column, column_span: span, row_span, column_widths_pt: logical_widths.clone(),
                        row_height_pt: row_heights[row_index], can_resize_row: row_span == 1 && !protected_rows[row_index] }) },
                object_regions: fragment.object_regions.iter().map(|r| r.translated(cell_padding, vertical)).collect(),
                x: logical_lefts[start_column],
                width: logical_widths[start_column..(start_column + span).min(column_count)].iter().sum::<f32>(),
                height: cell_height,
                content_bottom: vertical + fragment.height,
                commands,
                hit_regions,
                shading: cell.shading.as_deref().and_then(|s| parse_color(Some(s))),
                borders: merged_cell_borders(cell, table),
            });
        }
        rows.push(PreparedTableRow {
            height: row_heights[row_index],
            cells,
            allow_break_across_pages: row.allow_break_across_pages && !protected_rows[row_index],
            span_group_end: group_end_at_start[row_index],
        });
    }
    let repeating_header_rows = table.rows.iter().take_while(|row| row.repeat_as_header).count();
    Ok(PreparedTable { x_offset, rows, repeating_header_rows })
}

fn merged_cell_borders(cell: &TableCell, table: &Table) -> BorderSet {
    let mut borders = table.properties.borders.clone().unwrap_or_default();
    if let Some(cell_borders) = &cell.borders {
        if cell_borders.top.is_some() { borders.top = cell_borders.top.clone(); }
        if cell_borders.right.is_some() { borders.right = cell_borders.right.clone(); }
        if cell_borders.bottom.is_some() { borders.bottom = cell_borders.bottom.clone(); }
        if cell_borders.left.is_some() { borders.left = cell_borders.left.clone(); }
    }
    borders
}

fn prepare_block_fragment(
    blocks: &[Block],
    width: f32,
    styles: &StyleSheet,
    ctx: &mut WorkerContext,
    resources: &BTreeMap<ResourceId, Resource>,
    field_context: &FieldContext,
) -> Result<PreparedCellFragment, LayoutError> {
    let mut y = 0.0f32;
    let mut commands = Vec::new();
    let mut hit_regions = Vec::new();
    let mut object_regions = Vec::new();
    for block in blocks {
        let prepared = prepare_block(block, width, styles, ctx, resources, None, field_context)?;
        match prepared {
            PreparedBlock::Paragraph { lines, space_before, space_after, left_indent, marker, decoration, .. } => {
                y += space_before;
                if let Some(decoration) = &decoration {
                    let height = lines.iter().map(|line| line.height).sum();
                    append_paragraph_decoration(&mut commands, decoration,
                        Rect::new(left_indent, y, decoration.width, height), true, true);
                }
                let mut first = true;
                for line in lines {
                    let baseline = y + line.ascent;
                    if first {
                        if let Some(marker) = &marker {
                            commands.extend(marker.commands.iter().map(|c| c.translated((left_indent - marker.width - marker.gap).max(0.0), baseline)));
                        }
                        first = false;
                    }
                    commands.extend(line.commands.iter().map(|c| c.translated(left_indent, baseline)));
                    hit_regions.extend(line.hit_regions.iter().map(|r| r.translated(left_indent, baseline)));
                    y += line.height;
                }
                y += space_after;
            }
            PreparedBlock::Equation { id, fragment, before, after } => {
                y += before;
                let x = ((width - fragment.size.width) * 0.5).max(0.0);
                commands.extend(fragment.commands.iter().map(|c| c.translated(x, y)));
                object_regions.push(ObjectHitRegion { node_id: id, rect: Rect::new(x, y, fragment.size.width, fragment.size.height), kind: ObjectKind::Equation { source: None } });
                y += fragment.size.height + after;
            }
            PreparedBlock::Image(image) => {
                let x = match image.alignment {
                    ImageAlignment::Left => 0.0,
                    ImageAlignment::Center => ((width - image.width) * 0.5).max(0.0),
                    ImageAlignment::Right => (width - image.width).max(0.0),
                };
                commands.extend(image.commands.iter().map(|c| c.translated(x, y)));
                object_regions.push(ObjectHitRegion { node_id: image.id.clone(), rect: Rect::new(x, y, image.width, image.height), kind: ObjectKind::Picture { source: None } });
                y += image.height + 3.0;
            }
            PreparedBlock::Table(table) => {
                for row in &table.rows {
                    let mut temp_page = VectorPage::new(Size::new(width, row.height));
                    render_table_row(&mut temp_page, row, table.x_offset, 0.0);
                    commands.extend(temp_page.commands.into_iter().map(|c| c.translated(0.0, y)));
                    hit_regions.extend(temp_page.hit_regions.into_iter().map(|r| r.translated(0.0, y)));
                    object_regions.extend(temp_page.object_regions.into_iter().map(|r| r.translated(0.0, y)));
                    y += row.height;
                }
                y += 3.0;
            }
            PreparedBlock::PageBreak | PreparedBlock::ColumnBreak => y += 4.0,
            PreparedBlock::Foreign { height } => y += height,
        }
    }
    Ok(PreparedCellFragment { height: y.max(1.0), commands, hit_regions, object_regions })
}

fn prepare_text_box(
    boxed: &model::TextBox,
    styles: &StyleSheet,
    ctx: &mut WorkerContext,
    resources: &BTreeMap<ResourceId, Resource>,
    fields: &FieldContext,
) -> Result<(Vec<DrawCommand>, Vec<TextHitRegion>), LayoutError> {
    let width = (boxed.width_mm * MM_TO_PT).max(4.0);
    let height = (boxed.height_mm * MM_TO_PT).max(4.0);
    let inset = 3.6;
    let inner = Rect::new(inset, inset, (width - 2.0 * inset).max(1.0), (height - 2.0 * inset).max(1.0));
    let mut commands = Vec::new();
    if let Some(color) = parse_color(boxed.fill.as_deref()) {
        commands.push(DrawCommand::FillRect { rect: Rect::new(0.0, 0.0, width, height), paint: Paint::Solid(color) });
    }
    if let Some(color) = parse_color(boxed.line.as_deref()) {
        let t = 0.75;
        for rect in [Rect::new(0.0, 0.0, width, t), Rect::new(0.0, height - t, width, t),
            Rect::new(0.0, 0.0, t, height), Rect::new(width - t, 0.0, t, height)] {
            commands.push(DrawCommand::FillRect { rect, paint: Paint::Solid(color) });
        }
    }
    let fragment = prepare_block_fragment(&boxed.blocks, inner.width, styles, ctx, resources, fields)?;
    commands.extend(fragment.commands.into_iter().map(|command| command.translated(inset, inset).clipped(inner)));
    let hit_regions = fragment.hit_regions.into_iter().filter_map(|region| region.translated(inset, inset).clipped(inner)).collect();
    Ok((commands, hit_regions))
}

fn collect_inline_tokens(
    paragraph: &Paragraph,
    styles: &StyleSheet,
    ctx: &mut WorkerContext,
    resources: &BTreeMap<ResourceId, Resource>,
    base_char: &CharacterProperties,
    field_context: &FieldContext,
) -> Result<Vec<HardLine>, LayoutError> {
    let mut lines = vec![HardLine { break_after: None, start_offset: 0, tokens: Vec::new() }];
    let mut source_offset = 0usize;
    collect_inline_slice(
        &paragraph.content,
        paragraph,
        styles,
        ctx,
        resources,
        base_char,
        &mut source_offset,
        &mut lines,
        field_context,
    )?;
    Ok(lines)
}

#[allow(clippy::too_many_arguments)]
fn collect_inline_slice(
    content: &[Inline],
    paragraph: &Paragraph,
    styles: &StyleSheet,
    ctx: &mut WorkerContext,
    resources: &BTreeMap<ResourceId, Resource>,
    base_char: &CharacterProperties,
    source_offset: &mut usize,
    lines: &mut Vec<HardLine>,
    field_context: &FieldContext,
) -> Result<(), LayoutError> {
    let mut index=0;
    while index<content.len() {
        let inline=&content[index];
        index+=1;
        match inline {
            Inline::Text(run) => {
                if run.text.is_empty() { continue; }
                // Imported DOCX runs and edits may encode identical formatting
                // differently. Shape their resolved text together, without
                // rewriting the document or ligating across true style/objects.
                let props = normalized_shaping_properties(styles.resolve_run(paragraph.style.as_ref(), run.style.as_ref(), &run.properties));
                let mut text=run.text.clone();
                while let Some(Inline::Text(next))=content.get(index) {
                    if next.text.is_empty() { index += 1; continue; }
                    let next_props=normalized_shaping_properties(styles.resolve_run(paragraph.style.as_ref(),next.style.as_ref(),&next.properties));
                    if props!=next_props {break;}
                    text.push_str(&next.text);index+=1;
                }
                collect_text_tokens(&text, &props, source_offset, lines, &ctx.fonts)?;
            }
            Inline::Equation(eq) => {
                let size = base_char.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
                let fragment = ctx.typst.render_equation_vector(eq, size)?;
                let start = *source_offset;
                *source_offset += INLINE_OBJECT_MARKER_LEN;
                let baseline = fragment.baseline_pt.clamp(0.0, fragment.size.height.max(0.0));
                lines.last_mut().expect("hard line exists").tokens.push(InlineToken {
                    start,
                    end: *source_offset,
                    whitespace: false,
                    width: fragment.size.width,
                    ascent: baseline,
                    descent: (fragment.size.height - baseline).max(0.0),
                    bidi_level: 0,
                    kind: InlineTokenKind::Equation { fragment },
                });
            }
            Inline::Image(image) => {
                let start = *source_offset;
                *source_offset += INLINE_OBJECT_MARKER_LEN;
                let width = (image.width_mm * MM_TO_PT).max(1.0);
                let height = (image.height_mm * MM_TO_PT).max(1.0);
                lines.last_mut().expect("hard line exists").tokens.push(InlineToken {
                    start,
                    end: *source_offset,
                    whitespace: false,
                    width,
                    ascent: height,
                    descent: 0.0,
                    bidi_level: 0,
                    kind: InlineTokenKind::Image {
                        width, height,
                        commands: resources.get(&image.resource).and_then(|r| raster_preview_commands(r, width, height, &image.effects)).unwrap_or_else(|| image_frame_commands(width, height)),
                        hit_regions: Vec::new(),
                    },
                });
            }
            Inline::EmbeddedObject(object) => {
                let start=*source_offset;*source_offset+=INLINE_OBJECT_MARKER_LEN;
                let width=(object.width_mm*MM_TO_PT).max(1.0);
                let height=(object.height_mm*MM_TO_PT).max(1.0);
                let commands=object.preview.as_ref().and_then(|id|resources.get(id))
                    .and_then(|r|raster_preview_commands(r,width,height,&model::ImageEffects::default()))
                    .unwrap_or_else(||image_frame_commands(width,height));
                lines.last_mut().expect("hard line exists").tokens.push(InlineToken{
                    start,end:*source_offset,whitespace:false,width,ascent:height,descent:0.0,bidi_level:0,
                    kind:InlineTokenKind::Image{width,height,commands,hit_regions:Vec::new()},
                });
            }
            Inline::TextBox(boxed) => {
                let start = *source_offset;
                *source_offset += INLINE_OBJECT_MARKER_LEN;
                // Anchored boxes are painted by the floating page pass. They
                // retain their source marker but consume no text-flow width.
                if boxed.anchor.is_some() && boxed.wrap != model::ImageWrap::Inline { continue; }
                let width = (boxed.width_mm * MM_TO_PT).max(4.0);
                let height = (boxed.height_mm * MM_TO_PT).max(4.0);
                let (commands, hit_regions) = prepare_text_box(boxed, styles, ctx, resources, field_context)?;
                lines.last_mut().expect("hard line exists").tokens.push(InlineToken {
                    start, end: *source_offset, whitespace: false, width, ascent: height, descent: 0.0, bidi_level: 0,
                    kind: InlineTokenKind::Image { width, height, commands, hit_regions },
                });
            }
            Inline::Field(field) => {
                let text=field.display_text.clone().unwrap_or_else(||resolve_field_text(&field.kind,field_context));
                let text=if text.is_empty(){" ".to_owned()}else{text.replace(['\r','\n']," ")};
                let start=*source_offset;*source_offset+=INLINE_OBJECT_MARKER_LEN;
                let mut properties=base_char.clone();if let Some(color)=&field.color { properties.color=Some(color.clone()); }let base_size=properties.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
                let lift=if matches!(&field.kind,FieldKind::FootnoteReference{..}){properties.font_size_pt=Some(base_size*0.72);base_size*0.35}else{0.0};
                let mut pieces=Vec::new();
                for span in properties.font_spans(&text){let mut regional=properties.clone();regional.set_font_family(span.family.map(str::to_owned));let text=text[span.range].to_owned();let shaped=shape_token(&text,&regional,&ctx.fonts)?;pieces.push(FieldFontPiece{text,properties:regional,shaped});}
                let width=pieces.iter().map(|piece|piece.shaped.advance_pt).sum();let size=properties.font_size_pt.unwrap_or(base_size);
                lines.last_mut().expect("hard line exists").tokens.push(InlineToken{start,end:*source_offset,whitespace:false,width,
                    ascent:size+lift,descent:size*0.2,bidi_level:0,kind:InlineTokenKind::Field{pieces,properties,lift}});
            }
            Inline::PaginationBreak { break_kind, .. } => {
                *source_offset += 1;
                lines.last_mut().expect("hard line exists").break_after = Some(*break_kind);
                lines.push(HardLine { break_after: None, start_offset: *source_offset, tokens: Vec::new() });
            }
            Inline::LineBreak { .. } => {
                *source_offset += 1;
                lines.push(HardLine { break_after: None, start_offset: *source_offset, tokens: Vec::new() });
            }
            Inline::Tab { .. } => {
                let start = *source_offset;
                *source_offset += 1;
                let props = base_char.clone();
                let shaped = shape_token("    ", &props, &ctx.fonts)?;
                let size = props.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
                lines.last_mut().expect("hard line exists").tokens.push(InlineToken {
                    start,
                    end: *source_offset,
                    whitespace: true,
                    width: shaped.advance_pt,
                    ascent: size,
                    descent: size * 0.2,
                    bidi_level: 0,
                    kind: InlineTokenKind::Space,
                });
            }
            Inline::Hyperlink(link) => {
                collect_inline_slice(&link.content, paragraph, styles, ctx, resources, base_char, source_offset, lines, field_context)?;
            }
        }
    }
    Ok(())
}

fn normalized_shaping_properties(mut props:CharacterProperties)->CharacterProperties {
    props.font_family=Some(props.font_family.unwrap_or_else(|| DEFAULT_FONT_FAMILY.into()));
    let mut size=props.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
    if style_flag(&props.visual_effects,"fnt_superscript") || style_flag(&props.visual_effects,"fnt_subscript") {size*=0.68;}
    props.font_size_pt=Some(size);
    props.bold=Some(props.bold.unwrap_or(false));props.italic=Some(props.italic.unwrap_or(false));
    props.underline=Some(props.underline.unwrap_or(false));
    props.color=Some(props.color.unwrap_or_else(|| "#000000".into()).to_ascii_lowercase());
    props.highlight=props.highlight.filter(|v| !v.eq_ignore_ascii_case("none"))
        .map(|v| v.to_ascii_lowercase());
    props.language=props.language.map(|v| v.trim().to_ascii_lowercase()).filter(|v| !v.is_empty());
    let mut typography=props.typography.unwrap_or_default();
    let threshold=style_number(&props.visual_effects,"fnt_kerning_from",0.0);
    if props.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT)<threshold {
        typography.kerning=false;
        typography.kerning_mode=model::KerningMode::Off;
    }
    props.typography=Some(typography);props
}

fn collect_text_tokens(
    text: &str,
    properties: &CharacterProperties,
    source_offset: &mut usize,
    lines: &mut Vec<HardLine>,
    fonts: &FontCatalog,
) -> Result<(), LayoutError> {
    if text.is_empty() { return Ok(()); }
    // Word's Symbol/Wingdings bullets show as boxes without those fonts.
    let text = &*model::replace_symbol_font_chars(text);

    // Newlines from imported/plain text are treated as hard line breaks even
    // though normal editor Enter operations use semantic paragraph splitting.
    let mut segment_start = 0usize;
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            if i > segment_start {
                collect_text_segment(&text[segment_start..i], properties, source_offset, lines, fonts)?;
            }
            *source_offset += ch.len_utf8();
            lines.push(HardLine { break_after: None, start_offset: *source_offset, tokens: Vec::new() });
            segment_start = i + ch.len_utf8();
        }
    }
    if segment_start < text.len() {
        collect_text_segment(&text[segment_start..], properties, source_offset, lines, fonts)?;
    }
    Ok(())
}

fn collect_text_segment(
    text: &str,
    properties: &CharacterProperties,
    source_offset: &mut usize,
    lines: &mut Vec<HardLine>,
    fonts: &FontCatalog,
) -> Result<(), LayoutError> {
    for span in properties.font_spans(text) {
        let mut resolved = properties.clone();
        resolved.set_font_family(span.family.map(str::to_owned));
        collect_uniform_font_segment(&text[span.range], &resolved, source_offset, lines, fonts)?;
    }
    Ok(())
}

fn collect_uniform_font_segment(
    text: &str,
    properties: &CharacterProperties,
    source_offset: &mut usize,
    lines: &mut Vec<HardLine>,
    fonts: &FontCatalog,
) -> Result<(), LayoutError> {
    let mut start = 0usize;
    let mut state = text.chars().next().map(char::is_whitespace).unwrap_or(false);
    for (i, ch) in text.char_indices() {
        let ws = ch.is_whitespace();
        if i > start && ws != state {
            push_text_token(&text[start..i], state, properties, source_offset, lines, fonts)?;
            start = i;
            state = ws;
        }
    }
    if start < text.len() {
        push_text_token(&text[start..], state, properties, source_offset, lines, fonts)?;
    }
    Ok(())
}

fn push_text_token(
    text: &str,
    whitespace: bool,
    properties: &CharacterProperties,
    source_offset: &mut usize,
    lines: &mut Vec<HardLine>,
    fonts: &FontCatalog,
) -> Result<(), LayoutError> {
    let mut displayed=text.to_owned();
    if properties.visual_effects.get("fnt_all_caps").is_some_and(|value|value=="true") {
        // ASCII case changes preserve the source byte offsets used by the editor.
        displayed.make_ascii_uppercase();
    }
    let shaped = shape_token(&displayed, properties, fonts)?;
    let start = *source_offset;
    *source_offset += text.len();
    let size = properties.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
    lines.last_mut().expect("hard line exists").tokens.push(InlineToken {
        start,
        end: *source_offset,
        whitespace,
        width: shaped.advance_pt,
        ascent: size,
        descent: size * 0.22,
        bidi_level: 0,
        kind: InlineTokenKind::Text { text: displayed, properties: properties.clone(), shaped },
    });
    Ok(())
}

fn shape_token(text: &str, properties: &CharacterProperties, fonts: &FontCatalog) -> Result<ShapedRun, LayoutError> {
    shape_token_directed(text, properties, fonts, TextDirection::LeftToRight)
}

fn shape_token_directed(
    text: &str,
    properties: &CharacterProperties,
    fonts: &FontCatalog,
    direction: TextDirection,
) -> Result<ShapedRun, LayoutError> {
    let resolved = resolve_font(fonts, properties, text)?;
    let options = ShapeOptions::from_character_properties_with_direction(properties, direction);
    let mut shaped=fonts.shape(resolved, text, &options)
        .ok_or_else(|| LayoutError::FontDataUnavailable(properties.font_family.clone().unwrap_or_else(|| DEFAULT_FONT_FAMILY.into())))?
        .map_err(LayoutError::from)?;
    let scale=horizontal_scale(properties);
    if (scale-1.0).abs()>0.001 {
        for glyph in &mut shaped.glyphs {glyph.x_advance_pt*=scale;glyph.x_offset_pt*=scale;}
        shaped.advance_pt*=scale;
    }
    let spacing=letter_spacing(properties);
    if spacing.abs()>0.001 && shaped.glyphs.len()>1 {
        for i in 0..shaped.glyphs.len()-1 {
            if shaped.glyphs[i].cluster!=shaped.glyphs[i+1].cluster {
                let amount=spacing.max(-shaped.glyphs[i].x_advance_pt.abs()*0.8);
                shaped.glyphs[i].x_advance_pt+=amount;
                shaped.advance_pt+=amount;
            }
        }
    }
    Ok(shaped)
}

fn horizontal_scale(properties:&CharacterProperties)->f32 {
    properties.visual_effects.get("fnt_scale")
        .and_then(|value|value.trim_end_matches('%').parse::<f32>().ok())
        .map(|percent|(percent/100.0).clamp(0.25,4.0)).unwrap_or(1.0)
}

fn letter_spacing(properties:&CharacterProperties)->f32 {
    let amount=style_number(&properties.visual_effects,"fnt_spacing_by",0.0)
        .clamp(0.0,properties.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT));
    match properties.visual_effects.get("fnt_spacing").map(String::as_str) {
        Some("Expanded")=>amount,Some("Condensed")=>-amount,_=>0.0,
    }
}

fn apply_hyphenation(lines: &mut [HardLine], fonts: &FontCatalog) -> Result<(), LayoutError> {
    for line in lines {
        let mut expanded=Vec::with_capacity(line.tokens.len());
        for token in line.tokens.drain(..) {
            let InlineTokenKind::Text {text,properties,shaped}= &token.kind else {expanded.push(token);continue;};
            if token.whitespace {expanded.push(token);continue;}
            let points=hyphenation_points(text,properties.language.as_deref());
            // Preserve complete GSUB clusters. Do not put an optional break
            // between f and i inside one fi/ffi glyph (or inside a mark cluster).
            let points:Vec<usize>=points.into_iter().filter(|p| *p>0 && *p<text.len()
                && text.is_char_boundary(*p) && shaped.glyphs.iter().any(|g| g.cluster==*p)).collect();
            if points.is_empty() {expanded.push(token);continue;}
            let hyphen=shape_token_directed("-",properties,fonts,direction_for_level(token.bidi_level))?;
            let mut previous=0;
            for point in points.into_iter().chain(std::iter::once(text.len())) {
                if point<=previous {continue;}
                // Slice the already-shaped word, so the advances used by the
                // line breaker add up to the intact word, including kerning.
                let piece=shaped_slice(shaped,previous,point);
                expanded.push(InlineToken {start:token.start+previous,end:token.start+point,
                    whitespace:false,width:piece.advance_pt,ascent:token.ascent,descent:token.descent,bidi_level:token.bidi_level,
                    kind:InlineTokenKind::Text {text:text[previous..point].into(),properties:properties.clone(),shaped:piece}});
                if point<text.len() {
                    expanded.push(InlineToken {start:token.start+point,end:token.start+point,whitespace:false,
                        width:hyphen.advance_pt,ascent:token.ascent,descent:token.descent,bidi_level:token.bidi_level,
                        kind:InlineTokenKind::HyphenOpportunity {properties:properties.clone(),shaped:hyphen.clone()}});
                }
                previous=point;
            }
        }
        line.tokens=expanded;
    }
    Ok(())
}
fn shaped_slice(shaped:&ShapedRun,start:usize,end:usize)->ShapedRun {
    let glyphs:Vec<_>=shaped.glyphs.iter().filter(|g| (start..end).contains(&g.cluster)).map(|g| {
        let mut g=g.clone();g.cluster-=start;g
    }).collect();
    let advance_pt=glyphs.iter().map(|g| g.x_advance_pt).sum();
    ShapedRun {glyphs,advance_pt,units_per_em:shaped.units_per_em}
}
/// Rejoin unused discretionary breaks and shape each final continuous run once.
/// A genuine line break/format change/inline object still terminates shaping.
fn reshape_line_text(tokens:&[InlineToken],fonts:&FontCatalog)->Result<Vec<InlineToken>,LayoutError> {
    let mut out:Vec<InlineToken>=Vec::new();
    for (index,token) in tokens.iter().enumerate() {
        if matches!(&token.kind,InlineTokenKind::HyphenOpportunity {..}) && index+1<tokens.len() {continue;}
        if let Some(previous)=out.last_mut() {
            if previous.start==token.start && previous.end==token.end && previous.whitespace==token.whitespace {
                if let (InlineTokenKind::Field{pieces:a,properties:ap,lift:al},InlineTokenKind::Field{pieces:b,properties:bp,lift:bl})=(&mut previous.kind,&token.kind) {
                    if ap==bp && al==bl {a.extend(b.iter().cloned());continue;}
                }
            }
            if previous.end==token.start && previous.whitespace==token.whitespace && previous.bidi_level==token.bidi_level {
                if let (InlineTokenKind::Text {text:a,properties:ap,..},InlineTokenKind::Text {text:b,properties:bp,..})=(&mut previous.kind,&token.kind) {
                    if ap==bp {a.push_str(b);previous.end=token.end;previous.ascent=previous.ascent.max(token.ascent);previous.descent=previous.descent.max(token.descent);continue;}
                }
            }
        }
        out.push(token.clone());
    }
    for token in &mut out {
        if let InlineTokenKind::Text {text,properties,shaped}=&mut token.kind {
            *shaped=shape_token_directed(text,properties,fonts,direction_for_level(token.bidi_level))?;
            token.width=shaped.advance_pt;
        } else { field_wrapping::reshape(token,fonts)?; }
    }
    kern_cross_run_boundaries(&mut out,fonts)?;
    Ok(out)
}

/// A format change inside a word creates two shaping runs. Optical kerning
/// still needs to inspect the two outlines meeting at that boundary.
fn kern_cross_run_boundaries(tokens:&mut [InlineToken],fonts:&FontCatalog)->Result<(),LayoutError> {
    for index in 0..tokens.len().saturating_sub(1) {
        let (left_tokens,right_tokens)=tokens.split_at_mut(index+1);
        let left=&mut left_tokens[index];let right=&right_tokens[0];
        if left.end!=right.start || left.whitespace || right.whitespace
            || left.bidi_level%2!=0 || right.bidi_level%2!=0 {continue;}
        let (InlineTokenKind::Text{text:left_text,properties:left_props,shaped:left_shape},
            InlineTokenKind::Text{text:right_text,properties:right_props,shaped:right_shape})=
            (&mut left.kind,&right.kind) else {continue;};
        let optical=|p:&CharacterProperties|p.typography.as_ref().is_some_and(|t|
            t.kerning && t.kerning_mode==model::KerningMode::Optical);
        if !optical(left_props)||!optical(right_props){continue;}
        let (Some(left_char),Some(right_char),Some(left_glyph),Some(right_glyph))=
            (left_text.chars().last(),right_text.chars().next(),left_shape.glyphs.last(),right_shape.glyphs.first())
            else {continue;};
        let left_font=resolve_font(fonts,left_props,left_text)?;
        let right_font=resolve_font(fonts,right_props,right_text)?;
        let left_size=left_props.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
        let right_size=right_props.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
        let same_font=left_font==right_font && (left_size-right_size).abs()<0.001
            && left_props.typography==right_props.typography;
        let delta=fonts.with_face(left_font,|left_face|
            fonts.with_face(right_font,|right_face| {
                let font_kern=if same_font {left_props.typography.as_ref().and_then(|typography|
                    typography::font_pair_kerning(left_face,left_char,right_char,
                        left_size,typography))} else {None};
                typography::optical_pair_adjustment(left_face,left_glyph,left_size,
                    right_face,right_glyph,right_size,left_char,right_char,font_kern)
            }))
            .flatten().flatten();
        if let Some(delta)=delta {
            if let Some(glyph)=left_shape.glyphs.last_mut(){glyph.x_advance_pt+=delta;}
            left_shape.advance_pt+=delta;left.width+=delta;
        }
    }
    Ok(())
}

fn apply_bidi_levels(lines: &mut [HardLine], paragraph_text: &str, fonts: &FontCatalog) -> Result<(), LayoutError> {
    let levels = bidi_levels(paragraph_text);
    for line in lines {
        let mut resolved = Vec::with_capacity(line.tokens.len());
        for token in line.tokens.drain(..) {
            let token_level = level_for_token(&levels, &token);
            let InlineTokenKind::Text { text, properties, .. } = &token.kind else {
                let mut token = token;
                token.bidi_level = token_level;
                resolved.push(token);
                continue;
            };

            // A whitespace-delimited token may still contain more than one
            // directional run (for example Latin immediately followed by
            // Hebrew). Shape each resolved embedding-level run independently;
            // assigning the first character's level to the whole token would
            // reverse/mirror mixed-script text incorrectly.
            let mut run_start = 0usize;
            let mut run_level = explicit_run_level(level_at_source_offset(&levels, token.start), properties);
            for (local_offset, _) in text.char_indices().skip(1) {
                let level = explicit_run_level(level_at_source_offset(&levels, token.start + local_offset), properties);
                if level != run_level {
                    push_bidi_text_slice(
                        &mut resolved,
                        &token,
                        text,
                        properties,
                        run_start,
                        local_offset,
                        run_level,
                        fonts,
                    )?;
                    run_start = local_offset;
                    run_level = level;
                }
            }
            push_bidi_text_slice(
                &mut resolved,
                &token,
                text,
                properties,
                run_start,
                text.len(),
                run_level,
                fonts,
            )?;
        }
        line.tokens = resolved;
        kern_cross_run_boundaries(&mut line.tokens,fonts)?;
    }
    Ok(())
}

fn explicit_run_level(level: u8, properties: &CharacterProperties) -> u8 {
    match properties.right_to_left {
        Some(true) => level | 1,
        Some(false) => level & !1,
        None => level,
    }
}

fn level_at_source_offset(levels: &[u8], byte_offset: usize) -> u8 {
    if levels.is_empty() { 0 } else { levels[byte_offset.min(levels.len().saturating_sub(1))] }
}

fn level_for_token(levels: &[u8], token: &InlineToken) -> u8 {
    if token.start == token.end && token.start > 0 {
        // A discretionary hyphen belongs to the preceding syllable. UAX #9
        // levels are stored per UTF-8 byte, so the previous byte has the same
        // resolved level as the preceding scalar even when it is multibyte.
        level_at_source_offset(levels, token.start - 1)
    } else {
        level_at_source_offset(levels, token.start)
    }
}

#[allow(clippy::too_many_arguments)]
fn push_bidi_text_slice(
    output: &mut Vec<InlineToken>,
    original: &InlineToken,
    full_text: &str,
    properties: &CharacterProperties,
    local_start: usize,
    local_end: usize,
    level: u8,
    fonts: &FontCatalog,
) -> Result<(), LayoutError> {
    if local_start >= local_end { return Ok(()); }
    if local_start==0 && local_end==full_text.len() && level%2==original.bidi_level%2 {
        let mut token=original.clone();token.bidi_level=level;output.push(token);return Ok(());
    }
    let text = &full_text[local_start..local_end];
    let shaped = shape_token_directed(text, properties, fonts, direction_for_level(level))?;
    output.push(InlineToken {
        start: original.start + local_start,
        end: original.start + local_end,
        whitespace: original.whitespace,
        width: shaped.advance_pt,
        ascent: original.ascent,
        descent: original.descent,
        bidi_level: level,
        kind: InlineTokenKind::Text {
            text: text.to_string(),
            properties: properties.clone(),
            shaped,
        },
    });
    Ok(())
}

fn paragraph_plain_text(paragraph: &Paragraph) -> String {
    fn append(content: &[Inline], output: &mut String) {
        for inline in content {
            match inline {
                Inline::Text(run) => output.push_str(&run.text),
                Inline::Equation(_) | Inline::Image(_) | Inline::TextBox(_) | Inline::EmbeddedObject(_) | Inline::Field(_) => output.push('\u{FFFC}'),
                Inline::LineBreak { .. } => output.push('\n'), Inline::PaginationBreak { break_kind, .. } => output.push(break_kind.marker()),
                Inline::Tab { .. } => output.push('\t'),
                Inline::Hyperlink(link) => append(&link.content, output),
            }
        }
    }
    let mut output = String::new();
    append(&paragraph.content, &mut output);
    output
}

fn token_advance(token: &InlineToken, terminal: bool, glue_ratio: f32, glyph_scale: f32) -> f32 {
    match &token.kind {
        InlineTokenKind::HyphenOpportunity { .. } => if terminal { token.width } else { 0.0 },
        _ if token.whitespace => {
            if glue_ratio >= 0.0 {
                token.width * (1.0 + DEFAULT_SPACE_STRETCH * glue_ratio)
            } else {
                token.width * (1.0 + DEFAULT_SPACE_SHRINK * glue_ratio)
            }
        }
        InlineTokenKind::Text { .. } => token.width * glyph_scale,
        _ => token.width,
    }
}

fn style_flag(format: &BTreeMap<String, String>, key: &str) -> bool {
    format.get(key).is_some_and(|value| value == "true")
}

fn style_number(format: &BTreeMap<String, String>, key: &str, default: f32) -> f32 {
    format.get(key).and_then(|value| value.parse::<f32>().ok()).filter(|value| value.is_finite()).unwrap_or(default)
}

/// Decorative copies carry no source range, so they do not become extra text
/// in selection, copy/paste, or accessible output.
fn effect_copy(command: &DrawCommand, dx: f32, dy: f32, color: Color) -> Option<DrawCommand> {
    let DrawCommand::TextRun(_) = command else { return None; };
    let mut copy = command.translated(dx, dy);
    if let DrawCommand::TextRun(run) = &mut copy {
        let run = Arc::make_mut(run);
        run.source = None;
        run.actual_text = None;
        run.fill = Paint::Solid(color);
        for glyph in &mut run.glyphs { glyph.cluster = None; }
    }
    Some(copy)
}

fn append_text_effects(commands: &mut Vec<DrawCommand>, command: &DrawCommand, format: &BTreeMap<String, String>) {
    if format.get("fx_bevel").is_some_and(|value|value!="None") {
        let depth=style_number(format,"fx_depth",1.0).clamp(0.5,6.0);
        let color=match format.get("fx_material").map(String::as_str) {
            Some("Metal"|"Plastic")=>Color::rgba(0.30,0.36,0.48,0.55),
            Some("Warm matte")=>Color::rgba(0.52,0.38,0.26,0.45),
            _=>Color::rgba(0.18,0.23,0.30,0.42),
        };
        for step in (1..=3).rev() {
            let offset=depth*step as f32/3.0;
            if let Some(copy)=effect_copy(command,offset,offset,color) {commands.push(copy);}
        }
    }
    let shadow = format.get("fx_shadow").map(String::as_str).unwrap_or("No shadow");
    if shadow != "No shadow" && !shadow.is_empty() {
        let mut color = format.get("fx_shadow_color").and_then(|v| parse_color(Some(v))).unwrap_or(Color::BLACK);
        color.a = 1.0 - style_number(format, "fx_shadow_transparency", 40.0).clamp(0.0, 100.0) / 100.0;
        let angle = style_number(format, "fx_shadow_angle", 45.0).to_radians();
        let distance = style_number(format, "fx_shadow_distance", 2.0).clamp(0.0, 24.0);
        if let Some(copy) = effect_copy(command, angle.cos() * distance, angle.sin() * distance, color) { commands.push(copy); }
    }
    let glow = format.get("fx_glow").map(String::as_str).unwrap_or("No glow");
    if glow != "No glow" && !glow.is_empty() {
        let mut color = format.get("fx_glow_color").and_then(|v| parse_color(Some(v))).unwrap_or(Color::rgb8(58, 104, 194));
        color.a = (1.0 - style_number(format, "fx_glow_transparency", 55.0).clamp(0.0, 100.0) / 100.0) * 0.35;
        let radius = style_number(format, "fx_glow_size", 2.0).clamp(0.5, 6.0);
        for (dx, dy) in [(-radius, 0.0), (radius, 0.0), (0.0, -radius), (0.0, radius)] {
            if let Some(copy) = effect_copy(command, dx, dy, color) { commands.push(copy); }
        }
    }
    if style_flag(format, "fx_line_solid") || style_flag(format, "fx_line_gradient") {
        let mut color = format.get("fx_line_color").and_then(|v| parse_color(Some(v))).unwrap_or(Color::BLACK);
        color.a = 1.0 - style_number(format, "fx_line_transparency", 0.0).clamp(0.0, 100.0) / 100.0;
        let radius = style_number(format, "fx_line_width", 0.75).clamp(0.25, 3.0);
        for (dx, dy) in [(-radius, 0.0), (radius, 0.0), (0.0, -radius), (0.0, radius)] {
            if let Some(copy) = effect_copy(command, dx, dy, color) { commands.push(copy); }
        }
    }
    let reflection=format.get("fx_reflection").map(String::as_str).unwrap_or("No reflection");
    if reflection!="No reflection" {
        let distance=style_number(format,"fx_reflection_distance",if reflection.contains("4 pt") {4.0}else{0.0}).clamp(0.0,24.0);
        let size=style_number(format,"fx_reflection_size",if reflection.starts_with("Half") {50.0}else{100.0}).clamp(5.0,100.0)/100.0;
        let alpha=(1.0-style_number(format,"fx_reflection_transparency",65.0).clamp(0.0,100.0)/100.0)*0.5;
        if let DrawCommand::TextRun(original)=command {
            if let Some(mut copy)=effect_copy(command,0.0,0.0,Color::rgba(0.25,0.30,0.40,alpha)) {
                if let DrawCommand::TextRun(run)=&mut copy {
                    let run=Arc::make_mut(run);
                    let mirror=Affine2D{sx:1.0,kx:0.0,tx:0.0,ky:0.0,sy:-1.0,ty:distance};
                    for glyph in &mut run.glyphs {glyph.transform=glyph.transform.then(mirror);}
                    run.bounds=Rect::new(original.bounds.x,distance,original.bounds.width,original.font_size_pt*size);
                }
                commands.push(copy.clipped(Rect::new(original.bounds.x,distance,original.bounds.width,original.font_size_pt*size)));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn vectorize_mixed_line(
    tokens: &[InlineToken],
    node_id: &str,
    visual_line: usize,
    line_width: f32,
    glue_ratio: f32,
    requested_glyph_scale: f32,
    alignment: ParagraphAlignment,
    base_line_height: f32,
    first_line_indent: f32,
    hanging_punctuation: bool,
    format: &BTreeMap<String, String>,
    fonts: &FontCatalog,
) -> Result<PreparedLine, LayoutError> {
    let has_fixed_object = tokens.iter().any(|t| matches!(t.kind, InlineTokenKind::Equation { .. } | InlineTokenKind::Image { .. } | InlineTokenKind::Field { .. }));
    // Never horizontally distort math. On lines containing inline math, prefer
    // ordinary inter-word adjustment to pdfTeX-style glyph expansion.
    let glyph_scale = if has_fixed_object { 1.0 } else { requested_glyph_scale };
    let mut levels: Vec<u8> = tokens.iter().map(|token| token.bidi_level).collect();
    // UAX #9 L1 resets trailing whitespace to the paragraph base level after
    // line wrapping. The minimum resolved level is the paragraph base for the
    // normal 0/1 base cases, so wrapped spaces remain on the visual line edge
    // instead of being pulled through an opposite-direction run.
    let base_level = levels.iter().copied().min().unwrap_or(0);
    for index in (0..tokens.len()).rev() {
        if tokens[index].whitespace { levels[index] = base_level; } else { break; }
    }
    let visual_indices = visual_order(&levels);

    let line_advance = tokens.iter().enumerate().map(|(index, token)| {
        let terminal_hyphen = index + 1 == tokens.len()
            && matches!(token.kind, InlineTokenKind::HyphenOpportunity { .. });
        token_advance(token, terminal_hyphen, glue_ratio, glyph_scale)
    }).sum::<f32>();

    let visual_char = |token: &InlineToken, first: bool| -> Option<char> {
        let InlineTokenKind::Text { text, .. } = &token.kind else { return None; };
        if token.bidi_level % 2 == 1 {
            if first { text.chars().rev().find(|c| !c.is_whitespace()) }
            else { text.chars().find(|c| !c.is_whitespace()) }
        } else if first {
            text.chars().find(|c| !c.is_whitespace())
        } else {
            text.chars().rev().find(|c| !c.is_whitespace())
        }
    };
    let first_char = visual_indices.iter().find_map(|&index| visual_char(&tokens[index], true));
    let last_char = visual_indices.iter().rev().find_map(|&index| visual_char(&tokens[index], false));
    let representative_size = tokens.iter().find_map(|t| match &t.kind {
        InlineTokenKind::Text { properties, .. } | InlineTokenKind::HyphenOpportunity { properties, .. } => properties.font_size_pt,
        _ => None,
    }).unwrap_or(DEFAULT_FONT_SIZE_PT);
    let edge_advance = |token: &InlineToken, first: bool| -> Option<f32> {
        let InlineTokenKind::Text { shaped, .. } = &token.kind else { return None; };
        let glyph = if (token.bidi_level % 2 == 1) == first {
            shaped.glyphs.last()
        } else {
            shaped.glyphs.first()
        }?;
        Some(glyph.x_advance_pt.abs())
    };
    let first_advance = visual_indices.iter().find_map(|&index| {
        visual_char(&tokens[index], true).and_then(|_| edge_advance(&tokens[index], true))
    });
    let last_advance = visual_indices.iter().rev().find_map(|&index| {
        visual_char(&tokens[index], false).and_then(|_| edge_advance(&tokens[index], false))
    });
    let margins = if hanging_punctuation {
        typography::optical_margins_with_advances(first_char, last_char,
            representative_size, first_advance, last_advance)
    } else { typography::OpticalMargins::default() };

    let origin_x = match alignment {
        ParagraphAlignment::Center => ((line_width - line_advance) * 0.5).max(0.0)
            + (margins.right_pt - margins.left_pt) * 0.5,
        ParagraphAlignment::Right => (line_width - line_advance).max(0.0) + margins.right_pt,
        ParagraphAlignment::Left | ParagraphAlignment::Justify => -margins.left_pt,
    } + first_line_indent;

    let ascent = tokens.iter().map(|t| t.ascent).fold(representative_size, f32::max);
    // Picture-only lines sit on the baseline and have no text descenders.
    let text_descent = if tokens.iter().all(|t| matches!(t.kind, InlineTokenKind::Image { .. })) { 0.0 } else { representative_size * 0.2 };
    let descent = tokens.iter().map(|t| t.descent).fold(text_descent, f32::max);
    let height = base_line_height.max(ascent + descent);
    let mut x = origin_x;
    let mut commands = Vec::new();
    let mut hit_regions = Vec::new();

    for &logical_index in &visual_indices {
        let token = &tokens[logical_index];
        let terminal_hyphen = logical_index + 1 == tokens.len()
            && matches!(token.kind, InlineTokenKind::HyphenOpportunity { .. });
        let advance = token_advance(token, terminal_hyphen, glue_ratio, glyph_scale);

        let effects=match &token.kind {
            InlineTokenKind::Text {properties,..} | InlineTokenKind::Field {properties,..}
            | InlineTokenKind::HyphenOpportunity {properties,..} => &properties.visual_effects,
            _ => format,
        };

        let highlight = match &token.kind {
            InlineTokenKind::Text {properties,..} | InlineTokenKind::Field {properties,..}
            | InlineTokenKind::HyphenOpportunity {properties,..} => properties.highlight.as_deref(),
            _ => None,
        };
        let highlight_color=parse_color(highlight).or_else(|| {
            (format.get("shd_apply").is_some_and(|value|value=="Text"))
                .then(||format.get("shd_effective_fill").and_then(|fill|shading_color(fill,format))).flatten()
        });
        if !style_flag(effects, "fnt_hidden") { if let Some(color) = highlight_color {
            if advance > 0.0 { commands.push(DrawCommand::FillRect {
                rect: Rect::new(x, -ascent, advance, height), paint: Paint::Solid(color),
            }); }
        } }
        match &token.kind {
            InlineTokenKind::Field{pieces,lift,..}=>{
                let mut field_x=x;let mut field_hit=None;
                let natural=pieces.iter().map(|piece|piece.shaped.advance_pt).sum::<f32>().max(f32::EPSILON);
                for piece in pieces {
                    let piece_advance=advance*piece.shaped.advance_pt/natural;
                    let (mut command,hit)=vectorize_text_token(&piece.text,token.start,token.end,node_id,visual_line,false,&piece.properties,&piece.shaped,field_x,1.0,piece_advance,height,fonts)?;
                    if let DrawCommand::TextRun(run)=&mut command{let run=Arc::make_mut(run);run.source=None;for glyph in &mut run.glyphs{glyph.cluster=None;}}
                    commands.push(command.translated(0.0,-*lift));if field_hit.is_none(){field_hit=Some(hit);}field_x+=piece_advance;
                }
                if let Some(mut hit)=field_hit {
                    hit.byte_range=token.start..token.end;hit.rect.x=x;hit.rect.width=advance;
                    hit.caret_stops=vec![CaretStop{byte_offset:token.start,x_pt:x},CaretStop{byte_offset:token.end,x_pt:x+advance}];hit.rect.y-=*lift;hit_regions.push(hit);
                }
            }
            InlineTokenKind::Text { text, properties, shaped } => {
                let (command, hit) = vectorize_text_token(
                    text,
                    token.start,
                    token.end,
                    node_id,
                    visual_line,
                    token.bidi_level % 2 == 1,
                    properties,
                    shaped,
                    x,
                    glyph_scale,
                    advance,
                    height,
                    fonts,
                )?;
                let size_pt = properties.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
                let position=style_number(effects,"fnt_position_by",0.0).clamp(0.0,size_pt*2.0);
                let shift = if style_flag(effects, "fnt_superscript") { -size_pt * 0.35 }
                    else if style_flag(effects, "fnt_subscript") { size_pt * 0.20 }
                    else {match effects.get("fnt_position").map(String::as_str) {
                        Some("Raised")=>-position,Some("Lowered")=>position,_=>0.0,
                    }};
                if !style_flag(effects, "fnt_hidden") {
                    let mut command = command.translated(0.0, shift);
                    if let DrawCommand::TextRun(run) = &mut command {
                        let run = Arc::make_mut(run);
                        if style_flag(effects, "fx_fill_none") { run.fill = Paint::Solid(Color::rgba(0.0, 0.0, 0.0, 0.0)); }
                        else if let Some(mut fill) = effects.get("fx_fill_color").and_then(|v| parse_color(Some(v))) {
                            fill.a = 1.0 - style_number(effects, "fx_fill_transparency", 0.0).clamp(0.0, 100.0) / 100.0;
                            run.fill = Paint::Solid(fill);
                        }
                    }
                    append_text_effects(&mut commands, &command, effects);
                    commands.push(command.clone());
                    if style_flag(effects,"fx_fill_gradient") && !style_flag(effects,"fx_fill_none") {
                        if let DrawCommand::TextRun(run)=&command {
                            let Paint::Solid(base)=run.fill;
                            let light=Color::rgba((base.r+1.0)*0.5,(base.g+1.0)*0.5,(base.b+1.0)*0.5,base.a);
                            if let Some(copy)=effect_copy(&command,0.0,0.0,light) {
                                commands.push(copy.clipped(Rect::new(x,-size_pt*0.5,advance.max(0.0),size_pt*0.65)));
                            }
                        }
                    }
                }
                let underline_style = effects.get("fnt_underline").map(String::as_str).unwrap_or("Single");
                if properties.underline.unwrap_or(false) && underline_style != "(none)" && !style_flag(effects, "fnt_hidden") {
                    let size_pt = properties.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
                    let thickness = (size_pt * 0.055).clamp(0.45, 1.35);
                    let offset = (size_pt * 0.095).max(thickness) + shift;
                    let ink = effects.get("fnt_underline_color").and_then(|v| parse_color(Some(v))).or_else(|| parse_color(properties.color.as_deref())).unwrap_or(Color::BLACK);
                    let step = if underline_style.contains("Dot") { thickness * 2.5 }
                        else if underline_style.contains("Dash") { thickness * 5.0 } else { advance.max(1.0) };
                    let dash = if underline_style.contains("Dot") { thickness }
                        else if underline_style.contains("Dash") { thickness * 3.0 } else { advance.max(0.0) };
                    let mut pos = 0.0;
                    while pos < advance { commands.push(DrawCommand::FillRect {
                        rect: Rect::new(x + pos, offset, dash.min(advance - pos).max(0.0), thickness), paint: Paint::Solid(ink),
                    }); pos += step.max(0.1); }
                    if underline_style.contains("Double") { commands.push(DrawCommand::FillRect {
                        rect: Rect::new(x, offset + thickness * 2.2, advance.max(0.0), thickness), paint: Paint::Solid(ink),
                    }); }
                }
                let mut hit = hit;
                hit.rect.y += shift;
                if (style_flag(effects, "fnt_strike") || style_flag(effects, "fnt_double_strike")) && !style_flag(effects, "fnt_hidden") {
                    let ink = parse_color(properties.color.as_deref()).unwrap_or(Color::BLACK);
                    let thickness = (size_pt * 0.055).clamp(0.45, 1.35);
                    let mid = -size_pt * 0.3 + shift;
                    let lines: &[f32] = if style_flag(effects, "fnt_double_strike") { &[-1.2, 1.2] } else { &[0.0] };
                    for offset in lines { commands.push(DrawCommand::FillRect {
                        rect: Rect::new(x, mid + offset * thickness, advance.max(0.0), thickness), paint: Paint::Solid(ink),
                    }); }
                }
                hit_regions.push(hit);
            }
            InlineTokenKind::Equation { fragment } => {
                let top = -fragment.baseline_pt;
                commands.extend(fragment.commands.iter().map(|c| c.translated(x, top)));
                hit_regions.push(TextHitRegion {
                    node_id: node_id.to_string(),
                    visual_line,
                    byte_range: token.start..token.end,
                    rect: Rect::new(x, top, fragment.size.width.max(1.0), fragment.size.height.max(1.0)),
                    caret_stops: vec![
                        CaretStop { byte_offset: token.start, x_pt: x },
                        CaretStop { byte_offset: token.end, x_pt: x + fragment.size.width },
                    ],
                });
            }
            InlineTokenKind::Image { width, height, commands: image_commands, hit_regions: inner_hits } => {
                let top = -*height;
                commands.extend(image_commands.iter().map(|c| c.translated(x, top)));
                hit_regions.extend(inner_hits.iter().map(|r| r.translated(x, top)));
                hit_regions.push(TextHitRegion {
                    node_id: node_id.to_string(),
                    visual_line,
                    byte_range: token.start..token.end,
                    rect: Rect::new(x, top, (*width).max(1.0), (*height).max(1.0)),
                    caret_stops: vec![
                        CaretStop { byte_offset: token.start, x_pt: x },
                        CaretStop { byte_offset: token.end, x_pt: x + *width },
                    ],
                });
            }
            InlineTokenKind::HyphenOpportunity { properties, shaped } => {
                if terminal_hyphen {
                    let (command, hit) = vectorize_text_token(
                        "-",
                        token.start,
                        token.end,
                        node_id,
                        visual_line,
                        token.bidi_level % 2 == 1,
                        properties,
                        shaped,
                        x,
                        1.0,
                        advance,
                        height,
                        fonts,
                    )?;
                    commands.push(command);
                    hit_regions.push(hit);
                }
            }
            InlineTokenKind::TabLeader(leader) => {
                let unit=match leader {model::TabLeader::Dots|model::TabLeader::MiddleDots=>3.5,model::TabLeader::Dashes=>6.0,_=>0.0};
                if unit>0.0 {let count=(advance/unit).floor().clamp(0.0,10000.0)as usize;for n in 1..count {
                    let width=if matches!(leader,model::TabLeader::Dashes){3.0}else{0.8};
                    let y=if matches!(leader,model::TabLeader::MiddleDots){-representative_size*0.3}else{0.0};
                    commands.push(DrawCommand::FillRect{rect:Rect::new(x+n as f32*unit,y,width,0.8),paint:Paint::Solid(Color::BLACK)});
                }} else if matches!(leader,model::TabLeader::Line|model::TabLeader::Heavy) {
                    commands.push(DrawCommand::FillRect{rect:Rect::new(x,1.0,advance,if matches!(leader,model::TabLeader::Heavy){1.5}else{0.6}),paint:Paint::Solid(Color::BLACK)});
                }
                hit_regions.push(TextHitRegion{node_id:node_id.into(),visual_line,byte_range:token.start..token.end,rect:Rect::new(x,-ascent,advance.max(1.0),height),caret_stops:vec![CaretStop{byte_offset:token.start,x_pt:x},CaretStop{byte_offset:token.end,x_pt:x+advance}]});
            }
            InlineTokenKind::Space => {
                hit_regions.push(TextHitRegion {
                    node_id: node_id.to_string(),
                    visual_line,
                    byte_range: token.start..token.end,
                    rect: Rect::new(x, -ascent, advance.max(1.0), height),
                    caret_stops: vec![
                        CaretStop { byte_offset: token.start, x_pt: x },
                        CaretStop { byte_offset: token.end, x_pt: x + advance },
                    ],
                });
            }
        }
        x += advance;
    }

    Ok(PreparedLine { pagination_break: None, commands, hit_regions, height, ascent })
}

#[allow(clippy::too_many_arguments)]
fn vectorize_text_token(
    text: &str,
    source_start: usize,
    source_end: usize,
    node_id: &str,
    visual_line: usize,
    rtl: bool,
    properties: &CharacterProperties,
    shaped: &ShapedRun,
    origin_x: f32,
    glyph_scale: f32,
    adjusted_advance: f32,
    line_height: f32,
    fonts: &FontCatalog,
) -> Result<(DrawCommand, TextHitRegion), LayoutError> {
    let size_pt = properties.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);
    let resolved = resolve_font(fonts, properties, text)?;
    let family = text.chars().next().and_then(|character| properties.font_family_for(character)).unwrap_or(DEFAULT_FONT_FAMILY).to_owned();

    fonts.with_face(resolved, |face| -> Result<(DrawCommand, TextHitRegion), LayoutError> {
        let natural = shaped.advance_pt.max(f32::EPSILON);
        let spacing_scale = adjusted_advance / natural;
        let visual_scale = if text.chars().all(char::is_whitespace) { spacing_scale } else { glyph_scale };
        let mut pen_x = origin_x;
        let mut glyphs = Vec::with_capacity(shaped.glyphs.len());
        let mut cluster_spans: BTreeMap<usize, (f32, f32)> = BTreeMap::new();
        let x_scale=visual_scale*horizontal_scale(properties);

        for glyph in &shaped.glyphs {
            let cluster = glyph.cluster.min(text.len());
            let start_x = pen_x;
            // Spaces and some formatting glyphs intentionally have no outline.
            // They still contribute advances/caret geometry.
            if let Some(outline) = scaled_outline(face, resolved, glyph.glyph_id, size_pt, x_scale) {
                glyphs.push(GlyphVector {
                    glyph_id: glyph.glyph_id,
                    outline,
                    transform: Affine2D::translation(pen_x + glyph.x_offset_pt * visual_scale, glyph.y_offset_pt),
                    cluster: Some(source_start + cluster),
                });
            }
            pen_x += glyph.x_advance_pt * visual_scale;
            cluster_spans.entry(cluster)
                .and_modify(|span| { span.0 = span.0.min(start_x); span.1 = span.1.max(pen_x); })
                .or_insert((start_x, pen_x));
        }

        // Make exact line-justification width authoritative even if shaping has
        // unusual zero-advance glyphs.
        let target_end = origin_x + adjusted_advance;
        if (pen_x - target_end).abs() > 0.01 { pen_x = target_end; }

        let mut stops = if source_start==source_end {
            // A displayed discretionary hyphen has no byte of its own.
            vec![CaretStop {byte_offset:source_start,x_pt:if rtl {origin_x} else {pen_x}}]
        } else { caret_stops_for_clusters(text, source_start, origin_x, pen_x, &cluster_spans, rtl) };
        if stops.is_empty() {
            stops.push(CaretStop { byte_offset: source_start, x_pt: origin_x });
            stops.push(CaretStop { byte_offset: source_end, x_pt: pen_x });
        }

        let bounds = Rect::new(origin_x, -size_pt, adjusted_advance.max(1.0), line_height);
        let source = TextSource { node_id: node_id.to_string(), byte_range: source_start..source_end };
        let run = VectorTextRun {
            actual_text: Some(text.to_owned()),
            source: Some(source.clone()),
            font_family: family.clone(),
            font_size_pt: size_pt,
            fill: Paint::Solid(parse_color(properties.color.as_deref()).unwrap_or(Color::BLACK)),
            glyphs,
            bounds,
            advance_pt: adjusted_advance,
        };
        let hit = TextHitRegion {
            node_id: node_id.to_string(),
            visual_line,
            byte_range: source.byte_range,
            rect: bounds,
            caret_stops: stops,
        };
        Ok((DrawCommand::TextRun(Arc::new(run)), hit))
    })
    .ok_or_else(|| LayoutError::FontDataUnavailable(family))?
}


fn caret_stops_for_clusters(
    text: &str,
    source_start: usize,
    origin_x: f32,
    pen_x: f32,
    cluster_spans: &BTreeMap<usize, (f32, f32)>,
    rtl: bool,
) -> Vec<CaretStop> {
    let clusters: Vec<usize> = cluster_spans.keys().copied().collect();
    let mut stops = Vec::new();
    if clusters.is_empty() {
        stops.push(CaretStop { byte_offset: source_start, x_pt: origin_x });
    } else {
        for (index, &cluster) in clusters.iter().enumerate() {
            let next_cluster = clusters.get(index + 1).copied().unwrap_or(text.len()).max(cluster);
            let (start_x, end_x) = cluster_spans[&cluster];
            let segment = &text[cluster..next_cluster];
            let graphemes: Vec<usize> = segment.grapheme_indices(true).map(|(offset, _)| offset).collect();
            let count = graphemes.len().max(1);
            for (ordinal, offset) in graphemes.into_iter().enumerate() {
                let ratio = ordinal as f32 / count as f32;
                stops.push(CaretStop {
                    byte_offset: source_start + cluster + offset,
                    x_pt: if rtl {
                        end_x - (end_x - start_x) * ratio
                    } else {
                        start_x + (end_x - start_x) * ratio
                    },
                });
            }
        }
    }
    stops.push(CaretStop {
        byte_offset: source_start + text.len(),
        x_pt: if rtl { origin_x } else { pen_x },
    });
    stops.sort_by_key(|s| s.byte_offset);
    stops.dedup_by_key(|s| s.byte_offset);
    stops
}

/// Every placement of a glyph at one size shares one outline. Extracting and
/// cloning the outline per placement multiplied scene memory, allocation and
/// comparison cost by the number of glyphs on the page.
fn scaled_outline(face: &typography::ShapingFace<'_>, font: typography::ResolvedFont, glyph_id: u32,
    size_pt: f32, x_scale: f32) -> Option<Arc<render::VectorPath>> {
    use std::sync::{Mutex, OnceLock};
    type Key = (typography::ResolvedFont, u32, u32, u32);
    static CACHE: OnceLock<Mutex<HashMap<Key, Option<Arc<render::VectorPath>>>>> = OnceLock::new();
    let key = (font, glyph_id, size_pt.to_bits(), x_scale.to_bits());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) { return found.clone(); }
    let outline = render::glyph_outline_from_face(face, glyph_id).ok()
        .map(|o| Arc::new(o.scale_to_points(size_pt).transformed(Affine2D::scale(x_scale, 1.0))));
    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
    if map.len() >= 200_000 { map.clear(); }
    map.insert(key, outline.clone());
    outline
}

fn resolve_font(
    fonts: &FontCatalog,
    properties: &CharacterProperties,
    text: &str,
) -> Result<typography::ResolvedFont, LayoutError> {
    let family = text.chars().next().and_then(|character| properties.font_family_for(character)).unwrap_or(DEFAULT_FONT_FAMILY);
    fonts.resolve_for_text(family, properties.bold.unwrap_or(false), properties.italic.unwrap_or(false), text)
        .ok_or_else(|| LayoutError::FontNotFound(family.to_string()))
}

fn empty_line(node_id: &str, offset: usize, visual_line: usize, height: f32, ascent: f32) -> PreparedLine {
    let bounds = Rect::new(0.0, -ascent, 1.0, height);
    PreparedLine {
        pagination_break: None,
        commands: vec![DrawCommand::TextRun(Arc::new(VectorTextRun {
            actual_text: Some(String::new()),
            source: Some(TextSource { node_id: node_id.to_string(), byte_range: offset..offset }),
            font_family: DEFAULT_FONT_FAMILY.into(),
            font_size_pt: ascent,
            fill: Paint::Solid(Color::BLACK),
            glyphs: Vec::new(),
            bounds,
            advance_pt: 0.0,
        }))],
        hit_regions: vec![TextHitRegion {
            node_id: node_id.to_string(),
            visual_line,
            byte_range: offset..offset,
            rect: bounds,
            caret_stops: vec![CaretStop { byte_offset: offset, x_pt: 0.0 }],
        }],
        height,
        ascent,
    }
}

fn new_page(width_pt: f32, height_pt: f32, background: bool) -> VectorPage {
    let mut page = VectorPage::new(Size::new(width_pt, height_pt));
    if background {
        page.commands.push(DrawCommand::FillRect {
            rect: Rect::new(0.0, 0.0, width_pt, height_pt),
            paint: Paint::Solid(Color::WHITE),
        });
    }
    page
}

fn parse_color(input: Option<&str>) -> Option<Color> {
    let s = input?.trim();
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 { return None; }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::rgb8(r, g, b))
}

fn shading_color(fill: &str, format: &BTreeMap<String, String>) -> Option<Color> {
    if fill == "none" {return None;}
    let base=parse_color(Some(fill))?;
    let pattern=format.get("shd_pattern").map(String::as_str).unwrap_or("Clear");
    if pattern == "Clear" {return Some(base);}
    let strength=if pattern.starts_with("Solid") {1.0}
        else {pattern.trim_end_matches('%').parse::<f32>().ok().map(|p|p/100.0).unwrap_or(0.0)};
    let ink=format.get("shd_pattern_color").and_then(|value|parse_color(Some(value))).unwrap_or(Color::BLACK);
    let t=strength.clamp(0.0,1.0);
    Some(Color::rgba(base.r*(1.0-t)+ink.r*t,base.g*(1.0-t)+ink.g*t,base.b*(1.0-t)+ink.b*t,1.0))
}

#[cfg(test)] #[path = "../tests/unit/lib.rs"] mod tests;

#[cfg(test)]
#[path="../../../test-support/ligature_font.rs"] mod ligature_fixture;
#[cfg(test)] #[path = "../tests/unit/lib/ligature_tests.rs"] mod ligature_tests;

// Tabs resolve in logical LTR coordinates. Mixed-direction tab layout is
// rejected until BiDi tab stops can be positioned against the visual order.
fn tab_gap_with_decimal(x:f32,following:f32,decimal_prefix:f32,props:&model::ParagraphProperties)->Result<(f32,model::TabLeader),LayoutError>{
    use model::{TabAlignment,TabLeader};
    let x = x + props.left_indent_mm.unwrap_or(0.0) * MM_TO_PT;
    if let Some(tab)=props.tabs.as_ref().and_then(|tabs|tabs.iter().filter(|t|t.alignment != TabAlignment::Bar && t.position_mm*MM_TO_PT>x+0.05).min_by(|a,b|a.position_mm.total_cmp(&b.position_mm))){
        let shift=match tab.alignment{TabAlignment::Left|TabAlignment::Bar=>0.0,TabAlignment::Center=>following*0.5,TabAlignment::Right=>following,TabAlignment::Decimal=>decimal_prefix};
        Ok(((tab.position_mm*MM_TO_PT-x-shift).max(0.0),tab.leader))
    }else{
        let step=props.default_tab_mm.unwrap_or(model::DEFAULT_TAB_INTERVAL_MM).clamp(model::MIN_TAB_INTERVAL_MM,model::MAX_TAB_INTERVAL_MM)*MM_TO_PT;
        Ok((((x/step).floor()+1.0)*step-x,TabLeader::None))
    }
}
fn tab_geometry(tokens:&[InlineToken],source:&str)->(Vec<bool>,Vec<f32>){
    let tabs:Vec<_>=tokens.iter().map(|t|source.get(t.start..t.end)==Some("\t")).collect();let mut suffix=vec![0.0;tokens.len()];let mut sum=0.0;
    for i in (0..tokens.len()).rev(){if tabs[i]{suffix[i]=sum;sum=0.0;}else{sum+=token_advance(&tokens[i],false,0.0,1.0);}}
    (tabs,suffix)
}
fn decimal_prefix(tokens:&[InlineToken],source:&str,from:usize)->f32 {
    let mut width=0.0;
    for token in tokens.iter().skip(from+1) {
        if source.get(token.start..token.end)==Some("\t") {break;}
        if let InlineTokenKind::Text{text,shaped,..}=&token.kind {
            if let Some(point)=text.find(['.',',']) {
                width+=shaped.glyphs.iter().filter(|glyph|glyph.cluster<point).map(|glyph|glyph.x_advance_pt).sum::<f32>();
                return width;
            }
        }
        width+=token.width;
    }
    width
}
fn align_tabs(tokens:&mut[InlineToken],source:&str,props:&model::ParagraphProperties,width:f32,indent:f32)->Result<(),LayoutError>{
    let(tabs,suffix)=tab_geometry(tokens,source);if !tabs.iter().any(|b|*b){return Ok(());}
    if props.contents_leader.is_none() && tokens.iter().any(|t|t.bidi_level%2==1){return Err(LayoutError::RenderPreparation("RTL/mixed-direction tab positioning is not implemented".into()));}
    let decimal=tabs.iter().enumerate().map(|(i,is_tab)|if *is_tab{decimal_prefix(tokens,source,i)}else{0.0}).collect::<Vec<_>>();
    let mut x=indent;let len=tokens.len();
    for(i,t)in tokens.iter_mut().enumerate(){if tabs[i]{let(gap,leader)=tab_gap_with_decimal(x,suffix[i],decimal[i],props)?;t.width=gap;t.whitespace=false;
        t.kind=InlineTokenKind::TabLeader(leader);}
        x+=token_advance(t,i+1==len,0.0,1.0);}
    if x>width+1.0{return Err(LayoutError::RenderPreparation("Tab-aligned text exceeds the available line after shaping; reduce its text or tab position".into()));}Ok(())
}

#[cfg(test)] #[path = "../tests/unit/lib/header_footer_tests.rs"] mod header_footer_tests;


#[cfg(test)] #[path = "../tests/unit/bounds_tests.rs"] mod bounds_tests;




#[cfg(test)] #[path = "../tests/unit/theme_fields.rs"] mod theme_fields;
