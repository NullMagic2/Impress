//! Style authoring is a semantic, validated, undoable operation. Display names
//! may change; stable IDs keep every paragraph and character-style use linked.
use super::*;
use model::{Style, StyleKind, StyleSheet, ParagraphProperties, TabStop};
use std::collections::BTreeSet;

pub fn validate_styles(sheet:&StyleSheet)->Result<(),String>{
    if sheet.styles.len()>4096{return Err("A document may contain at most 4096 styles".into());}
    let mut names=BTreeSet::new();
    for (id,style) in &sheet.styles{
        if &style.id!=id || style.name.trim().is_empty() || style.name.len()>256 || style.name.contains(['\0','\r','\n']){
            return Err("Style IDs must agree and names must be 1–256 bytes on one line".into());}
        if !names.insert(style.name.trim().to_lowercase()){return Err("Style names must be unique (ignoring case)".into());}
        let mut seen=BTreeSet::new();let mut current=Some(id);
        while let Some(k)=current{
            if !seen.insert(k.clone()){return Err(format!("Style inheritance cycle at {}",style.name));}
            let parent=sheet.get(k).ok_or("A base style does not exist")?;
            if parent.kind!=style.kind{return Err("Base style must have the same paragraph/character kind".into());}
            current=parent.based_on.as_ref();
        }
        if let Some(next)=&style.next_style{if style.kind!=StyleKind::Paragraph || sheet.get(next).is_none_or(|target|target.kind!=StyleKind::Paragraph){return Err("Following paragraph style must be a paragraph style in this document".into());}}
        if style.character.font_size_pt.is_some_and(|v|!v.is_finite()||!(1.0..=400.0).contains(&v)){return Err("Font size must be 1–400 pt".into());}
        if style.character.font_family.as_ref().is_some_and(|s|s.trim().is_empty()||s.len()>256||s.contains('\0')){return Err("Invalid font family".into());}
        for (name,value) in [("font color",&style.character.color),("highlight",&style.character.highlight)]{
            if let Some(s)=value{if !(name=="highlight"&&s=="none") && !(s.len()==7&&s.starts_with('#')&&s[1..].bytes().all(|b|b.is_ascii_hexdigit())){return Err(format!("Invalid {name}; use #RRGGBB"));}}
        }
        let p=&style.paragraph;
        for v in [p.space_before_pt,p.space_after_pt,p.first_line_indent_mm,p.left_indent_mm,p.right_indent_mm,p.line_spacing].into_iter().flatten(){if !v.is_finite()||v.abs()>2000.0{return Err("Invalid style measurement".into());}}
        if p.line_spacing.is_some_and(|v|!(0.5..=10.0).contains(&v)){return Err("Line spacing must be 0.5–10".into());}
        if style.kind==StyleKind::Character && p!=&ParagraphProperties::default(){return Err("A character style cannot contain paragraph properties".into());}
        if let Some(tabs)=&p.tabs {validate_tabs(tabs)?;}
    }Ok(())
}
fn validate_tabs(tabs:&[TabStop])->Result<(),String>{
    if tabs.len()>model::MAX_TAB_STOPS{return Err("At most 64 tab stops are allowed".into());}
    let mut previous=-1.0;
    for tab in tabs{
        if !tab.position_mm.is_finite() || !(0.0..=model::MAX_TAB_POSITION_MM).contains(&tab.position_mm) || tab.position_mm<=previous{return Err("Tab positions must be unique, increasing and between 0 and 1000 mm".into());}
        previous=tab.position_mm;
    }Ok(())
}

