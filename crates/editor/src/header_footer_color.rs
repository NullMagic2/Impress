//! Independent decoration and text colors for editable header/footer streams.
use super::header_footer::{TEMPLATE_ACCENT_COLOR, TEMPLATE_BAND_COLOR, TEMPLATE_RULE_COLOR};
use super::*;
use model::{BorderSet, HeaderFooter, StyleSheet};

/// Automatic text starts black; decorations use the template palette.
const DEFAULT_TEXT_COLOR: &str = "#000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeaderFooterColor(pub [u8; 3]);
impl HeaderFooterColor {
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2])
    }
    pub fn from_hex(value: &str) -> Option<Self> {
        if value.len() != 7 || !value.starts_with('#') {
            return None;
        }
        let value = u32::from_str_radix(&value[1..], 16).ok()?;
        Some(Self([(value >> 16) as u8, (value >> 8) as u8, value as u8]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderFooterColorTarget {
    Bracket,
    Line,
    Text,
    Background,
}
impl HeaderFooterColorTarget {
    pub fn default_color(self) -> HeaderFooterColor {
        let value = match self {
            Self::Bracket => TEMPLATE_ACCENT_COLOR,
            Self::Line => TEMPLATE_RULE_COLOR,
            Self::Text => DEFAULT_TEXT_COLOR,
            Self::Background => TEMPLATE_BAND_COLOR,
        };
        HeaderFooterColor::from_hex(value).expect("template colors are valid RGB literals")
    }
}

/// None leaves that component untouched, including mixed colors in existing text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeaderFooterColors {
    pub bracket: Option<HeaderFooterColor>,
    pub line: Option<HeaderFooterColor>,
    pub text: Option<HeaderFooterColor>,
    pub background: Option<HeaderFooterColor>,
}
impl HeaderFooterColors {
    pub fn get(self, target: HeaderFooterColorTarget) -> Option<HeaderFooterColor> {
        match target {
            HeaderFooterColorTarget::Bracket => self.bracket,
            HeaderFooterColorTarget::Line => self.line,
            HeaderFooterColorTarget::Text => self.text,
            HeaderFooterColorTarget::Background => self.background,
        }
    }
    pub fn set(&mut self, target: HeaderFooterColorTarget, color: HeaderFooterColor) {
        *match target {
            HeaderFooterColorTarget::Bracket => &mut self.bracket,
            HeaderFooterColorTarget::Line => &mut self.line,
            HeaderFooterColorTarget::Text => &mut self.text,
            HeaderFooterColorTarget::Background => &mut self.background,
        } = Some(color);
    }
    /// Inspect the first visible value of each component without changing content.
    pub fn from_stream(stream: &HeaderFooter, styles: &StyleSheet) -> Self {
        let mut paragraphs = Vec::new();
        model::visit_paragraphs(&stream.blocks, &mut paragraphs);
        let mut colors = Self::default();
        for paragraph in paragraphs {
            if let Some(borders) = &paragraph.properties.borders {
                colors.bracket = colors
                    .bracket
                    .or_else(|| border_color([&borders.left, &borders.right]));
                colors.line = colors
                    .line
                    .or_else(|| border_color([&borders.top, &borders.bottom]));
            }
            colors.background = colors.background.or_else(|| {
                paragraph
                    .properties
                    .shading
                    .as_deref()
                    .and_then(HeaderFooterColor::from_hex)
            });
            if colors.text.is_none() {
                let base = styles.resolve_character(paragraph.style.as_ref());
                colors.text = inline_color(&paragraph.content, &base, styles);
            }
        }
        colors
    }
    pub fn apply(self, stream: &mut HeaderFooter) {
        self.blocks(&mut stream.blocks);
    }
    fn borders(self, borders: &mut Option<BorderSet>) {
        let Some(borders) = borders else {
            return;
        };
        for (color, sides) in [
            (
                self.bracket,
                [
                    &mut borders.left,
                    &mut borders.right,
                    &mut borders.inside_vertical,
                ],
            ),
            (
                self.line,
                [
                    &mut borders.top,
                    &mut borders.bottom,
                    &mut borders.inside_horizontal,
                ],
            ),
        ] {
            if let Some(color) = color {
                for border in sides.into_iter().flatten() {
                    border.color = color.hex();
                }
            }
        }
    }
    fn blocks(self, blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    self.borders(&mut paragraph.properties.borders);
                    if paragraph.properties.shading.is_some() {
                        if let Some(color) = self.background {
                            paragraph.properties.shading = Some(color.hex());
                        }
                    }
                    self.inline(&mut paragraph.content);
                }
                Block::Table(table) => {
                    for row in table.rows.iter_mut() {
                        for cell in row.cells.iter_mut() {
                            self.blocks(&mut cell.blocks);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    fn inline(self, content: &mut [Inline]) {
        for inline in content {
            match inline {
                Inline::Text(run) => {
                    if let Some(color) = self.text {
                        run.properties.color = Some(color.hex());
                    }
                }
                Inline::Field(field) => {
                    if let Some(color) = self.text {
                        field.color = Some(color.hex());
                    }
                }
                Inline::Hyperlink(link) => self.inline(&mut link.content),
                Inline::TextBox(boxed) => self.blocks(&mut boxed.blocks),
                _ => {}
            }
        }
    }
}
fn border_color<const N: usize>(borders: [&Option<model::Border>; N]) -> Option<HeaderFooterColor> {
    borders
        .into_iter()
        .flatten()
        .find_map(|border| HeaderFooterColor::from_hex(&border.color))
}
fn inline_color(
    content: &[Inline],
    base: &CharacterProperties,
    styles: &StyleSheet,
) -> Option<HeaderFooterColor> {
    for inline in content {
        match inline {
            Inline::Text(run) => {
                let mut properties = base.clone();
                if let Some(style) = &run.style {
                    properties.overlay(&styles.resolve_character(Some(style)));
                }
                properties.overlay(&run.properties);
                return Some(
                    properties
                        .color
                        .as_deref()
                        .and_then(HeaderFooterColor::from_hex)
                        .unwrap_or(HeaderFooterColorTarget::Text.default_color()),
                );
            }
            Inline::Field(field) => {
                return Some(
                    field
                        .color
                        .as_deref()
                        .or(base.color.as_deref())
                        .and_then(HeaderFooterColor::from_hex)
                        .unwrap_or(HeaderFooterColorTarget::Text.default_color()),
                );
            }
            Inline::Hyperlink(link) => {
                if let Some(color) = inline_color(&link.content, base, styles) {
                    return Some(color);
                }
            }
            _ => {}
        }
    }
    None
}

impl EditorState {
    pub fn header_footer_at_caret(
        &self,
        header: bool,
        variant: HeaderFooterVariant,
    ) -> Option<HeaderFooter> {
        let index = self
            .focus_position()
            .and_then(|position| self.section_index_for_paragraph(&position.node_id))?;
        let section = self.document.sections.get(index)?;
        (if header {
            section.headers.get(variant)
        } else {
            section.footers.get(variant)
        })
        .cloned()
    }
    /// Recolor just the requested components in one edit, preserving text and IDs.
    pub fn set_header_footer_colors(
        &mut self,
        header: bool,
        variant: HeaderFooterVariant,
        colors: HeaderFooterColors,
    ) -> Result<(), String> {
        let index = self
            .focus_position()
            .and_then(|position| self.section_index_for_paragraph(&position.node_id))
            .ok_or("Section not found")?;
        let before = self.document.sections[index].clone();
        let mut after = before.clone();
        let stream = after
            .header_footer_set_mut(header)
            .slot_mut(variant)
            .as_mut()
            .ok_or("Enter a header or footer first")?;
        colors.apply(stream);
        if before == after {
            return Ok(());
        }
        self.document.sections[index] = after.clone();
        self.commit(
            "Header/footer color",
            vec![Mutation::SectionChanged {
                section_id: after.id.clone(),
                before,
                after,
            }],
            self.selection.clone(),
            None,
        )
    }
}
