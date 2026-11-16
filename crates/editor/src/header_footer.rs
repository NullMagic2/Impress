//! Header and footer editing. Each section owns up to three header streams and
//! three footer streams (default, first page, even pages). Once the caret is
//! inside one of them, normal typing, formatting and undo use the same editor
//! code as body paragraphs, because paragraph lookup is recursive.
use super::*;
use crate::formatting::first_paragraph_id;
use model::{Field, FieldKind, HeaderFooter, ParagraphAlignment, ParagraphProperties, TabStop, TabAlignment, TabLeader, Border, BorderSet, BorderStyle};

/// Address of the header or footer stream that holds the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderFooterContext {
    pub section_id: NodeId,
    pub section_index: usize,
    pub header: bool,
    pub variant: HeaderFooterVariant,
}

impl HeaderFooterContext {
    /// Human-readable stream name, for example "First page header".
    pub fn title(&self) -> String {
        let band = if self.header { "header" } else { "footer" };
        match self.variant {
            HeaderFooterVariant::Default => format!("{}{}", band[..1].to_uppercase(), &band[1..]),
            HeaderFooterVariant::First => format!("First page {band}"),
            HeaderFooterVariant::Even => format!("Even page {band}"),
        }
    }
}

/// Built-in layouts for an editable header or footer stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderFooterTemplate {
    Blank,
    Centered,
    ThreeColumns,
    TitlePage,
    PageOfPages,
    TitleDate,
    SimpleRule,
    AccentRule,
    DoubleRule,
    ShadedBand,
    Framed,
}

/// Localized placeholder text. Fields remain semantic document fields.
pub struct HeaderFooterTemplateText {
    pub placeholder: String,
    pub title: String,
    pub author: String,
    pub date: String,
    pub page: String,
    pub of: String,
}

/// Template colors and stroke widths are document properties, in points.
pub(super) const TEMPLATE_RULE_COLOR: &str = "#66758B";
pub(super) const TEMPLATE_ACCENT_COLOR: &str = "#336EC1";
pub(super) const TEMPLATE_BAND_COLOR: &str = "#EAF1FB";
const TEMPLATE_RULE_WIDTH_PT: f32 = 0.75;
const TEMPLATE_ACCENT_WIDTH_PT: f32 = 2.0;
const TEMPLATE_DOUBLE_WIDTH_PT: f32 = 0.6;
const TEMPLATE_BAND_EDGE_WIDTH_PT: f32 = 4.0;
const TEMPLATE_LINE_SPACING: f32 = 1.8;
const TEMPLATE_TEXT_INSET_MM: f32 = 2.0;