impl EditorState{
    pub fn put_style(&mut self,style:Style)->Result<(),String>{
        self.put_styles(vec![style])
    }
    /// Commit a coordinated style edit as one undoable change (used when the
    /// keyboard dialog changes assignments for several named styles).
    pub fn put_styles(&mut self, styles: Vec<Style>) -> Result<(), String> {
        self.put_styles_inner(styles,false)
    }
    /// Style-editor edits also refresh changed formatting on the active sample.
    pub fn put_styles_with_selection(&mut self, styles: Vec<Style>) -> Result<(), String> {
        self.put_styles_inner(styles,true)
    }
    fn put_styles_inner(&mut self, styles: Vec<Style>, refresh_selection:bool) -> Result<(), String> {
        let before=self.document.styles.clone();let mut after=before.clone();
        for style in styles {
            if let Some(old)=before.get(&style.id){if old.kind!=style.kind{return Err("An existing style's kind cannot change".into());}}
            after.styles.insert(style.id.clone(),style);
        }
        validate_styles(&after)?;
        if after==before{return Ok(());}
        let mut changes=vec![Mutation::StylesChanged{before:before.clone(),after:after.clone()}];
        if refresh_selection {changes.extend(self.style_selection_mutations(&before,&after)?);}
        let mut staged=self.clone();
        for change in &changes {apply_mutation(&mut staged.document,change,true)?;}
        staged.commit("Modify style",changes,self.selection.clone(),None)?;
        if refresh_selection {staged.typing_properties=self.refreshed_style_typing(&before,&after);}
        *self=staged;Ok(())
    }
    pub fn remove_unused_style(&mut self,id:&StyleId)->Result<(),String>{
        if [STYLE_NORMAL,STYLE_HEADING_1,STYLE_TITLE].contains(&id.0.as_str()){return Err("Built-in Ribbon styles cannot be deleted".into());}
        fn uses(items:&[Inline],id:&StyleId)->bool{items.iter().any(|i|match i{Inline::Text(t)=>t.style.as_ref()==Some(id),Inline::Hyperlink(h)=>uses(&h.content,id),_=>false})}
        if self.typing_style.as_ref()==Some(id) || self.document.styles.styles.values().any(|s|s.based_on.as_ref()==Some(id)||s.next_style.as_ref()==Some(id))
            || model::document_paragraphs(&self.document).iter().any(|p|p.style.as_ref()==Some(id)||uses(&p.content,id)){
            return Err("This style is in use. Apply another style and change dependent base styles before deleting it.".into());}
        let before=self.document.styles.clone();let mut after=before.clone();after.styles.remove(id).ok_or("Style not found")?;
        self.document.styles=after.clone();self.commit("Delete style",vec![Mutation::StylesChanged{before,after}],self.selection.clone(),None)
    }
    pub fn apply_named_style(&mut self,id:&StyleId,clear_direct:bool)->Result<(),String>{
        let kind=self.document.styles.get(id).ok_or("Style not found")?.kind;
        let before_selection=self.selection.clone().ok_or("Select text or place the caret first")?;
        if kind==StyleKind::Character && before_selection.is_collapsed() && self.table_formatting_spans.is_none(){
            self.typing_style=Some(id.clone());if clear_direct{self.typing_properties=CharacterProperties::default();}self.break_coalescing();return Ok(());
        }
        fn named(items:&mut[Inline],id:&StyleId,clear:bool){for i in items{match i{Inline::Text(t)=>{t.style=Some(id.clone());if clear{t.properties=Default::default();}},Inline::Hyperlink(h)=>named(&mut h.content,id,clear),_=>{}}}}
        fn clear_runs(items:&mut[Inline]){for i in items{match i{Inline::Text(t)=>t.properties=Default::default(),Inline::Hyperlink(h)=>clear_runs(&mut h.content),_=>{}}}}
        let mut changes=Vec::new();
        let spans=if kind==StyleKind::Paragraph {self.paragraph_formatting_spans()} else {self.character_formatting_spans()};
        for (node,a,b) in spans{
            let before=self.paragraph(&node).cloned().ok_or("Paragraph not found")?;let mut after=before.clone();
            match kind{
                StyleKind::Paragraph=>{after.style=Some(id.clone());if clear_direct{after.properties=Default::default();clear_runs(&mut after.content);}},
                StyleKind::Character=>{let (mut prefix, mut selected, suffix)=self.split_for_formatting(&before,a,b)?;named(&mut selected,id,clear_direct);prefix.extend(selected);prefix.extend(suffix);after.content=self.finish_formatting_content(prefix).into();}
            }
            if after!=before{changes.push(Mutation::ParagraphChanged{id:node,before,after});}
        }
        let mut staged=self.clone();for m in &changes{apply_mutation(&mut staged.document,m,true)?;}
        staged.commit("Apply named style",changes,Some(before_selection),None)?;
        if kind==StyleKind::Paragraph && clear_direct {
            staged.reset_typing_properties_from_caret();
        }
        *self=staged;Ok(())
    }
    pub fn clear_direct_formatting(&mut self)->Result<(),String>{
        if self.table_formatting_spans.is_none() && self.selection.as_ref().is_some_and(Selection::is_collapsed){self.typing_properties=Default::default();return Ok(());}
        fn clear(items:&mut[Inline]){for i in items{match i{Inline::Text(t)=>t.properties=Default::default(),Inline::Hyperlink(h)=>clear(&mut h.content),_=>{}}}}
        let selection=self.selection.clone();let mut changes=Vec::new();
        for(id,a,b)in self.character_formatting_spans(){let before=self.paragraph(&id).cloned().ok_or("Paragraph not found")?;let mut after=before.clone();
            let (mut prefix, mut selected, suffix)=self.split_for_formatting(&before,a,b)?;clear(&mut selected);prefix.extend(selected);prefix.extend(suffix);after.content=self.finish_formatting_content(prefix).into();
            if a==0&&b==paragraph_flat_text(&before).len(){after.properties=Default::default();}
            changes.push(Mutation::ParagraphChanged{id,before,after});}
        let mut staged=self.clone();for m in &changes{apply_mutation(&mut staged.document,m,true)?;}staged.commit("Clear direct formatting",changes,selection,None)?;*self=staged;Ok(())
    }
    /// Change tab stops and the document's automatic interval in one undo record.
    /// Omitted fields retain mixed paragraph settings rather than flattening them.
    pub fn set_tab_settings(&mut self, tabs: Option<Vec<TabStop>>, default_mm: Option<f32>) -> Result<(), String> {
        let mut tabs = tabs;
        if let Some(stops) = &mut tabs {
            stops.sort_by(|a, b| a.position_mm.total_cmp(&b.position_mm));
            validate_tabs(stops)?;
        }
        if default_mm.is_some_and(|value| !value.is_finite() || !(model::MIN_TAB_INTERVAL_MM..=model::MAX_TAB_INTERVAL_MM).contains(&value)) {
            return Err("Default tabs must be between 1 and 600 mm.".into());
        }
        let mut mutations = Vec::new();
        for id in self.selected_paragraph_ids() {
            let before = self.paragraph(&id).cloned().ok_or("Paragraph not found")?;
            let mut after = before.clone();
            if let Some(stops) = &tabs { after.properties.tabs = Some(stops.clone()); }
            if let Some(value) = default_mm { after.properties.default_tab_mm = Some(value); }
            if after != before { mutations.push(Mutation::ParagraphChanged { id, before, after }); }
        }
        if let Some(value) = default_mm {
            let before = self.document.styles.clone();
            let mut after = before.clone();
            let normal = after.styles.get_mut(&StyleId::from(model::STYLE_NORMAL)).ok_or("Normal style not found")?;
            normal.paragraph.default_tab_mm = Some(value);
            if after != before { mutations.push(Mutation::StylesChanged { before, after }); }
        }
        let mut staged = self.clone();
        for mutation in &mutations { apply_mutation(&mut staged.document, mutation, true)?; }
        staged.commit("Tab stops", mutations, self.selection.clone(), None)?;
        *self = staged;
        Ok(())
    }

