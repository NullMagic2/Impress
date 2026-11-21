/// Shared inset used to lay out and edit text inside table cells, in points.
pub const TABLE_TEXT_PADDING_PT: f32 = 4.0;

use crate::{CharacterProperties, Equation, NodeId, ParagraphProperties, ResourceId, StyleId, StyleSheet, STYLE_NORMAL};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DOCUMENT_SCHEMA_VERSION: u32 = 6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u32,
    pub metadata: DocumentMetadata,
    #[serde(default)] pub review: crate::ReviewState,
    #[serde(default)] pub compatibility: Vec<crate::CompatibilityIssue>,
    #[serde(default)] pub content_controls: BTreeMap<NodeId, crate::ContentControl>,
    #[serde(default)] pub custom_xml: BTreeMap<String, crate::CustomXmlData>,
    pub styles: StyleSheet,
    pub sections: Vec<Section>,
    pub resources: BTreeMap<ResourceId, Resource>,
    /// Native list/numbering definitions. DOCX numbering.xml maps here rather
    /// than leaking OOXML-specific identifiers into paragraph state.
    #[serde(default)]
    pub lists: BTreeMap<NodeId, ListDefinition>,
    /// OOXML/package parts Impress does not currently interpret. Adapters may
    /// round-trip these blobs without making them part of editor semantics.
    #[serde(default)]
    pub preserved_parts: BTreeMap<String, PreservedPackagePart>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            metadata: DocumentMetadata::default(),
            review: crate::ReviewState::default(),
            compatibility: Vec::new(),
            content_controls: BTreeMap::new(),
            custom_xml: BTreeMap::new(),
            styles: StyleSheet::default(),
            sections: vec![Section::default()],
            resources: BTreeMap::new(),
            lists: BTreeMap::new(),
            preserved_parts: BTreeMap::new(),
        }
    }
}

impl Document {
    /// Whether the document holds anything worth keeping: visible text, or any
    /// object, table or field, in the body or in a header or footer. Empty
    /// paragraphs, breaks and whitespace do not count.
    pub fn has_content(&self) -> bool {
        self.sections.iter().any(|section| {
            blocks_have_content(&section.blocks)
                || section.header_footers().any(|stream| blocks_have_content(&stream.stream.blocks))
        })
    }
}

fn blocks_have_content(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(paragraph) => paragraph.content.iter().any(|inline| match inline {
            Inline::Text(run) => !run.text.trim().is_empty(),
            Inline::LineBreak { .. } | Inline::PaginationBreak { .. } | Inline::Tab { .. } => false,
            Inline::Equation(equation) => equation.root.has_content(),
            _ => true,
        }),
        Block::PageBreak { .. } | Block::ColumnBreak { .. } => false,
        Block::Equation(equation) => equation.root.has_content(),
        _ => true,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub subject: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub id: NodeId,
    pub properties: SectionProperties,
    pub blocks: crate::Shared<Vec<Block>>,
    #[serde(default)]
    pub headers: HeaderFooterSet,
    #[serde(default)]
    pub footers: HeaderFooterSet,
}

