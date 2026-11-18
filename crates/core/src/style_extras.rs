use crate::{BorderSet, ListDefinition, NodeId, StyleId, StyleSheet, Table};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableStyle {
    pub name: String,
    pub body_fill: Option<String>,
    pub header_fill: Option<String>,
    pub band_fill: Option<String>,
    pub borders: Option<BorderSet>,
}

impl TableStyle {
    pub fn fill_for_row(&self, row: usize, first_row: bool, banding: bool) -> Option<&str> {
        if first_row && row == 0 && self.header_fill.is_some() {
            return self.header_fill.as_deref();
        }
        if banding && row % 2 == usize::from(first_row) && self.band_fill.is_some() {
            return self.band_fill.as_deref();
        }
        self.body_fill.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedListStyle {
    pub name: String,
    pub definition: ListDefinition,
    /// Zero-based numbering level to paragraph style.
    pub headings: BTreeMap<u8, StyleId>,
}

impl StyleSheet {
    pub fn table_style(&self, table: &Table) -> Option<&TableStyle> {
        self.table_styles.get(table.properties.style.as_ref()?)
    }

    pub fn table_style_fill(&self, table: &Table, row: usize) -> Option<&str> {
        self.table_style(table)?.fill_for_row(row,
            table.properties.style_first_row.unwrap_or(true), table.properties.style_row_banding.unwrap_or(true))
    }

    pub fn table_borders<'a>(&'a self, table: &'a Table) -> Option<&'a BorderSet> {
        let styled = self.table_style(table).and_then(|style| style.borders.as_ref());
        let direct = table.properties.borders.as_ref();
        if table.properties.style_overrides_table_borders == Some(true) { styled.or(direct) } else { direct.or(styled) }
    }

    pub fn list_definition<'a>(
        &'a self,
        lists: &'a BTreeMap<NodeId, ListDefinition>,
        id: &NodeId,
    ) -> Option<&'a ListDefinition> {
        self.list_styles
            .get(id)
            .map(|style| &style.definition)
            .or_else(|| lists.get(id))
    }

    pub fn linked_list(&self, style: &StyleId) -> Option<(&NodeId, u8)> {
        self.list_styles.iter().find_map(|(id, list)| {
            list.headings
                .iter()
                .find_map(|(&level, heading)| (heading == style).then_some((id, level)))
        })
    }
}