    /// Add, move, or remove one stop without replacing other selected paragraphs' stops.
    pub fn edit_tab_stop(&mut self, old_mm: Option<f32>, stop: Option<TabStop>) -> Result<(), String> {
        let mut changes = Vec::new();
        for id in self.selected_paragraph_ids() {
            let before = self.paragraph(&id).cloned().ok_or("Paragraph not found")?;
            let mut after = before.clone();
            let mut tabs = self.document.styles.resolve_paragraph_with_direct(before.style.as_ref(), &before.properties).tabs.unwrap_or_default();
            tabs.retain(|tab| !old_mm.is_some_and(|position| (position - tab.position_mm).abs() < model::TAB_POSITION_EPSILON_MM)
                && !stop.as_ref().is_some_and(|new| (new.position_mm - tab.position_mm).abs() < model::TAB_POSITION_EPSILON_MM));
            if let Some(stop) = &stop { tabs.push(stop.clone()); }
            tabs.sort_by(|a,b|a.position_mm.total_cmp(&b.position_mm));
            validate_tabs(&tabs)?;
            after.properties.tabs = Some(tabs);
            if after != before { changes.push(Mutation::ParagraphChanged { id, before, after }); }
        }
        let mut staged = self.clone();
        for change in &changes { apply_mutation(&mut staged.document, change, true)?; }
        staged.commit("Ruler tab stop", changes, self.selection.clone(), None)?;
        *self = staged;
        Ok(())
    }

    pub fn set_tab_stops(&mut self, tabs: Vec<TabStop>) -> Result<(), String> {
        self.set_tab_settings(Some(tabs), None)
    }
}