impl Default for Section {
    fn default() -> Self {
        Self {
            id: NodeId::new(),
            properties: SectionProperties::default(),
            blocks: (Vec::new()).into(),
            headers: HeaderFooterSet::default(),
            footers: HeaderFooterSet::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionProperties {
    pub page_width_mm: f32,
    pub page_height_mm: f32,
    pub margin_top_mm: f32,
    pub margin_bottom_mm: f32,
    pub margin_left_mm: f32,
    pub margin_right_mm: f32,
    #[serde(default = "default_header_footer_mm")]
    pub header_distance_mm: f32,
    #[serde(default = "default_header_footer_mm")]
    pub footer_distance_mm: f32,
    #[serde(default = "default_column_count")]
    pub column_count: u16,
    #[serde(default = "default_column_gap_mm")]
    pub column_gap_mm: f32,
    /// Custom width and spacing of each column, as in Word's Columns dialog
    /// with "Equal column width" off. Empty means `column_count` equal
    /// columns `column_gap_mm` apart.
    #[serde(default)]
    pub columns: Vec<ColumnSpec>,
    /// A vertical rule between columns (Word's "Line between").
    #[serde(default)]
    pub column_separator: bool,
    #[serde(default)]
    pub different_first_page: bool,
    #[serde(default)]
    pub different_odd_even: bool,
    #[serde(default)]
    pub start_on: SectionStart,
    /// Number printed by PAGE fields on this section's first page.
    #[serde(default)]
    pub page_number_start: Option<u32>,
    #[serde(default)]
    pub page_number_format: Option<PageNumberFormat>,
}

const fn default_column_count() -> u16 { 1 }
const fn default_header_footer_mm() -> f32 { 12.7 }
const fn default_column_gap_mm() -> f32 { 10.0 }

impl Default for SectionProperties {
    fn default() -> Self {
        Self {
            page_width_mm: 210.0,
            page_height_mm: 297.0,
            margin_top_mm: 25.4,
            margin_bottom_mm: 25.4,
            margin_left_mm: 25.4,
            margin_right_mm: 25.4,
            header_distance_mm: default_header_footer_mm(),
            footer_distance_mm: default_header_footer_mm(),
            column_count: 1,
            column_gap_mm: default_column_gap_mm(),
            columns: Vec::new(),
            column_separator: false,
            different_first_page: false,
            different_odd_even: false,
            start_on: SectionStart::NewPage,
            page_number_start: None,
            page_number_format: None,
        }
    }
}

/// One column of a section with unequal columns.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnSpec {
    pub width_mm: f32,
    /// Space between this column and the next; ignored on the last column.
    pub spacing_mm: f32,
}

/// Where a column sits, measured from the left margin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnBand {
    pub left_mm: f32,
    pub width_mm: f32,
}

impl SectionProperties {
    /// Width between the left and right margins.
    pub fn text_width_mm(&self) -> f32 {
        (self.page_width_mm - self.margin_left_mm - self.margin_right_mm).max(1.0)
    }

    pub fn has_equal_columns(&self) -> bool {
        self.columns.len() != usize::from(self.column_count.max(1)) || self.columns.len() < 2
    }

