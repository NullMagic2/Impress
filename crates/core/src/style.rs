use crate::{BorderSet, NodeId, StyleId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const STYLE_NORMAL: &str = "builtin.normal";
pub const STYLE_HEADING_1: &str = "builtin.heading1";
pub const STYLE_TITLE: &str = "builtin.title";
pub const STYLE_HEADING_2: &str = "builtin.heading2";
pub const STYLE_HEADING_3: &str = "builtin.heading3";
pub const STYLE_SUBTITLE: &str = "builtin.subtitle";
pub const STYLE_QUOTE: &str = "builtin.quote";
/// Built-in styles beyond the three every document keeps. A document that
/// brings styles of the same names (a Word file) uses its own instead.
pub const OPTIONAL_BUILTIN_STYLES: [&str; 4] = [STYLE_HEADING_2, STYLE_HEADING_3, STYLE_SUBTITLE, STYLE_QUOTE];
/// The built-in headings' blue, the one the style gallery has always shown.
const HEADING_COLOR: &str = "#336EC1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleSheet {
    pub styles: BTreeMap<StyleId, Style>,
}

impl Default for StyleSheet {
    fn default() -> Self {
        let mut styles = BTreeMap::new();

        let normal_id = StyleId::from(STYLE_NORMAL);
        styles.insert(normal_id.clone(), Style {
            id: normal_id.clone(),
            name: "Normal".into(),
            kind: StyleKind::Paragraph,
            based_on: None,
            next_style: None,
            character: CharacterProperties {
                font_family: Some("Linux Libertine G".into()),
                font_size_pt: Some(11.0),
                language: Some("en".into()),
                typography: Some(OpenTypeTypography::default()),
                ..CharacterProperties::default()
            },
            paragraph: ParagraphProperties {
                alignment: Some(ParagraphAlignment::Left),
                space_after_pt: Some(3.0),
                line_spacing: Some(1.2),
                hyphenation: Some(true),
                hanging_punctuation: Some(true),
                glyph_expansion_per_mille: Some(15),
                ..ParagraphProperties::default()
            },
            formatting: BTreeMap::new(),
        });

        let heading_id = StyleId::from(STYLE_HEADING_1);
        styles.insert(heading_id.clone(), Style {
            id: heading_id.clone(),
            name: "Heading 1".into(),
            kind: StyleKind::Paragraph,
            based_on: Some(normal_id.clone()),
            next_style: Some(normal_id.clone()),
            character: CharacterProperties {
                font_size_pt: Some(20.0),
                bold: Some(true),
                color: Some(HEADING_COLOR.into()),
                ..CharacterProperties::default()
            },
            paragraph: ParagraphProperties {
                space_before_pt: Some(16.0),
                space_after_pt: Some(6.0),
                keep_with_next: Some(true),
                ..ParagraphProperties::default()
            },
            formatting: BTreeMap::new(),
        });

        let title_id = StyleId::from(STYLE_TITLE);
        styles.insert(title_id.clone(), Style {
            id: title_id,
            name: "Title".into(),
            kind: StyleKind::Paragraph,
            based_on: Some(normal_id),
            next_style: Some(StyleId::from(STYLE_NORMAL)),
            character: CharacterProperties {
                font_size_pt: Some(28.0),
                bold: Some(true),
                ..CharacterProperties::default()
            },
            paragraph: ParagraphProperties {
                alignment: Some(ParagraphAlignment::Center),
                space_after_pt: Some(12.0),
                ..ParagraphProperties::default()
            },
            formatting: BTreeMap::new(),
        });

        let paragraph_style = |id: &str, name: &str, character: CharacterProperties, paragraph: ParagraphProperties| Style {
            id: StyleId::from(id), name: name.into(), kind: StyleKind::Paragraph,
            based_on: Some(StyleId::from(STYLE_NORMAL)), next_style: Some(StyleId::from(STYLE_NORMAL)),
            character, paragraph, formatting: BTreeMap::new(),
        };
        let heading = |size: f32| (
            CharacterProperties { font_size_pt: Some(size), bold: Some(true), color: Some(HEADING_COLOR.into()), ..CharacterProperties::default() },
            ParagraphProperties { space_before_pt: Some(size * 0.6), space_after_pt: Some(4.0), keep_with_next: Some(true), ..ParagraphProperties::default() },
        );
        for (id, name, (character, paragraph)) in [
            (STYLE_HEADING_2, "Heading 2", heading(16.0)),
            (STYLE_HEADING_3, "Heading 3", heading(14.0)),
            (STYLE_SUBTITLE, "Subtitle", (
                CharacterProperties { font_size_pt: Some(14.0), color: Some("#595959".into()), ..CharacterProperties::default() },
                ParagraphProperties { alignment: Some(ParagraphAlignment::Center), space_after_pt: Some(12.0), ..ParagraphProperties::default() })),
            (STYLE_QUOTE, "Quote", (
                CharacterProperties { italic: Some(true), color: Some("#404040".into()), ..CharacterProperties::default() },
                ParagraphProperties { alignment: Some(ParagraphAlignment::Center), space_before_pt: Some(8.0), space_after_pt: Some(8.0), ..ParagraphProperties::default() })),
        ] {
            styles.insert(StyleId::from(id), paragraph_style(id, name, character, paragraph));
        }

        Self { styles }
    }
}