impl HeaderFooterTemplate {
    /// Build ordinary document content, shared by insertion and gallery previews.
    pub fn paragraph(
        self,
        width_mm: f32,
        text: &HeaderFooterTemplateText,
        header: bool,
    ) -> Paragraph {
        let run = |value: &str| Inline::Text(TextRun::plain(value));
        let tab = || Inline::Tab { id: NodeId::new() };
        let field = |kind, display_text| {
            Inline::Field(Field {
                id: NodeId::new(),
                kind,
                display_text,
                color: None,
            })
        };
        let mut paragraph = Paragraph::plain("");
        paragraph.properties = ParagraphProperties {
            alignment: Some(
                if matches!(self, Self::Centered | Self::PageOfPages | Self::DoubleRule) {
                    ParagraphAlignment::Center
                } else {
                    ParagraphAlignment::Left
                },
            ),
            space_before_pt: Some(0.0),
            space_after_pt: Some(0.0),
            first_line_indent_mm: Some(0.0),
            left_indent_mm: Some(0.0),
            right_indent_mm: Some(0.0),
            ..Default::default()
        };
        let mut tabs = Vec::new();
        let mut stop = |fraction, alignment| {
            tabs.push(TabStop {
                position_mm: width_mm * fraction,
                alignment,
                leader: TabLeader::None,
            })
        };
        let content = match self {
            Self::Blank => vec![run(&text.placeholder)],
            Self::Centered => vec![run(&text.title)],
            Self::ThreeColumns | Self::ShadedBand => {
                stop(0.5, TabAlignment::Center);
                stop(
                    if self == Self::ShadedBand {
                        1.0 - TEMPLATE_TEXT_INSET_MM / width_mm
                    } else {
                        1.0
                    },
                    TabAlignment::Right,
                );
                vec![
                    run(&text.author),
                    tab(),
                    run(&text.title),
                    tab(),
                    field(FieldKind::PageNumber, None),
                ]
            }
            Self::TitlePage | Self::SimpleRule | Self::AccentRule => {
                stop(1.0, TabAlignment::Right);
                vec![run(&text.title), tab(), field(FieldKind::PageNumber, None)]
            }
            Self::PageOfPages | Self::DoubleRule => vec![
                run(&format!("{} ", text.page)),
                field(FieldKind::PageNumber, None),
                run(&format!(" {} ", text.of)),
                field(FieldKind::PageCount, None),
            ],
            Self::TitleDate | Self::Framed => {
                stop(
                    if self == Self::Framed {
                        1.0 - TEMPLATE_TEXT_INSET_MM / width_mm
                    } else {
                        1.0
                    },
                    TabAlignment::Right,
                );
                vec![
                    run(&text.title),
                    tab(),
                    field(FieldKind::Date { format: None }, Some(text.date.clone())),
                ]
            }
        };
        paragraph.properties.tabs = Some(tabs);
        paragraph.content = content.into();
        self.decorate(&mut paragraph, header);
        paragraph
    }
    fn decorate(self, paragraph: &mut Paragraph, header: bool) {
        let border = |width_pt, color: &str, style| Border {
            width_pt,
            color: color.into(),
            style,
        };
        let mut borders = BorderSet::default();
        let rule = match self {
            Self::SimpleRule => Some(border(
                TEMPLATE_RULE_WIDTH_PT,
                TEMPLATE_RULE_COLOR,
                BorderStyle::Single,
            )),
            Self::AccentRule => Some(border(
                TEMPLATE_ACCENT_WIDTH_PT,
                TEMPLATE_ACCENT_COLOR,
                BorderStyle::Single,
            )),
            Self::DoubleRule => Some(border(
                TEMPLATE_DOUBLE_WIDTH_PT,
                TEMPLATE_RULE_COLOR,
                BorderStyle::Double,
            )),
            _ => None,
        };
        if let Some(rule) = rule {
            if header {
                borders.bottom = Some(rule);
            } else {
                borders.top = Some(rule);
            }
        }
        match self {
            Self::ShadedBand => {
                paragraph.properties.shading = Some(TEMPLATE_BAND_COLOR.into());
                borders.left = Some(border(
                    TEMPLATE_BAND_EDGE_WIDTH_PT,
                    TEMPLATE_ACCENT_COLOR,
                    BorderStyle::Single,
                ));
            }
            Self::Framed => {
                let line = border(
                    TEMPLATE_RULE_WIDTH_PT,
                    TEMPLATE_RULE_COLOR,
                    BorderStyle::Single,
                );
                borders.top = Some(line.clone());
                borders.bottom = Some(line.clone());
                borders.left = Some(line.clone());
                borders.right = Some(line);
            }
            _ => {}
        }
        if matches!(self, Self::ShadedBand | Self::Framed) {
            paragraph.properties.first_line_indent_mm = Some(TEMPLATE_TEXT_INSET_MM);
        }
        if borders != BorderSet::default() {
            paragraph.properties.borders = Some(borders);
            paragraph.properties.line_spacing = Some(TEMPLATE_LINE_SPACING);
        }
        if self == Self::AccentRule {
            for inline in paragraph.content.iter_mut() {
                if let Inline::Text(run) = inline {
                    run.properties.bold = Some(true);
                    run.properties.color = Some(TEMPLATE_ACCENT_COLOR.into());
                }
            }
        }
    }
}

impl EditorState {
    /// Whether the addressed stream in the current section already has content.
    pub fn has_header_footer_at_caret(&self, header: bool, variant: HeaderFooterVariant) -> bool {
        self.focus_position().and_then(|position| self.section_index_for_paragraph(&position.node_id))
            .and_then(|index| self.document.sections.get(index))
            .and_then(|section| if header { section.headers.get(variant) } else { section.footers.get(variant) })
            .is_some_and(|stream| !stream.blocks.is_empty())
    }

    /// Replace one stream in a single undo step; leave its placeholder selected.
    pub fn apply_header_footer_template(&mut self, header: bool, variant: HeaderFooterVariant, template: HeaderFooterTemplate, text: &HeaderFooterTemplateText) -> Result<(), String> {
        self.apply_header_footer_template_impl(header, variant, template, text, None)
    }

