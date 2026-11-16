//! Word's Columns command: a column layout (count, widths, spacing, line
//! between) applied to the whole document, to the section at the caret, or
//! from the caret onwards through a new section. Each is one undo step.
use super::*;
use model::{ColumnSpec, SectionProperties, SectionStart};

/// Most columns the Columns dialog offers.
pub const MAX_COLUMNS: u16 = 12;
/// Narrowest column allowed, as in Word (0.5 in).
pub const MIN_COLUMN_WIDTH_MM: f32 = 12.7;
/// Word's default space between columns (0.5 in).
pub const DEFAULT_COLUMN_SPACING_MM: f32 = 12.7;

/// The column choices of Word's Columns menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnPreset { One, Two, Three, Left, Right }

impl ColumnPreset {
    pub const ALL: [Self; 5] = [Self::One, Self::Two, Self::Three, Self::Left, Self::Right];

    pub const fn label(self) -> &'static str {
        match self {
            Self::One => "One",
            Self::Two => "Two",
            Self::Three => "Three",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

/// Where a column layout applies (the Columns dialog's "Apply to").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnScope {
    WholeDocument,
    ThisSection,
    /// A new section starts at the caret; `new_column` starts it at the top
    /// of the next column (Word's "Start new column").
    ThisPointForward { new_column: bool },
}

/// A section's columns as the Columns dialog edits them.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnLayout {
    pub count: u16,
    /// Space between columns when they are equal.
    pub spacing_mm: f32,
    /// Width and spacing of each column; empty when the columns are equal.
    pub columns: Vec<ColumnSpec>,
    pub line_between: bool,
}

impl ColumnLayout {
    pub fn equal(count: u16, spacing_mm: f32) -> Self {
        Self { count: count.max(1), spacing_mm, columns: Vec::new(), line_between: false }
    }

    /// The layout of a preset for a section `text_width_mm` wide. Left and
    /// Right put a column a third as wide beside one twice its width.
    pub fn preset(preset: ColumnPreset, text_width_mm: f32) -> Self {
        let spacing = DEFAULT_COLUMN_SPACING_MM;
        match preset {
            ColumnPreset::One => Self::equal(1, spacing),
            ColumnPreset::Two => Self::equal(2, spacing),
            ColumnPreset::Three => Self::equal(3, spacing),
            ColumnPreset::Left | ColumnPreset::Right => {
                let narrow = (text_width_mm - spacing) / 3.0;
                let (first, second) = if preset == ColumnPreset::Left { (narrow, narrow * 2.0) } else { (narrow * 2.0, narrow) };
                Self { count: 2, spacing_mm: spacing, line_between: false, columns: vec![
                    ColumnSpec { width_mm: first, spacing_mm: spacing },
                    ColumnSpec { width_mm: second, spacing_mm: 0.0 },
                ] }
            }
        }
    }

    /// The preset this layout matches, if any (to highlight it in the menu).
    pub fn matching_preset(&self, text_width_mm: f32) -> Option<ColumnPreset> {
        ColumnPreset::ALL.into_iter().find(|&preset| {
            let candidate = Self::preset(preset, text_width_mm);
            candidate.count == self.count && candidate.columns.len() == self.columns.len()
                && candidate.columns.iter().zip(&self.columns).all(|(a, b)| (a.width_mm - b.width_mm).abs() < 1.0)
        })
    }

    pub fn of(properties: &SectionProperties) -> Self {
        let equal = properties.has_equal_columns();
        Self {
            count: if equal { properties.column_count.max(1) } else { properties.columns.len() as u16 },
            spacing_mm: properties.column_gap_mm,
            columns: if equal { Vec::new() } else { properties.columns.clone() },
            line_between: properties.column_separator,
        }
    }

    /// Width of each column and the space after it, for display. Equal
    /// columns share the text width evenly.
    pub fn resolved(&self, text_width_mm: f32) -> Vec<ColumnSpec> {
        if !self.columns.is_empty() { return self.columns.clone(); }
        let count = self.count.max(1);
        let width = (text_width_mm - self.spacing_mm * f32::from(count - 1)) / f32::from(count);
        (0..count).map(|index| ColumnSpec { width_mm: width, spacing_mm: if index + 1 < count { self.spacing_mm } else { 0.0 } }).collect()
    }

    /// Checks the layout fits a section `text_width_mm` wide.
    pub fn validate(&self, text_width_mm: f32) -> Result<(), String> {
        if !(1..=MAX_COLUMNS).contains(&self.count) {
            return Err(format!("Choose between 1 and {MAX_COLUMNS} columns."));
        }
        if !self.columns.is_empty() && self.columns.len() != usize::from(self.count) {
            return Err("Give every column a width.".into());
        }
        let columns = self.resolved(text_width_mm);
        if columns.iter().any(|column| !column.width_mm.is_finite() || column.width_mm < MIN_COLUMN_WIDTH_MM - 0.05) {
            return Err(format!("Columns must be at least {:.2} cm wide.", MIN_COLUMN_WIDTH_MM / 10.0));
        }
        if columns.iter().any(|column| !column.spacing_mm.is_finite() || column.spacing_mm < 0.0) {
            return Err("Spacing between columns cannot be negative.".into());
        }
        let total: f32 = columns.iter().map(|column| column.width_mm + column.spacing_mm).sum();
        if total > text_width_mm + 0.5 {
            return Err("The columns and spacing are wider than the page's text area.".into());
        }
        Ok(())
    }

    fn apply_to(&self, properties: &mut SectionProperties) {
        properties.column_count = self.count.max(1);
        properties.column_gap_mm = self.spacing_mm;
        properties.columns = self.columns.clone();
        properties.column_separator = self.line_between && self.count > 1;
    }
}

impl EditorState {
    /// The column layout of the section at the caret.
    pub fn column_layout_at_caret(&self) -> Option<ColumnLayout> {
        self.section_properties_at_caret().map(|properties| ColumnLayout::of(&properties))
    }

    pub fn apply_column_layout(&mut self, layout: &ColumnLayout, scope: ColumnScope) -> Result<(), String> {
        let properties = self.section_properties_at_caret().ok_or("Place the caret in the document first")?;
        layout.validate(properties.text_width_mm())?;
        match scope {
            ColumnScope::ThisSection => self.set_section_columns(layout),
            ColumnScope::WholeDocument => self.set_document_columns(layout),
            ColumnScope::ThisPointForward { new_column } => {
                let depth = self.undo.len();
                self.insert_section_break(if new_column { SectionStart::NewColumn } else { SectionStart::Continuous })?;
                self.set_section_columns(layout)?;
                self.group_undo_since(depth, "Columns");
                Ok(())
            }
        }
    }

    fn set_section_columns(&mut self, layout: &ColumnLayout) -> Result<(), String> {
        let mut properties = self.section_properties_at_caret().ok_or("No active section")?;
        layout.apply_to(&mut properties);
        self.set_section_properties(properties)
    }

    fn set_document_columns(&mut self, layout: &ColumnLayout) -> Result<(), String> {
        let mut mutations = Vec::new();
        for section in &mut self.document.sections {
            let before = section.clone();
            layout.apply_to(&mut section.properties);
            if section.properties != before.properties {
                mutations.push(Mutation::SectionChanged { section_id: section.id.clone(), before, after: section.clone() });
            }
        }
        if mutations.is_empty() { return Ok(()); }
        let selection = self.selection.clone();
        self.commit("Columns", mutations, selection, None)
    }
}

#[cfg(test)] #[path = "../tests/unit/columns.rs"] mod tests;