impl StyleSheet {
    pub fn get(&self, id: &StyleId) -> Option<&Style> { self.styles.get(id) }

    /// Resolve a Customize Keyboard assignment. The command name is looked up
    /// at use time so a renamed style does not silently target another ID.
    pub fn style_for_shortcut(&self, chord: &str) -> Option<StyleId> {
        self.styles.values().find_map(|owner| owner.formatting.iter().find_map(|(key, value)| {
            let name = key.strip_prefix("key_keys:")?;
            value.lines().any(|assigned| assigned == chord).then(|| self.id_by_name(name)).flatten()
        }))
    }

    pub fn outline_level(&self, id: Option<&StyleId>) -> Option<u8> {
        let mut current=id;
        let mut seen=BTreeSet::new();
        while let Some(style_id)=current {
            if !seen.insert(style_id.clone()) {return None;}
            let style=self.get(style_id)?;
            if let Some(level) = style.paragraph.outline_level {
                return (1..=9).contains(&level).then_some(level);
            }
            if let Some(choice)=style.formatting.get("par_outline_level") {
                return choice.strip_prefix("Level ").and_then(|value|value.parse::<u8>().ok()).filter(|level|(1..=9).contains(level));
            }
            let compact=style.name.to_ascii_lowercase().replace(' ',"");
            if let Some(level)=compact.strip_prefix("heading").and_then(|value|value.parse::<u8>().ok()).filter(|level|(1..=9).contains(level)) {
                return Some(level);
            }
            if style_id.0==STYLE_HEADING_1 {return Some(1);}
            current=style.based_on.as_ref();
        }
        None
    }

    pub fn paragraph_outline_level(&self, paragraph: &crate::Paragraph) -> Option<u8> {
        match paragraph.properties.outline_level {
            Some(level) => (1..=9).contains(&level).then_some(level),
            None => self.outline_level(paragraph.style.as_ref()),
        }
    }

    pub fn id_by_name(&self, name: &str) -> Option<StyleId> {
        self.styles.values()
            .find(|style| style.name.eq_ignore_ascii_case(name))
            .map(|style| style.id.clone())
    }

    pub fn resolve_character(&self, id: Option<&StyleId>) -> CharacterProperties {
        let Some(id) = id else { return CharacterProperties::default(); };
        let mut visiting = BTreeSet::new();
        self.resolve_character_inner(id, &mut visiting)
    }

