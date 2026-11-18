//! The style under the mouse is the target; the document selection is the sample.
use super::*;
use wxdragon::geometry::Point;

const UPDATE_ID:i32=32_180;
const CREATE_ID: i32 = 32_181;
const FIND_ID: i32 = 32_182;

impl Ribbon {
    pub(super) fn show_style_context(&self,id:model::StyleId,at:Point) {
        if self.mode.get()==ViewMode::Typst {return;}
        let (label,enabled,revision,selection)={
            let state=self.state.borrow();
            let Some(style)=state.editor.document.styles.get(&id) else{return;};
            (format!("Update {} to Match Selection",crate::i18n::style_name(style)),state.editor.selection.is_some(),state.editor.revision,state.editor.selection.clone())
        };
        let mut menu=Menu::builder().append_item(UPDATE_ID,&crate::i18n::text(&label),
            &crate::i18n::text("Redefine this style using the selection's formatting. Updates all text using this style."))
            .append_item(CREATE_ID, &crate::i18n::text("New from Selection…"), "")
            .append_item(FIND_ID, &crate::i18n::text("Find and Replace…"), "").build();
        menu.enable_item(CREATE_ID, enabled);
        menu.enable_item(UPDATE_ID,enabled);
        self.style_context.replace(Some((id,revision,selection)));
        self.ui.main_frame.popup_menu(&mut menu,Some(at));
        self.style_context.take();
    }

    pub(super) fn handle_style_context(&self,id:i32)->bool {
        if ![UPDATE_ID, CREATE_ID, FIND_ID].contains(&id) { return false; }
        let Some((style,revision,selection))=self.style_context.borrow_mut().take() else{return true;};
        let result = match id {
            CREATE_ID => super::style_tools::new_style(self, &self.ui.main_frame, true),
            FIND_ID => super::style_tools::find_replace(self, &self.ui.main_frame, &style).map(|target| {
                if let Some(target) = target { super::style_tools::navigate(self, target); }
            }),
            _ => self.edit(|editor|{
            if editor.revision!=revision || editor.selection!=selection {return Err("The selection changed; open the style menu again".into());}
            editor.update_style_from_selection(&style)
        }) };
        if let Err(error)=result {self.state.borrow_mut().action_status=Some(error);}
        self.styles_revision.set(None);self.refresh_styles_pane();
        if let Some(gallery)=self.style_gallery.borrow().as_ref() {gallery.refresh();}
        true
    }
}