    fn apply_header_footer_template_impl(&mut self, header: bool, variant: HeaderFooterVariant, template: HeaderFooterTemplate, text: &HeaderFooterTemplateText, colors: Option<HeaderFooterColors>) -> Result<(), String> {
        let section_index = self.focus_position().and_then(|p| self.section_index_for_paragraph(&p.node_id)).unwrap_or(0);
        let before = self.document.sections.get(section_index).ok_or("Section not found")?.clone();
        let before_selection = self.selection.clone();
        let mut after = before.clone();
        let paragraph = template.paragraph(after.properties.text_width_mm(), text, header);
        let target = paragraph.id.clone();
        let length = match paragraph.content.first() { Some(Inline::Text(run)) => run.text.len(), _ => 0 };
        *after.header_footer_set_mut(header).slot_mut(variant) = Some(HeaderFooter { id: NodeId::new(), blocks: vec![Block::Paragraph(paragraph)].into() });
        if let Some(colors) = colors { colors.apply(after.header_footer_set_mut(header).slot_mut(variant).as_mut().expect("template stream exists")); }
        match variant {
            HeaderFooterVariant::First => after.properties.different_first_page = true,
            HeaderFooterVariant::Even => after.properties.different_odd_even = true,
            _ => {}
        }
        self.document.sections[section_index] = after.clone();
        self.selection = Some(Selection { anchor: DocumentPosition::new(target.clone(), 0), focus: DocumentPosition::new(target, length) });
        self.reset_typing_properties_from_caret();
        self.break_coalescing();
        self.commit(if header { "Insert header template" } else { "Insert footer template" }, vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], before_selection, None)
    }

    /// Insert a template and its chosen colors as a single edit.
    pub fn apply_colored_header_footer_template(&mut self, header: bool, variant: HeaderFooterVariant, template: HeaderFooterTemplate, text: &HeaderFooterTemplateText, colors: HeaderFooterColors) -> Result<(), String> {
        self.apply_header_footer_template_impl(header, variant, template, text, Some(colors))
    }

    pub fn enter_header(&mut self, variant: HeaderFooterVariant) -> Result<(), String> {
        self.enter_header_footer(true, variant)
    }

    pub fn enter_footer(&mut self, variant: HeaderFooterVariant) -> Result<(), String> {
        self.enter_header_footer(false, variant)
    }

    /// Enter a stream of a specific section, for example after a double-click
    /// in a page margin.
    pub fn enter_header_footer_at(&mut self, section_id: &NodeId, header: bool, variant: HeaderFooterVariant) -> Result<(), String> {
        let section = self.document.sections.iter().find(|s| &s.id == section_id).ok_or("Section not found")?;
        let body_paragraph = first_paragraph_id(&section.blocks).ok_or("Section has no body paragraph")?;
        self.set_caret(body_paragraph, 0);
        self.enter_header_footer(header, variant)
    }

    /// Move from a header to the footer of the same variant, or back.
    pub fn switch_header_footer(&mut self) -> Result<(), String> {
        let context = self.header_footer_context().ok_or("Place the caret in a header or footer first")?;
        self.enter_header_footer(!context.header, context.variant)
    }

    pub fn exit_header_footer(&mut self) -> Result<(), String> {
        let section_index = self.focus_position()
            .and_then(|p| self.section_index_for_paragraph(&p.node_id))
            .unwrap_or(0);
        let paragraph = self.document.sections.get(section_index)
            .and_then(|section| first_paragraph_id(&section.blocks))
            .or_else(|| first_paragraph_position(&self.document).map(|p| p.node_id))
            .ok_or("document has no body paragraph")?;
        self.set_caret(paragraph, 0);
        Ok(())
    }

    pub fn is_editing_header_footer(&self) -> bool {
        self.header_footer_context().is_some()
    }

    /// The header or footer stream that holds the caret, if any.
    pub fn header_footer_context(&self) -> Option<HeaderFooterContext> {
        let id = self.focus_position()?.node_id;
        self.document.sections.iter().enumerate().find_map(|(section_index, section)| {
            section.header_footers()
                .find(|hf| find_paragraph_in_blocks(&hf.stream.blocks, &id).is_some())
                .map(|hf| HeaderFooterContext {
                    section_id: section.id.clone(), section_index, header: hf.header, variant: hf.variant,
                })
        })
    }

    /// Delete the stream that holds the caret and return to the body. When a
    /// first-page or even-page variant no longer has a header or a footer, the
    /// section stops using that variant. One undo step restores everything.
    pub fn remove_header_footer(&mut self) -> Result<(), String> {
        let context = self.header_footer_context().ok_or("Place the caret in a header or footer first")?;
        let before_selection = self.selection.clone();
        let before = self.document.sections[context.section_index].clone();
        let mut after = before.clone();
        *after.header_footer_set_mut(context.header).slot_mut(context.variant) = None;
        let variant_unused = after.headers.get(context.variant).is_none() && after.footers.get(context.variant).is_none();
        match context.variant {
            HeaderFooterVariant::First if variant_unused => after.properties.different_first_page = false,
            HeaderFooterVariant::Even if variant_unused => after.properties.different_odd_even = false,
            _ => {}
        }
        let body_paragraph = first_paragraph_id(&after.blocks).ok_or("Section has no body paragraph")?;
        self.document.sections[context.section_index] = after.clone();
        self.selection = Some(Selection::collapsed(DocumentPosition::new(body_paragraph, 0)));
        let label = if context.header { "Remove header" } else { "Remove footer" };
        self.commit(label, vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], before_selection, None)
    }

    /// Add a centered PAGE field to the current section's default footer.
    pub fn insert_page_number_footer(&mut self) -> Result<(), String> {
        let position = self.focus_position().ok_or("no active caret")?;
        let section_index = self.section_index_for_paragraph(&position.node_id).ok_or("section not found")?;
        let before = self.document.sections[section_index].clone();
        let mut after = before.clone();
        let footer = after.footers.default.get_or_insert_with(HeaderFooter::default);
        let already = footer.blocks.iter().any(|block| match block {
            Block::Paragraph(p) => p.content.iter().any(|i| matches!(i, Inline::Field(Field { kind: FieldKind::PageNumber, .. }))),
            _ => false,
        });
        if already { return Ok(()); }
        footer.blocks.push(Block::Paragraph(Paragraph {
            id: NodeId::new(), style: None,
            properties: ParagraphProperties { alignment: Some(ParagraphAlignment::Center), ..Default::default() },
            content: (vec![Inline::Field(Field { id: NodeId::new(), kind: FieldKind::PageNumber, display_text: None, color: None })]).into(),
        }));
        self.document.sections[section_index] = after.clone();
        self.commit("Insert page number", vec![Mutation::SectionChanged { section_id: after.id.clone(), before, after }], self.selection.clone(), None)?;
        Ok(())
    }

    /// Create the stream on demand (with one empty paragraph) and put the
    /// caret in it. Choosing a first-page or even-page stream turns that
    /// variant on for the section.
    fn enter_header_footer(&mut self, header: bool, variant: HeaderFooterVariant) -> Result<(), String> {
        let section_index = self.focus_position()
            .and_then(|p| self.section_index_for_paragraph(&p.node_id))
            .unwrap_or(0);
        if section_index >= self.document.sections.len() { return Err("section not found".into()); }
        let before_selection = self.selection.clone();
        let before = self.document.sections[section_index].clone();
        let mut created = false;
        let target = {
            let section = &mut self.document.sections[section_index];
            match variant {
                HeaderFooterVariant::First if !section.properties.different_first_page => {
                    section.properties.different_first_page = true;
                    created = true;
                }
                HeaderFooterVariant::Even if !section.properties.different_odd_even => {
                    section.properties.different_odd_even = true;
                    created = true;
                }
                _ => {}
            }
            let slot = section.header_footer_set_mut(header).slot_mut(variant);
            if slot.is_none() { *slot = Some(HeaderFooter::default()); created = true; }
            let stream = slot.as_mut().expect("header/footer was just created");
            if first_paragraph_id(&stream.blocks).is_none() {
                stream.blocks.push(Block::Paragraph(Paragraph::plain(""))); created = true;
            }
            first_paragraph_id(&stream.blocks).expect("header/footer paragraph exists")
        };
        self.selection = Some(Selection::collapsed(DocumentPosition::new(target, 0)));
        self.reset_typing_properties_from_caret();
        self.break_coalescing();
        if created {
            let after = self.document.sections[section_index].clone();
            self.commit(if header { "Create header" } else { "Create footer" }, vec![Mutation::SectionChanged {
                section_id: after.id.clone(), before, after,
            }], before_selection, None)?;
        }
        Ok(())
    }
}