    pub fn resolve_paragraph(&self, id: Option<&StyleId>) -> ParagraphProperties {
        let Some(id) = id else { return ParagraphProperties::default(); };
        let mut visiting = BTreeSet::new();
        self.resolve_paragraph_inner(id, &mut visiting)
    }

    /// Format-dialog settings inherited through the same base-style chain.
    /// A setting recorded on the child replaces the corresponding base value.
    pub fn resolve_formatting(&self, id: Option<&StyleId>) -> BTreeMap<String, String> {
        let Some(id) = id else { return BTreeMap::new(); };
        self.resolve_formatting_inner(id, &mut BTreeSet::new())
    }

    fn resolve_formatting_inner(&self, id: &StyleId, visiting: &mut BTreeSet<StyleId>) -> BTreeMap<String, String> {
        if !visiting.insert(id.clone()) { return BTreeMap::new(); }
        let Some(style) = self.styles.get(id) else { visiting.remove(id); return BTreeMap::new(); };
        let mut result = style.based_on.as_ref()
            .map(|parent| self.resolve_formatting_inner(parent, visiting)).unwrap_or_default();
        result.extend(style.formatting.iter().map(|(key, value)| (key.clone(), value.clone())));
        visiting.remove(id);
        result
    }

    pub fn resolve_run(
        &self,
        paragraph_style: Option<&StyleId>,
        character_style: Option<&StyleId>,
        direct: &CharacterProperties,
    ) -> CharacterProperties {
        let mut result = self.resolve_character(paragraph_style);
        if character_style.is_some() { result.overlay(&self.resolve_character(character_style)); }
        result.overlay(direct);
        result.visual_effects = self.resolve_formatting(paragraph_style);
        result.visual_effects.extend(self.resolve_formatting(character_style));
        result
    }

    pub fn resolve_paragraph_with_direct(
        &self,
        paragraph_style: Option<&StyleId>,
        direct: &ParagraphProperties,
    ) -> ParagraphProperties {
        let mut result = self.resolve_paragraph(paragraph_style);
        result.overlay(direct);
        result
    }

    fn resolve_character_inner(&self, id: &StyleId, visiting: &mut BTreeSet<StyleId>) -> CharacterProperties {
        if !visiting.insert(id.clone()) { return CharacterProperties::default(); }
        let Some(style) = self.styles.get(id) else {
            visiting.remove(id);
            return CharacterProperties::default();
        };
        let mut result = style.based_on.as_ref()
            .map(|parent| self.resolve_character_inner(parent, visiting))
            .unwrap_or_default();
        result.overlay(&style.character);
        visiting.remove(id);
        result
    }

    fn resolve_paragraph_inner(&self, id: &StyleId, visiting: &mut BTreeSet<StyleId>) -> ParagraphProperties {
        if !visiting.insert(id.clone()) { return ParagraphProperties::default(); }
        let Some(style) = self.styles.get(id) else {
            visiting.remove(id);
            return ParagraphProperties::default();
        };
        let mut result = style.based_on.as_ref()
            .map(|parent| self.resolve_paragraph_inner(parent, visiting))
            .unwrap_or_default();
        result.overlay(&style.paragraph);
        visiting.remove(id);
        result
    }
}


#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub id: StyleId,
    pub name: String,
    pub kind: StyleKind,
    pub based_on: Option<StyleId>,
    #[serde(default)] pub next_style: Option<StyleId>,
    pub character: CharacterProperties,
    pub paragraph: ParagraphProperties,
    /// Settings from Format dialogs that have no dedicated layout property yet.
    /// Keeping them on the style makes edits survive Save, undo and document IO.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub formatting: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleKind {
    Paragraph,
    Character,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterProperties {
    pub font_family: Option<String>,
    /// Sparse concrete fonts for ASCII, high ANSI, East Asian and complex-script text.
    #[serde(default)] pub font_slots: Option<crate::FontSlots>,
    pub font_size_pt: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub color: Option<String>,
    /// Background of the text advance box; "none" explicitly clears inheritance.
    #[serde(default)] pub highlight: Option<String>,
    pub language: Option<String>,
    /// Explicit run direction from DOCX `w:rtl`; `None` follows Unicode bidi.
    #[serde(default)] pub right_to_left: Option<bool>,
    /// OpenType shaping intent. Glyph IDs/advances are deliberately never
    /// serialized into the document; those are derived by typography.
    pub typography: Option<OpenTypeTypography>,
    /// Resolved Format-dialog settings used by shaping/layout. Style
    /// definitions keep the authoritative values in `Style::formatting`.
    #[serde(skip)] pub visual_effects: BTreeMap<String, String>,
}