    /// Every column's position and width. Custom widths are scaled together
    /// so the columns and the space between them fill the text width exactly.
    pub fn column_bands(&self) -> Vec<ColumnBand> {
        let text_width = self.text_width_mm();
        if self.has_equal_columns() {
            let count = usize::from(self.column_count.max(1));
            let gap = self.column_gap_mm.max(0.0);
            let width = ((text_width - gap * (count - 1) as f32) / count as f32).max(1.0);
            return (0..count).map(|index| ColumnBand { left_mm: index as f32 * (width + gap), width_mm: width }).collect();
        }
        let last = self.columns.len() - 1;
        let designed: f32 = self.columns.iter().enumerate()
            .map(|(index, column)| column.width_mm.max(0.0) + if index < last { column.spacing_mm.max(0.0) } else { 0.0 })
            .sum();
        let scale = if designed > 0.0 { text_width / designed } else { 1.0 };
        let mut left = 0.0;
        self.columns.iter().enumerate().map(|(index, column)| {
            let band = ColumnBand { left_mm: left, width_mm: (column.width_mm * scale).max(1.0) };
            left += band.width_mm + if index < last { column.spacing_mm.max(0.0) * scale } else { 0.0 };
            band
        }).collect()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionStart {
    Continuous,
    #[default]
    NewPage,
    OddPage,
    EvenPage,
    /// At the top of the next column on the same page (Word's "Start new
    /// column"), or on a new page when the page has no further column.
    NewColumn,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageNumberFormat {
    #[default] Decimal,
    UpperRoman,
    LowerRoman,
    UpperLetter,
    LowerLetter,
}

/// Which of a section's three header (or footer) streams is meant. The names
/// mirror Word's section-level variants and deliberately stay format-neutral.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeaderFooterVariant {
    #[default]
    Default,
    First,
    Even,
}

impl HeaderFooterVariant {
    pub const ALL: [Self; 3] = [Self::Default, Self::First, Self::Even];

    /// Compact index stored in page hit regions: 0 default, 1 first, 2 even.
    pub const fn index(self) -> u8 {
        match self {
            Self::Default => 0,
            Self::First => 1,
            Self::Even => 2,
        }
    }

    /// Inverse of [`Self::index`]; unknown values fall back to `Default`.
    pub const fn from_index(index: u8) -> Self {
        match index {
            1 => Self::First,
            2 => Self::Even,
            _ => Self::Default,
        }
    }

    /// The variant a section prints on its `local_page` (0-based), before
    /// falling back to the default stream when that variant is empty.
    pub fn for_page(properties: &SectionProperties, local_page: usize) -> Self {
        let is_even_page = (local_page + 1) % 2 == 0;
        if local_page == 0 && properties.different_first_page {
            Self::First
        } else if properties.different_odd_even && is_even_page {
            Self::Even
        } else {
            Self::Default
        }
    }

    /// Short name for status text and tooltips.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::First => "First page",
            Self::Even => "Even pages",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HeaderFooterSet {
    pub default: Option<HeaderFooter>,
    pub first: Option<HeaderFooter>,
    pub even: Option<HeaderFooter>,
}

impl HeaderFooterSet {
    pub fn get(&self, variant: HeaderFooterVariant) -> Option<&HeaderFooter> {
        match variant {
            HeaderFooterVariant::Default => self.default.as_ref(),
            HeaderFooterVariant::First => self.first.as_ref(),
            HeaderFooterVariant::Even => self.even.as_ref(),
        }
    }

    /// The storage slot itself, so callers can create or remove a stream.
    pub fn slot_mut(&mut self, variant: HeaderFooterVariant) -> &mut Option<HeaderFooter> {
        match variant {
            HeaderFooterVariant::Default => &mut self.default,
            HeaderFooterVariant::First => &mut self.first,
            HeaderFooterVariant::Even => &mut self.even,
        }
    }

    /// The stream printed on a section's `local_page`. An empty first-page or
    /// even-page variant falls back to the default stream, as in Word.
    pub fn for_page(&self, properties: &SectionProperties, local_page: usize) -> Option<&HeaderFooter> {
        let variant = HeaderFooterVariant::for_page(properties, local_page);
        self.get(variant).or(self.default.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = (HeaderFooterVariant, &HeaderFooter)> {
        [
            (HeaderFooterVariant::Default, self.default.as_ref()),
            (HeaderFooterVariant::First, self.first.as_ref()),
            (HeaderFooterVariant::Even, self.even.as_ref()),
        ].into_iter().filter_map(|(variant, stream)| stream.map(|stream| (variant, stream)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (HeaderFooterVariant, &mut HeaderFooter)> {
        [
            (HeaderFooterVariant::Default, self.default.as_mut()),
            (HeaderFooterVariant::First, self.first.as_mut()),
            (HeaderFooterVariant::Even, self.even.as_mut()),
        ].into_iter().filter_map(|(variant, stream)| stream.map(|stream| (variant, stream)))
    }
}

/// One existing header or footer stream of a section, with its address.
pub struct HeaderFooterRef<'a> {
    pub header: bool,
    pub variant: HeaderFooterVariant,
    pub stream: &'a HeaderFooter,
}

impl Section {
    pub fn header_footer_set(&self, header: bool) -> &HeaderFooterSet {
        if header { &self.headers } else { &self.footers }
    }

    pub fn header_footer_set_mut(&mut self, header: bool) -> &mut HeaderFooterSet {
        if header { &mut self.headers } else { &mut self.footers }
    }

    /// Every existing header stream, then every existing footer stream.
    pub fn header_footers(&self) -> impl Iterator<Item = HeaderFooterRef<'_>> {
        let headers = self.headers.iter().map(|(variant, stream)| HeaderFooterRef { header: true, variant, stream });
        let footers = self.footers.iter().map(|(variant, stream)| HeaderFooterRef { header: false, variant, stream });
        headers.chain(footers)
    }

    /// Mutable access to every existing header and footer stream.
    pub fn header_footers_mut(&mut self) -> impl Iterator<Item = &mut HeaderFooter> {
        self.headers.iter_mut().chain(self.footers.iter_mut()).map(|(_, stream)| stream)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeaderFooter {
    pub id: NodeId,
    pub blocks: crate::Shared<Vec<Block>>,
}

impl Default for HeaderFooter {
    fn default() -> Self { Self { id: NodeId::new(), blocks: (Vec::new()).into() } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Block {
    Paragraph(Paragraph),
    Equation(Equation),
    Table(Table),
    Image(BlockImage),
    PageBreak { id: NodeId },
    ColumnBreak { id: NodeId },
    Foreign(ForeignBlock),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paragraph {
    pub id: NodeId,
    pub style: Option<StyleId>,
    pub properties: ParagraphProperties,
    pub content: crate::Shared<Vec<Inline>>,
}

impl Paragraph {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            id: NodeId::new(),
            style: Some(StyleId::from(STYLE_NORMAL)),
            properties: ParagraphProperties::default(),
            content: (vec![Inline::Text(TextRun::plain(text))]).into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Inline {
    Text(TextRun),
    Equation(Equation),
    Image(InlineImage),
    EmbeddedObject(EmbeddedObject),
    TextBox(TextBox),
    Field(Field),
    LineBreak { id: NodeId },
    PaginationBreak { id: NodeId, break_kind: PaginationBreakKind },
    Tab { id: NodeId },
    Hyperlink(Hyperlink),
}

/// An explicit flow boundary inside a paragraph; its source marker is one byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaginationBreakKind { Page, Column }
impl PaginationBreakKind {
    pub const fn marker(self) -> char { match self { Self::Page => '\u{000C}', Self::Column => '\u{000E}' } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub id: NodeId,
    pub text: String,
    pub style: Option<StyleId>,
    pub properties: CharacterProperties,
}

impl TextRun {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { id: NodeId::new(), text: text.into(), style: None, properties: CharacterProperties::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hyperlink {
    pub id: NodeId,
    pub target: String,
    pub content: crate::Shared<Vec<Inline>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub id: NodeId,
    #[serde(default)]
    pub properties: TableProperties,
    pub rows: Vec<TableRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TableProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<StyleId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_first_row: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_row_banding: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_overrides_table_borders: Option<bool>,
    pub preferred_width_mm: Option<f32>,
    /// Offset from the text area's left edge; DOCX `w:tblInd` may be negative.
    #[serde(default)]
    pub left_indent_mm: Option<f32>,
    pub alignment: Option<TableAlignment>,
    pub borders: Option<BorderSet>,
    pub cell_spacing_mm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_margins: Option<TableCellMargins>,
    /// Logical table-grid column widths.  Keeping this on the table rather
    /// than individual physical cells makes merged cells and row spans share
    /// one stable width model and maps directly to DOCX `w:tblGrid`.
    #[serde(default)]
    pub grid_column_widths_mm: Vec<f32>,
    /// Floating table (DOCX `w:tblpPr`): placed by position instead of in
    /// the text flow, with text wrapping around it. `None` is an inline table.
    #[serde(default)]
    pub position: Option<TablePosition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableAlignment { Left, Center, Right }

/// Where a floating table sits, as in Word's Table Positioning dialog. The
/// table is anchored to the paragraph that follows it in the document; that
/// paragraph decides the page, and is the reference for `VerticalFrom::Paragraph`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TablePosition {
    pub horizontal_from: HorizontalFrom,
    pub horizontal: HorizontalPlacement,
    pub vertical_from: VerticalFrom,
    pub vertical: VerticalPlacement,
    /// Distance kept between the table and the text wrapping around it.
    pub distance_left_mm: f32,
    pub distance_right_mm: f32,
    pub distance_top_mm: f32,
    pub distance_bottom_mm: f32,
    #[serde(default)]
    pub wrap: TableWrap,
    /// Other floating tables may overlap this one (DOCX `w:tblOverlap`).
    #[serde(default = "yes")]
    pub allow_overlap: bool,
}

fn yes() -> bool { true }

impl Default for TablePosition {
    fn default() -> Self {
        Self {
            horizontal_from: HorizontalFrom::Margin, horizontal: HorizontalPlacement::Offset(0.0),
            vertical_from: VerticalFrom::Paragraph, vertical: VerticalPlacement::Offset(0.0),
            distance_left_mm: 3.2, distance_right_mm: 3.2, distance_top_mm: 0.0, distance_bottom_mm: 0.0,
            wrap: TableWrap::Around, allow_overlap: true,
        }
    }
}

impl TablePosition {
    pub fn validate(&self) -> Result<(), String> {
        let offset_ok = |v: f32| v.is_finite() && v.abs() <= 1000.0;
        let offsets = [match self.horizontal { HorizontalPlacement::Offset(v) => v, _ => 0.0 }, match self.vertical { VerticalPlacement::Offset(v) => v, _ => 0.0 }];
        let distances = [self.distance_left_mm, self.distance_right_mm, self.distance_top_mm, self.distance_bottom_mm];
        if !offsets.iter().all(|v| offset_ok(*v)) || !distances.iter().all(|v| v.is_finite() && (0.0..=100.0).contains(v)) {
            return Err("Invalid table position or distance from text".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizontalFrom { #[default] Margin, Page, Column }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerticalFrom { #[default] Paragraph, Margin, Page }

/// An offset in millimetres from the reference's left edge, or an alignment
/// within it. Inside/Outside are left/right on odd pages and mirrored on even ones.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizontalPlacement { Offset(f32), Left, Center, Right, Inside, Outside }

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerticalPlacement { Offset(f32), Top, Center, Bottom }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableWrap {
    /// Text flows on both sides of the table (Word's "Around").
    #[default]
    Around,
    /// Text stops above the table and continues below it.
    TopAndBottom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableRow {
    pub id: NodeId,
    pub cells: Vec<TableCell>,
    #[serde(default)]
    pub repeat_as_header: bool,
    /// Word's default is to allow a row to split across page/column boundaries.
    /// Setting this to false maps to OOXML `w:cantSplit`.
    #[serde(default = "default_true")]
    pub allow_break_across_pages: bool,
    pub height_mm: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableCell {
    pub id: NodeId,
    pub blocks: crate::Shared<Vec<Block>>,
    #[serde(default = "one")]
    pub column_span: u16,
    #[serde(default = "one")]
    pub row_span: u16,
    pub width_mm: Option<f32>,
    pub shading: Option<String>,
    pub borders: Option<BorderSet>,
    #[serde(default)]
    pub vertical_alignment: CellVerticalAlignment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margins: Option<TableCellMargins>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_direction: Option<CellTextDirection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellTextDirection { BottomToTop, TopToBottom }

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TableCellMargins {
    pub top_mm: Option<f32>,
    pub right_mm: Option<f32>,
    pub bottom_mm: Option<f32>,
    pub left_mm: Option<f32>,
}

impl TableCellMargins {
    pub fn over(self, inherited: Self) -> Self {
        Self {
            top_mm: self.top_mm.or(inherited.top_mm),
            right_mm: self.right_mm.or(inherited.right_mm),
            bottom_mm: self.bottom_mm.or(inherited.bottom_mm),
            left_mm: self.left_mm.or(inherited.left_mm),
        }
    }
}

const fn one() -> u16 { 1 }
const fn default_true() -> bool { true }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellVerticalAlignment { #[default] Top, Center, Bottom }

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BorderSet {
    pub top: Option<Border>,
    pub right: Option<Border>,
    pub bottom: Option<Border>,
    pub left: Option<Border>,
    pub inside_horizontal: Option<Border>,
    pub inside_vertical: Option<Border>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Border {
    pub width_pt: f32,
    pub color: String,
    #[serde(default)]
    pub style: BorderStyle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle { #[default] Single, Double, Dotted, Dashed, None }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InlineImage {
    pub id: NodeId,
    pub resource: ResourceId,
    pub width_mm: f32,
    pub height_mm: f32,
    pub alt_text: Option<String>,
    #[serde(default)]
    pub effects: ImageEffects,
}

/// An embedded document or OLE payload with an independent picture preview.
/// The payload bytes live in Document.resources, so editing the surrounding
/// paragraph does not discard the embedded object during DOCX export.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmbeddedObject {
    pub id: NodeId,
    pub payload: ResourceId,
    pub preview: Option<ResourceId>,
    pub width_mm: f32,
    pub height_mm: f32,
    pub program: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextBox {
    pub id: NodeId,
    pub width_mm: f32,
    pub height_mm: f32,
    pub blocks: crate::Shared<Vec<Block>>,
    pub fill: Option<String>,
    pub line: Option<String>,
    #[serde(default)] pub anchor: Option<ImageAnchor>,
    #[serde(default)] pub wrap: ImageWrap,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockImage {
    pub id: NodeId,
    pub resource: ResourceId,
    pub width_mm: f32,
    pub height_mm: f32,
    pub alt_text: Option<String>,
    #[serde(default)]
    pub effects: ImageEffects,
    #[serde(default)]
    pub wrap: ImageWrap,
    #[serde(default)]
    pub alignment: ImageAlignment,
    #[serde(default)] pub anchor: Option<ImageAnchor>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImageEffects {
    /// Fractions removed from the four sides of the source bitmap.
    pub crop: Option<ImageCrop>,
    pub outer_shadow: Option<ImageShadow>,
    #[serde(default)] pub glow: Option<ImageGlow>,
    #[serde(default)] pub soft_edge_mm: Option<f32>,
    #[serde(default)] pub reflection: Option<ImageReflection>,
    #[serde(default)] pub shape: ImageShape,
    #[serde(default)] pub outline: Option<Border>,
}

/// Geometry used to crop the picture, independently of its source bitmap.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageShape {
    pub preset: String,
    #[serde(default)] pub adjustments: BTreeMap<String, String>,
}

impl Default for ImageShape {
    fn default() -> Self { Self { preset: "rect".into(), adjustments: BTreeMap::new() } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageGlow { pub color: String, pub opacity: f32, pub radius_mm: f32 }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageReflection { pub blur_mm: f32, pub distance_mm: f32, pub start_opacity: f32, pub end_opacity: f32 }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageCrop {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageShadow {
    pub color: String,
    pub opacity: f32,
    pub distance_mm: f32,
    pub direction_degrees: f32,
    pub blur_mm: f32,
}
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Serialize,Deserialize)]
pub enum AnchorReference { #[default] Paragraph, Page }
/// Page coordinates follow the anchor paragraph's physical page. Locking the
/// anchor prevents reattachment, not repositioning within the anchored page.
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub struct ImageAnchor {
    pub paragraph:NodeId, pub reference:AnchorReference,
    pub x_mm:f32, pub y_mm:f32, pub gap_mm:f32, pub locked:bool,
}
impl ImageAnchor {
    pub fn validate(&self)->Result<(),String>{
        if !self.x_mm.is_finite()||!self.y_mm.is_finite()||!self.gap_mm.is_finite()
            ||self.x_mm.abs()>1000.0||self.y_mm.abs()>1000.0||!(0.0..=50.0).contains(&self.gap_mm){return Err("Invalid picture anchor coordinates or text distance".into());}Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageWrap {
    #[default] Inline, Square, Tight, BehindText, InFrontOfText,
    /// Text above and below the picture only, none beside it (Word's "Top and Bottom").
    TopAndBottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageAlignment { #[default] Left, Center, Right }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub id: NodeId,
    // Inline itself uses `kind` as its externally tagged discriminator.
    // Giving the nested field the same JSON key produced duplicate `kind`
    // entries and made native documents with fields impossible to reopen.
    #[serde(rename = "field_kind")]
    pub kind: FieldKind,
    pub display_text: Option<String>,
    /// Direct text color, including live page-number and date field results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    PageNumber,
    PageReference { bookmark: String },
    FootnoteReference { note: NodeId },
    PageCount,
    SectionNumber,
    Date { format: Option<String> },
    Raw { instruction: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListDefinition {
    pub id: NodeId,
    pub levels: Vec<ListLevel>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListLevel {
    pub level: u8,
    pub format: ListFormat,
    pub text: String,
    pub start_at: i32,
    pub left_indent_mm: f32,
    pub hanging_indent_mm: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListFormat { Bullet, Decimal, LowerLetter, UpperLetter, LowerRoman, UpperRoman }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ForeignBlock {
    pub id: NodeId,
    pub format: String,
    pub media_type: String,
    pub payload_base64: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmbeddedFontFace {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub face_index: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    pub id: ResourceId,
    pub media_type: String,
    pub original_name: Option<String>,
    /// Font bindings are local to this document, including fallback aliases.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub font_faces: Vec<EmbeddedFontFace>,
    #[serde(skip, default)]
    pub data: std::sync::Arc<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreservedPackagePart {
    pub media_type: Option<String>,
    pub data_base64: crate::Shared<String>,
}


