//! Redefining a named style is distinct from applying it to paragraphs.
//! Keep definitions and refreshes of the active sample in one Undo record.
use super::*;
use model::{ParagraphProperties,StyleKind,StyleSheet};

impl EditorState {
    /// Copy uniform selection formatting into a style without changing its
    /// identity, inheritance, following style, shortcuts or paragraph membership.
    /// Mixed properties keep the style's existing value.
    pub fn update_style_from_selection(&mut self,id:&StyleId)->Result<(),String> {
        let selection=self.selection.as_ref().ok_or("Select text or place the caret first")?;
        let mut style=self.document.styles.get(id).cloned().ok_or("Style not found")?;
        let mut characters=Vec::new();
        if selection.is_collapsed() {
            let mut properties=self.formatting_summary().character;
            properties.language=self.typing_properties.language.clone();
            characters.push(sample_character(properties));
        } else {
            for (node,start,end) in self.selected_paragraph_spans() {
                let paragraph=self.paragraph(&node).ok_or("Paragraph not found")?;
                collect_characters(&slice_content(&paragraph.content,start,end)?,paragraph.style.as_ref(),&self.document.styles,&mut characters);
            }
        }
        if let Some(first)=characters.first() {
            macro_rules! common {($($field:ident),*)=>{$(if characters.iter().all(|c|c.$field==first.$field) {style.character.$field=first.$field.clone();})*};}
            common!(font_family,font_slots,font_size_pt,bold,italic,underline,color,highlight,typography,right_to_left);
            // Inherited Normal English is not an explicit proofing choice.
            if first.language.is_some() && characters.iter().all(|c|c.language==first.language) {style.character.language=first.language.clone();}
        }
        if style.kind==StyleKind::Paragraph {
            let paragraphs=self.selected_paragraph_ids().into_iter().filter_map(|node|self.paragraph(&node))
                .map(|p|sample_paragraph(self.document.styles.resolve_paragraph_with_direct(p.style.as_ref(),&p.properties))).collect::<Vec<_>>();
            if let Some(first)=paragraphs.first() {
                macro_rules! common {($($field:ident),*)=>{$(if paragraphs.iter().all(|p|p.$field==first.$field) {style.paragraph.$field=first.$field.clone();})*};}
                paragraph_fields!(common);
            }
        }
        self.put_style(style)
    }

    pub(super) fn style_selection_mutations(&self,before:&StyleSheet,after:&StyleSheet)->Result<Vec<Mutation>,String> {
        let collapsed=self.selection.as_ref().is_some_and(Selection::is_collapsed);
        let mut changes=Vec::new();
        for (node,start,end) in self.paragraph_formatting_spans() {
            let original=self.paragraph(&node).cloned().ok_or("Paragraph not found")?;
            let mut updated=original.clone();
            let old=before.resolve_paragraph(original.style.as_ref());
            let new=after.resolve_paragraph(original.style.as_ref());
            macro_rules! clear {($($field:ident),*)=>{$(if old.$field!=new.$field {updated.properties.$field=None;})*};}
            paragraph_fields!(clear);
            let (start,end)=if collapsed {(0,paragraph_flat_text(&original).len())}else{(start,end)};
            let (left,suffix)=split_content(&original.content,end)?;
            let (mut prefix,mut selected)=split_content(&left,start)?;
            refresh_runs(&mut selected,original.style.as_ref(),before,after);
            prefix.extend(selected);prefix.extend(suffix);updated.content=normalized_content(prefix).into();
            if updated!=original {changes.push(Mutation::ParagraphChanged{id:node,before:original,after:updated});}
        }
        Ok(changes)
    }

    pub(super) fn refreshed_style_typing(&self,before:&StyleSheet,after:&StyleSheet)->CharacterProperties {
        let paragraph=self.focus_position().and_then(|position|self.paragraph(&position.node_id));
        let mut run=TextRun::plain("");run.style=self.typing_style.clone();run.properties=self.typing_properties.clone();
        let mut items=vec![Inline::Text(run)];
        refresh_runs(&mut items,paragraph.and_then(|p|p.style.as_ref()),before,after);
        let Inline::Text(run)=items.remove(0) else{unreachable!()};run.properties
    }
}

macro_rules! paragraph_fields {($action:ident)=>{$action!(alignment,space_before_pt,space_after_pt,line_spacing,
    first_line_indent_mm,left_indent_mm,right_indent_mm,keep_with_next,hyphenation,hanging_punctuation,
    glyph_expansion_per_mille,direction,list_id,list_level,tabs,default_tab_mm,shading,borders,
    keep_lines_together,page_break_before,widow_control);};}
use paragraph_fields;

fn refresh_runs(items:&mut [Inline],paragraph:Option<&StyleId>,before:&StyleSheet,after:&StyleSheet) {
    for item in items {match item {
        Inline::Text(run)=>{
            let old=before.resolve_run(paragraph,run.style.as_ref(),&Default::default());
            let new=after.resolve_run(paragraph,run.style.as_ref(),&Default::default());
            macro_rules! clear {($($field:ident),*)=>{$(if old.$field!=new.$field {run.properties.$field=None;})*};}
            clear!(font_family,font_slots,font_size_pt,bold,italic,underline,color,highlight,typography,right_to_left);
        }
        Inline::Hyperlink(link)=>refresh_runs(&mut link.content,paragraph,before,after),
        _=>{}
    }}
}

fn collect_characters(items:&[Inline],paragraph:Option<&StyleId>,sheet:&StyleSheet,out:&mut Vec<CharacterProperties>) {
    for item in items {match item {
        Inline::Text(run) if !run.text.is_empty()=>{
            let mut properties=sheet.resolve_run(paragraph,run.style.as_ref(),&run.properties);
            properties.language=run.properties.language.clone();
            out.push(sample_character(properties));
        }
        Inline::Hyperlink(link)=>collect_characters(&link.content,paragraph,sheet,out),
        _=>{}
    }}
}
fn sample_character(mut c:CharacterProperties)->CharacterProperties {
    c.font_family.get_or_insert_with(||"Linux Libertine G".into());c.font_size_pt.get_or_insert(11.0);
    c.bold.get_or_insert(false);c.italic.get_or_insert(false);c.underline.get_or_insert(false);
    c.color.get_or_insert_with(||"#000000".into());c.highlight.get_or_insert_with(||"none".into());
    c.typography.get_or_insert_with(Default::default);c
}
fn sample_paragraph(mut p:ParagraphProperties)->ParagraphProperties {
    p.alignment.get_or_insert(model::ParagraphAlignment::Left);p.direction.get_or_insert(model::ParagraphDirection::Auto);
    p.space_before_pt.get_or_insert(0.0);p.space_after_pt.get_or_insert(0.0);p.line_spacing.get_or_insert(1.2);
    p.first_line_indent_mm.get_or_insert(0.0);p.left_indent_mm.get_or_insert(0.0);p.right_indent_mm.get_or_insert(0.0);
    p.keep_with_next.get_or_insert(false);p.keep_lines_together.get_or_insert(false);p.page_break_before.get_or_insert(false);p.widow_control.get_or_insert(true);
    p.tabs.get_or_insert_with(Vec::new);p.default_tab_mm.get_or_insert(model::DEFAULT_TAB_INTERVAL_MM);p
}

#[cfg(test)] #[path = "../tests/unit/style_selection.rs"] mod tests;