impl CharacterProperties {
    /// Applies explicitly specified properties from `other` on top of `self`.
    pub fn overlay(&mut self, other: &Self) {
        if other.font_family.is_some() {
            self.font_family = other.font_family.clone();
            if other.font_slots.is_none() {self.font_slots=None;}
        }
        if let Some(slots)=&other.font_slots {self.font_slots.get_or_insert_with(Default::default).overlay(slots);}
        if other.font_size_pt.is_some() { self.font_size_pt = other.font_size_pt; }
        if other.bold.is_some() { self.bold = other.bold; }
        if other.italic.is_some() { self.italic = other.italic; }
        if other.underline.is_some() { self.underline = other.underline; }
        if other.color.is_some() { self.color = other.color.clone(); }
        if other.highlight.is_some() { self.highlight = other.highlight.clone(); }
        if other.language.is_some() { self.language = other.language.clone(); }
        if other.right_to_left.is_some() { self.right_to_left = other.right_to_left; }
        if other.typography.is_some() { self.typography = other.typography.clone(); }
    }

    pub fn over(mut self, other: &Self) -> Self {
        self.overlay(other);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpenTypeTypography {
    pub kerning: bool,
    /// Metrics is the compatibility default for documents saved before this field existed.
    #[serde(default)]
    pub kerning_mode: KerningMode,
    pub ligatures: LigatureMode,
    pub contextual_ligatures: bool,
    pub contextual_alternates: bool,
    pub small_caps: bool,
    pub oldstyle_figures: bool,
    pub tabular_figures: bool,
    #[serde(default)] pub stylistic_set: Option<u8>,
}

impl Default for OpenTypeTypography {
    fn default() -> Self {
        Self {
            kerning: true,
            kerning_mode: KerningMode::Metrics,
            ligatures: LigatureMode::Standard,
            contextual_ligatures: true,
            contextual_alternates: true,
            small_caps: false,
            oldstyle_figures: false,
            tabular_figures: false,
            stylistic_set: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KerningMode {
    #[default]
    Metrics,
    Optical,
    Off,
}


#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LigatureMode {
    None,
    Standard,
    Discretionary,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParagraphProperties {
    /// Direct outline level: 1–9 includes this paragraph, 0 excludes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_level: Option<u8>,
    /// Contents entries resolve their page-number tab against the actual column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents_leader: Option<TabLeader>,
    pub alignment: Option<ParagraphAlignment>,
    pub space_before_pt: Option<f32>,
    pub space_after_pt: Option<f32>,
    pub line_spacing: Option<f32>,
    pub first_line_indent_mm: Option<f32>,
    pub left_indent_mm: Option<f32>,
    pub right_indent_mm: Option<f32>,
    pub keep_with_next: Option<bool>,
    pub hyphenation: Option<bool>,
    pub hanging_punctuation: Option<bool>,
    /// Maximum symmetric glyph expansion in per-mille. This is intentionally
    /// tiny (e.g. 15 = 1.5%) and is for TeX-like justification polish.
    pub glyph_expansion_per_mille: Option<u16>,
    /// Logical paragraph direction. Auto delegates base-direction resolution to
    /// the Unicode BiDi algorithm; explicit LTR/RTL mirrors Word's paragraph
    /// direction controls.
    pub direction: Option<ParagraphDirection>,
    /// Reference into Document::lists plus the active nesting level.
    pub list_id: Option<NodeId>,
    pub list_level: Option<u8>,
    pub tabs: Option<Vec<TabStop>>,
    #[serde(default)] pub default_tab_mm: Option<f32>,
    pub shading: Option<String>,
    #[serde(default)] pub borders: Option<BorderSet>,
    pub keep_lines_together: Option<bool>,
    pub page_break_before: Option<bool>,
    pub widow_control: Option<bool>,
}

impl ParagraphProperties {
    pub fn overlay(&mut self, other: &Self) {
        if other.outline_level.is_some() { self.outline_level = other.outline_level; }
        if other.contents_leader.is_some() { self.contents_leader = other.contents_leader; }
        if other.alignment.is_some() { self.alignment = other.alignment; }
        if other.space_before_pt.is_some() { self.space_before_pt = other.space_before_pt; }
        if other.space_after_pt.is_some() { self.space_after_pt = other.space_after_pt; }
        if other.line_spacing.is_some() { self.line_spacing = other.line_spacing; }
        if other.first_line_indent_mm.is_some() { self.first_line_indent_mm = other.first_line_indent_mm; }
        if other.left_indent_mm.is_some() { self.left_indent_mm = other.left_indent_mm; }
        if other.right_indent_mm.is_some() { self.right_indent_mm = other.right_indent_mm; }
        if other.keep_with_next.is_some() { self.keep_with_next = other.keep_with_next; }
        if other.hyphenation.is_some() { self.hyphenation = other.hyphenation; }
        if other.hanging_punctuation.is_some() { self.hanging_punctuation = other.hanging_punctuation; }
        if other.glyph_expansion_per_mille.is_some() { self.glyph_expansion_per_mille = other.glyph_expansion_per_mille; }
        if other.direction.is_some() { self.direction = other.direction; }
        if other.list_id.is_some() { self.list_id = other.list_id.clone(); }
        if other.list_level.is_some() { self.list_level = other.list_level; }
        if other.tabs.is_some() { self.tabs = other.tabs.clone(); }
        if other.default_tab_mm.is_some() { self.default_tab_mm = other.default_tab_mm; }
        if other.shading.is_some() { self.shading = other.shading.clone(); }
        if other.borders.is_some() { self.borders = other.borders.clone(); }
        if other.keep_lines_together.is_some() { self.keep_lines_together = other.keep_lines_together; }
        if other.page_break_before.is_some() { self.page_break_before = other.page_break_before; }
        if other.widow_control.is_some() { self.widow_control = other.widow_control; }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParagraphDirection { Auto, LeftToRight, RightToLeft }

/// Standard half-inch automatic tabs, expressed in millimetres.
/// Standard half-inch automatic spacing, shared by layout and tab controls.
pub const DEFAULT_TAB_INTERVAL_MM: f32 = 12.7;
/// Bound the editable tab set per paragraph.
pub const MAX_TAB_STOPS: usize = 64;
/// Stops closer than this edit tolerance are treated as one position.
pub const TAB_POSITION_EPSILON_MM: f32 = 0.005;
/// Largest supported explicit stop, measured from the text margin.
pub const MAX_TAB_POSITION_MM: f32 = 1000.0;
/// Supported range for the automatic tab interval.
pub const MIN_TAB_INTERVAL_MM: f32 = 1.0;
pub const MAX_TAB_INTERVAL_MM: f32 = 600.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabStop {
    pub position_mm: f32,
    pub alignment: TabAlignment,
    pub leader: TabLeader,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabAlignment { Left, Center, Right, Decimal, Bar }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabLeader { #[default] None, Dots, Dashes, Line, Heavy, MiddleDots }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParagraphAlignment {
    Left,
    Center,
    Right,
    Justify,
}

#[cfg(test)] #[path = "../tests/unit/style.rs"] mod tests;
