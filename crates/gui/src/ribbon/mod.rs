//! Native tabbed Ribbon and window composition. The semantic editor owns edits;
//! this module owns commands, native widgets, UI state and embedded icon images.
mod interface_language;
mod zoom_dialog;
mod dialog_actions;
mod document_info;
mod input_dialog;
mod header_footer_gallery;
mod art;
mod font_preview;
mod dropdown;
mod gallery_popup;
mod style_gallery;
mod style_context;
mod tab_strip;
mod tab_ruler;
mod ruler_geometry;
mod ruler_indents;
mod table_bar;
mod bar_paint;
mod picture_bar;
mod picture_shapes;
mod table_picker;
mod symbol_picker;
mod inline_equation;
mod comment_notes;
pub(crate) use comment_notes::comment_marks;
mod symbol_table;
mod output_dialogs;
mod output_progress;
mod recovery_dialog;
mod note_dialog;
mod column_art;
mod columns_menu;
mod section_break_menu;
mod contents_menu;
mod recent_files_menu;
mod context_menu;
pub(crate) mod content_controls;
mod columns_dialog;
pub use recovery_dialog::recovery_dialog;
mod table_position;
mod table_layout;
pub use output_dialogs::import_pdf;
mod bitmap_button;
mod face;
mod skin;
mod generated;
mod group_relief;
mod navigation;
mod word_count;
mod colour_menu;
mod custom_colours;
mod review_ui;
mod original_review;
pub(crate) mod revision_tools;
mod revision_gutter;
mod spelling_menu;
mod spelling_dialog;
mod dictionary_install_ui;
mod dictionary_manager;
mod style_manager;
mod style_tools;
mod picture_layout;
mod layout_dialog;
mod doc_switcher;
mod hyphenation;
mod typst_highlight;
mod search_ui;
mod styles;
mod format_dialogs;
pub(crate) use format_dialogs::shortcut_text;
thread_local! {
    /// The desktop's accent colour, which marks the active Ribbon tab.
    static ACCENT: Cell<[u8; 3]> = const { Cell::new([53, 132, 228]) };
    /// The title bar's background and text, matching the desktop's title bars.
    static TITLE_COLOURS: Cell<([u8; 3], [u8; 3])> = const { Cell::new(([214, 222, 235], [36, 48, 66])) };
}
/// The desktop's accent colour.
/// The red cross that closes a pane, the same in every pane (the Styles
/// pane's button gets its size from tools/build_ribbon_ui.py, PANE_CLOSE).
pub(crate) const CLOSE_MARK_DIP: f32 = 20.0;
/// That cross, `size` pixels square, as straight-alpha RGBA.
pub(crate) fn close_mark(size: u32) -> Option<std::rc::Rc<[u8]>> { art::raster("x", size, size, false) }
pub(crate) fn accent() -> [u8; 3] { ACCENT.with(Cell::get) }
/// The title bar's background colour.
pub(crate) fn title_bar() -> [u8; 3] { TITLE_COLOURS.with(Cell::get).0 }
/// The title bar's text colour.
pub(crate) fn title_ink() -> [u8; 3] { TITLE_COLOURS.with(Cell::get).1 }
/// Whether the title bar is dark enough that its icons are drawn in its
/// (light) text colour.
pub(crate) fn title_is_dark() -> bool { let ink = title_ink(); ink[0] as u32 + ink[1] as u32 + ink[2] as u32 > 3 * 160 }
/// `a` moved towards `b` by `amount` (0 to 1), for hover shades that suit
/// light and dark title bars alike.
pub(crate) fn mix(a: [u8; 3], b: [u8; 3], amount: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * amount).round() as u8)
}

mod find_dialog;
mod citation_ui;

use crate::{commands::AppCommand, *};
use model::{KerningMode, LigatureMode, CharacterProperties, ListFormat, ParagraphAlignment, ParagraphDirection, ParagraphProperties, SectionProperties, SectionStart};
use editor::FormattingSummary;
use std::{cell::Cell, collections::HashMap, sync::{OnceLock,mpsc::{self, Receiver, TryRecvError}}};
use wxdragon::{
    color::Colour,
    dialogs::message_dialog::MessageDialog,
    event::{menu_events::MenuEvents, scroll_events::ScrollEvents, text_events::TextEvents},
    id::ID_OK,
    menus::Menu,
    widgets::{button::{Button, ButtonBitmapPosition}, combobox::ComboBox, scrollbar::ScrollBar},
};

/// Index of the Insert tab in the tab strip.
const INSERT_TAB: usize = 2;
const RIBBON_PAGES: usize = 8;
/// Quiet time (no edits or caret moves) before a hidden page is prepared.
const BACKGROUND_PREPARE_IDLE: Duration = Duration::from_millis(1200);
/// Page key of the Table tools, which replace Insert while open.
const TABLE_TOOLS_PAGE: usize = 8;
/// Page key of the Header & Footer tools, which replace Insert while a
/// header or footer is edited.
const HEADER_TOOLS_PAGE: usize = 9;
/// Page key of the Picture tools, which replace Insert while a picture is
/// selected.
const PICTURE_TOOLS_PAGE: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode { Graphical, Typst, Split }

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    InterfaceLanguage,
    New, Open, RecentFiles, Save, SaveAs, Recovery, Editor(AppCommand),
    Pdf, ImportPdf, Print, CommentNew, CommentDelete, CommentResolve, CommentStep(bool), StyleManager, ManageStyles, StyleGalleryMore, StylesClose, StyleNew, StyleModify, StyleApply, StyleInspect, StyleDelete, ClearDirect, Highlight, Hyphenation, TextLanguage,
    SearchOptions, SearchStep(bool), ReplaceOne, ReplaceAll,
    Contents, ContentsUpdate, ContentsLevel, Footnotes, Bookmarks, Comments, TrackChanges, TextTracking, Spelling, Compatibility,
    ReviewDisplay, ReviewMarkup, ReviewOriginalClose, ReviewPane, RevisionResolve(bool), RevisionStep(bool),
    EquationClose,
    SelectCells(u8), Table(crate::table_ui::TableOp), TablePosition, CloseTableTools, PictureAlign(u8), PictureDelete, ClosePictureTools, HeaderVariant(u8), HeaderFooterGo(bool), InsertField(u8), RemoveHeaderFooter, InsertNote(bool), NextFootnote(bool), ShowNotes, ExitHeaderFooter, ToggleRibbon, Help, CitationManager, SelectRibbonTab(u8), Brand, Unavailable,
    GrowFont, ShrinkFont, FontColor, Typography, Align(ParagraphAlignment), Indent(f32),
    Direction(ParagraphDirection), Symbol, PageBreak, ColumnBreak, SectionBreak, Landscape(bool),
    Bullets, Numbering, InsertTable, InsertImage, PageNumber, HeaderFooter(bool), Columns,
    ShowNavigation, Find, WordCount, View(ViewMode), FitWidth, FitPage, Zoom100, ZoomDialog,
    ToggleNavigation, RefreshSource, CopySource, SearchNavigation, ZoomStep(f32),
}

struct ButtonSpec {
    button: Button,
    name: &'static str,
    label: &'static str,
    icon: &'static str,
    action: Action,
    kind: &'static str,
    segment: &'static str,
    tip: &'static str,
    disabled: bool,
    width_dip: i32,
    height_dip: i32,
    art_width_dip: i32,
    art_height_dip: i32,
}

#[derive(Clone, Copy)]
enum Field { Family, Size, LineSpacing, Before, After, Paper, Margins }

const LIGATURE_OFF_ID:i32=30001;
const LIGATURE_STANDARD_ID:i32=30002;
const LIGATURE_DISCRETIONARY_ID:i32=30003;
const KERNING_OPTICAL_ID:i32=30004;
const KERNING_METRICS_ID:i32=30005;
const KERNING_OFF_ID:i32=30006;
const KERNING_HEADING_ID:i32=30007;
const LIGATURE_HEADING_ID:i32=30008;
const HYPHENATION_SELECTION_HEADING_ID:i32=30010;
const HYPHENATION_SELECTION_ON_ID:i32=30011;
const HYPHENATION_SELECTION_OFF_ID:i32=30012;
const HYPHENATION_DOCUMENT_HEADING_ID:i32=30013;
const HYPHENATION_DOCUMENT_ON_ID:i32=30014;
const HYPHENATION_DOCUMENT_OFF_ID:i32=30015;

#[derive(Clone, Debug, PartialEq)]
struct EditKey {
    revision: u64,
    tracking: bool,
    selection: Option<Selection>,
    typing: CharacterProperties,
    cells: Option<(NodeId, Vec<NodeId>)>,
}

type ScrollMetrics = (i32, i32, i32);

fn installed_font_families() -> &'static [String] {
    static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
    FAMILIES.get_or_init(|| typography::FontCatalog::system().family_names())
}

fn populate_font_combo(combo: ComboBox, inherit: bool) {
    let value = combo.get_value();
    combo.freeze();
    combo.clear();
    if inherit { combo.append(&crate::i18n::text("Inherit from base")); }
    for family in installed_font_families() { combo.append(family); }
    combo.change_value(&value);
    combo.thaw();
}

pub struct Ribbon {
    owner: std::rc::Weak<Ribbon>,
    find_dialog: RefCell<Option<Rc<find_dialog::FindDialog>>>,
    pending_search: RefCell<Option<find_dialog::Request>>,
    styles_view: RefCell<styles::StylesView>,
    styles_revision: Cell<Option<u64>>,
    /// The document stack's epoch when last seen; see `documents::Stack`.
    document_epoch: Cell<u64>,
    last_theme_check: Cell<Instant>,
    /// The document's share of the width in Split view, set by the handle.
    split_ratio: Cell<f32>,
    /// Whether the main window is the active one (its title bar is then drawn active).
    window_active: Cell<bool>,
    doc_switcher: RefCell<Option<Rc<doc_switcher::DocSwitcher>>>,
    styles_snapshot: RefCell<Option<model::StyleSheet>>,
    styles_syncing: Cell<bool>,
    ui: Rc<MainUi>,
    state: Rc<RefCell<CanvasState>>,
    buttons: Vec<ButtonSpec>,
    button_states: RefCell<Vec<Option<(bool, bool)>>>,
    dpi: Cell<i32>,
    /// Ribbon pages whose buttons are fitted and painted at the current DPI.
    /// Revisiting one only shows it; fitting every button and group again
    /// made tab switches sluggish. Cleared when the DPI changes.
    prepared_pages: RefCell<std::collections::HashSet<usize>>,
    mode: Cell<ViewMode>,
    tab_index: Cell<usize>,
    /// Insert > Table was chosen: the Table tools replace the Insert page until closed.
    table_tools: Cell<bool>,
    table_insert_requested: Cell<bool>,
    /// A header or footer is being edited: its tools replace the Insert page.
    header_tools: Cell<bool>,
    /// A picture is selected: its tools replace the Insert page.
    picture_tools: Cell<bool>,
    /// The tab shown before contextual tools opened by themselves (Picture,
    /// Header & Footer), to return to when they close.
    tab_before_tools: Cell<Option<usize>>,
    syncing: Cell<bool>,
    last_key: RefCell<Option<EditKey>>,
    /// When the document, selection or typing format last changed.
    last_activity: Cell<Instant>,
    last_poll: Cell<Instant>,
    last_status: RefCell<String>,
    last_title: RefCell<String>,
    last_scroll: Cell<Option<(ScrollMetrics, ScrollMetrics)>>,
    navigation: RefCell<Vec<navigation::Entry>>,
    navigation_revision: Cell<Option<u64>>,
    word_count: RefCell<word_count::State>,
    search_options: Cell<editor::search::SearchOptions>,
    search_report: RefCell<Option<Arc<editor::search::SearchReport>>>,
    search_job: RefCell<Option<Receiver<search_ui::ResultSet>>>,
    search_due: Cell<Instant>,
    search_cursor: Cell<Option<usize>>,
    source_revision: Cell<Option<u64>>,
    source_job: RefCell<Option<Receiver<(u64, String)>>>,
    source_copy_pending: Cell<bool>,
    last_context: RefCell<Option<(bool, bool, bool, bool)>>,
    /// Editor state that the contextual tools were last derived from.
    context_key: RefCell<Option<(u64, Option<editor::Selection>, bool, bool)>>,
    /// Text last given to each status label. Setting a native label, even to
    /// the same text, invalidates its layout and repaints it.
    label_text: RefCell<HashMap<&'static str, String>>,
    equation: RefCell<Option<Rc<crate::equation_editor::EquationEditor>>>,
    comments: RefCell<Option<Rc<comment_notes::CommentNotes>>>,
    style_gallery: RefCell<Option<Rc<style_gallery::Gallery>>>,
    style_context: RefCell<Option<(model::StyleId,u64,Option<editor::Selection>)>>,
    tab_strip: RefCell<Option<Rc<tab_strip::TabStrip>>>,
    tab_ruler: RefCell<Option<Rc<tab_ruler::TabRuler>>>,
    recent_settings_open: Rc<Cell<bool>>,
    recent_paths: RefCell<Vec<std::path::PathBuf>>,
    table_bar: RefCell<Option<Rc<table_bar::TableBar>>>,
    picture_bar: RefCell<Option<Rc<picture_bar::PictureBar>>>,
    spell_job: RefCell<Option<review_ui::SpellJob>>,
    review_pane: RefCell<revision_tools::PaneView>,
    revision_gutter: revision_gutter::Gutter,
    original_preview: RefCell<Option<original_review::OriginalPreview>>,
    original_reader: Rc<output_dialogs::PagePreview>,
    spelling_languages: RefCell<Vec<String>>,
    output_progress: RefCell<Option<output_progress::Progress>>,
    dictionary_install: RefCell<Option<Rc<dictionary_install_ui::InstallDialog>>>,
    dictionary_manager: RefCell<Option<Rc<dictionary_manager::Manager>>>,
    spelling_context: RefCell<Option<(u64, u64, crate::spelling::Issue)>>,
    table_context_menu: RefCell<Option<context_menu::TableMenuTarget>>,
}

impl Ribbon {
    /// Keep the document panel stable when switching Graphical / Typst / Split views.
    pub fn prepare(ui: &MainUi) {
        generated::scale_controls(ui);
        ui.main_frame.set_min_size(ui.main_frame.from_dip(Size::new(960, 580)));
        ui.ribbon_notebook.set_min_size(ui.ribbon_notebook.from_dip(Size::new(1, generated::RIBBON_HEIGHT_DIP)));
        for (index,page) in [ui.ribbon_file,ui.ribbon_home,ui.ribbon_insert,ui.ribbon_layout,
            ui.ribbon_references,ui.ribbon_review,ui.ribbon_view,ui.ribbon_typst].iter().enumerate() {
            page.show(index==1);
        }
        ui.source_panel.hide();
        ui.canvas_context_bar.hide();
        ui.equation_panel.hide(); ui.table_context_tools.hide();
        ui.picture_context_tools.hide();
        ui.navigator_panel.hide();
        ui.styles_resize_handle.hide();
        ui.styles_panel.hide();
        ui.review_panel.hide();
        ui.revision_gutter_panel.hide();
        ui.original_panel.hide();
        ui.root_panel.layout();
        ui.main_frame.layout();
    }

    pub fn new(ui: Rc<MainUi>, state: Rc<RefCell<CanvasState>>) -> Rc<Self> {
        let buttons = generated::buttons(&ui);
        let count = buttons.len();
        let original_reader = output_dialogs::PagePreview::install(ui.original_surface, Arc::new(VectorDocument::default()));
        let this = Rc::new_cyclic(|owner| Self {
            owner: owner.clone(), find_dialog: RefCell::new(None), pending_search: RefCell::new(None),
            styles_view: RefCell::new(Default::default()), styles_revision: Cell::new(None), document_epoch: Cell::new(0), last_theme_check: Cell::new(Instant::now() - Duration::from_secs(10)), split_ratio: Cell::new(0.5), window_active: Cell::new(true), doc_switcher: RefCell::new(None), styles_snapshot: RefCell::new(None), styles_syncing: Cell::new(false),
            ui, state, buttons,
            button_states: RefCell::new(vec![None; count]),
            dpi: Cell::new(0), prepared_pages: RefCell::default(), mode: Cell::new(ViewMode::Graphical), tab_index: Cell::new(1), table_tools: Cell::new(false), table_insert_requested: Cell::new(false), header_tools: Cell::new(false), picture_tools: Cell::new(false), tab_before_tools: Cell::new(None), syncing: Cell::new(false),
            last_key: RefCell::new(None), last_activity: Cell::new(Instant::now()), last_poll: Cell::new(Instant::now() - Duration::from_secs(1)),
            last_status: RefCell::new(String::new()), last_title: RefCell::new(String::new()),
            word_count: RefCell::default(), last_scroll: Cell::new(None), navigation: RefCell::new(Vec::new()), navigation_revision: Cell::new(None),
            search_options: Cell::new(Default::default()), search_report: RefCell::new(None), search_job: RefCell::new(None), search_due: Cell::new(Instant::now()), search_cursor: Cell::new(None),
            last_context: RefCell::new(None), context_key: RefCell::new(None), label_text: RefCell::new(HashMap::new()), equation: RefCell::new(None), comments: RefCell::new(None), style_gallery: RefCell::new(None), style_context: RefCell::new(None), tab_strip: RefCell::new(None), tab_ruler: RefCell::new(None), recent_paths: RefCell::new(Vec::new()), recent_settings_open: Rc::new(Cell::new(false)), table_bar: RefCell::new(None), picture_bar: RefCell::new(None), spell_job: RefCell::new(None), review_pane: RefCell::default(), revision_gutter: revision_gutter::Gutter::default(), original_preview: RefCell::default(), original_reader, spelling_languages: RefCell::new(Vec::new()), output_progress: RefCell::new(None), dictionary_install: RefCell::new(None), dictionary_manager: RefCell::new(None), spelling_context: RefCell::new(None), table_context_menu: RefCell::new(None),
            source_revision: Cell::new(None), source_job: RefCell::new(None), source_copy_pending: Cell::new(false),
        });
        generated::install_group_relief(&this.ui);
        let close = Rc::downgrade(&this);
        this.equation.replace(Some(crate::equation_editor::EquationEditor::install(
            this.ui.equation_surface, this.ui.equation_panel, this.ui.equation_input, Rc::clone(&this.state),
            Box::new(move || { if let Some(r) = close.upgrade() { r.run(Action::EquationClose); } }),
        )));
        this.comments.replace(Some(comment_notes::CommentNotes::install(&this)));
        this.bind();
        content_controls::bind(&this);
        revision_tools::bind(&this);
        revision_gutter::bind(&this);
        this.style_gallery.replace(Some(style_gallery::install(&this)));
        this.tab_strip.replace(Some(tab_strip::install(&this)));
        interface_language::install(&this);
        this.table_bar.replace(Some(table_bar::install(&this)));
        this.picture_bar.replace(Some(picture_bar::install(&this)));
        font_preview::install(&this);
        this.tab_ruler.replace(Some(tab_ruler::install(&this)));
        this.bind_styles_pane();
        this.bind_split_handle();
        this.bind_context_menu();
        typst_highlight::use_code_font(this.ui.typst_editor);
        this.doc_switcher.replace(Some(doc_switcher::install(&this)));
        dropdown::set_host(this.ui.root_panel, &this.ui.main_frame);
        {
            // The title bar fades while another window is active, as GNOME's do.
            let weak = Rc::downgrade(&this);
            this.ui.main_frame.on_activate(move |event| {
                if let (WindowEventData::Activate(activation), Some(ribbon)) = (&event, weak.upgrade()) {
                    ribbon.window_active.set(activation.is_active());
                    ribbon.follow_theme();
                }
                event.skip(true);
            });
        }
        this.follow_theme();
        this.refresh_art();
        this.poll();
        this
    }

    fn bind(self: &Rc<Self>) {
        // A character hook reaches the frame before a focused GTK control can
        // consume function keys. Queue the same command used by the canvas.
        {
            use wxdragon::event::{EventType, WxEvtHandler};
            let weak = Rc::downgrade(self);
            self.ui.main_frame.bind_internal(EventType::CHAR_HOOK, move |event| {
                if event.get_key_code() == Some(wxdragon::keycode::WXK_F7)
                    && !event.cmd_down() && !event.alt_down() && !event.shift_down() {
                    if let Some(ribbon) = weak.upgrade() { ribbon.state.borrow_mut().pending_spelling = true; }
                    event.skip(false);
                } else { event.skip(true); }
            });
        }
        let weak=Rc::downgrade(self);
        self.ui.navigation_search.on_text_updated(move |_| {if let Some(r)=weak.upgrade(){
            if !r.syncing.get(){r.navigation_revision.set(None);r.search_due.set(Instant::now()+Duration::from_millis(180));r.search_cursor.set(None);r.state.borrow_mut().search_marks=None;r.sync_find_from_navigation();}
        }});
        for spec in &self.buttons {
            spec.button.set_accessibility_label(if spec.label.is_empty() { spec.tip } else { spec.label });
            spec.button.set_accessibility_description(&crate::i18n::text(&(spec.tip)));
            spec.button.set_tooltip(&crate::i18n::text(&(spec.tip)));
            if spec.disabled { spec.button.enable(false); continue; }
            let weak = Rc::downgrade(self);
            let action = spec.action;
            spec.button.on_click(move |_| { if let Some(ribbon) = weak.upgrade() { ribbon.run(action); } });
        }
        let weak = Rc::downgrade(self);
        self.ui.status_word_count.on_mouse_left_up(move |_| {
            if let Some(ribbon) = weak.upgrade() { ribbon.run(Action::WordCount); }
        });
        let weak = Rc::downgrade(self);
        self.ui.track_changes_button.on_mouse_right_up(move |_| {
            if let Some(ribbon) = weak.upgrade() { ribbon.show_review_menu(); }
        });
        for (combo, field) in [
            (self.ui.font_size_combo, Field::Size),
            (self.ui.line_spacing_combo, Field::LineSpacing), (self.ui.space_before_combo, Field::Before),
            (self.ui.space_after_combo, Field::After), (self.ui.paper_size_combo, Field::Paper),
            (self.ui.margins_combo, Field::Margins),
        ] {
            let weak = Rc::downgrade(self);
            combo.on_selection_changed(move |_| {
                if let Some(ribbon) = weak.upgrade() { ribbon.commit_field(combo, field); }
            });
            let weak = Rc::downgrade(self);
            combo.on_enter_pressed(move |_| {
                if let Some(ribbon) = weak.upgrade() { ribbon.commit_field(combo, field); }
            });
        }
        let weak = Rc::downgrade(self);
        self.ui.main_frame.on_menu_selected(move |event| {
            if let Some(ribbon) = weak.upgrade() {
                let ligature_mode = match event.get_id() {
                    LIGATURE_OFF_ID => Some(LigatureMode::None),
                    LIGATURE_STANDARD_ID => Some(LigatureMode::Standard),
                    LIGATURE_DISCRETIONARY_ID => Some(LigatureMode::Discretionary),
                    _ => None,
                };
                let kerning_mode = match event.get_id() {
                    KERNING_OPTICAL_ID => Some(KerningMode::Optical),
                    KERNING_METRICS_ID => Some(KerningMode::Metrics),
                    KERNING_OFF_ID => Some(KerningMode::Off),
                    _ => None,
                };
                // (on, whole document)
                let hyphenation = match event.get_id() {
                    HYPHENATION_SELECTION_ON_ID => Some((true, false)),
                    HYPHENATION_SELECTION_OFF_ID => Some((false, false)),
                    HYPHENATION_DOCUMENT_ON_ID => Some((true, true)),
                    HYPHENATION_DOCUMENT_OFF_ID => Some((false, true)),
                    _ => None,
                };
                if table_layout::handle(&ribbon, event.get_id()) { return; }
                if content_controls::handle(&ribbon, event.get_id()) { return; }
                if ribbon.handle_style_context(event.get_id()) { return; }
                if contents_menu::handle(&ribbon, event.get_id()) { return; }
                if section_break_menu::handle(&ribbon, event.get_id()) { return; }
                if ribbon.handle_recent_files(event.get_id()) { return; }
                if ribbon.handle_review_menu(event.get_id()) { return; }
                if ribbon.handle_markup_menu(event.get_id()) { return; }
                if ribbon.handle_table_context_menu(event.get_id()) { return; }
                if ribbon.handle_spelling_menu(event.get_id()) { return; }
                if let Some(mode) = ligature_mode { ribbon.commit_ligature_mode(mode); }
                else if let Some(mode) = kerning_mode { ribbon.commit_kerning_mode(mode); }
                else if let Some((on, everywhere)) = hyphenation {
                    if let Err(error) = hyphenation::apply(&ribbon, on, everywhere) { ribbon.state.borrow_mut().action_status = Some(error); }
                }
                else { event.skip(true); }
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.zoom_slider.on_slider(move |_| {
            if let Some(ribbon) = weak.upgrade() {
                if !ribbon.syncing.get() { ribbon.zoom_to(ribbon.ui.zoom_slider.value() as f32 / 100.0); }
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.navigation_search.on_enter_pressed(move |_| {
            if let Some(ribbon) = weak.upgrade() { ribbon.run(Action::SearchNavigation); }
        });
        let weak = Rc::downgrade(self);
        self.ui.navigation_list.on_selection_changed(move |_| {
            if let Some(ribbon) = weak.upgrade() { if !ribbon.syncing.get() { ribbon.navigate_selected(); } }
        });
        // Word-style dialog launcher in the Styles group caption: the
        // Styles pane, as in Word.
        // The Footnotes group's launcher opens Footnotes and Endnotes.
        for (caption, tip, action) in [
            (self.ui.home_styles_3_group_caption, "Styles pane (F11)", Action::StyleManager),
            (self.ui.references_footnotes_1_group_caption, "Footnotes and endnotes", Action::Footnotes),
        ] {
            caption.set_tooltip(&crate::i18n::text(&(tip)));
            let weak = Rc::downgrade(self);
            caption.on_mouse_left_up(move |event| {
                let x = match &event { WindowEventData::MouseButton(m) => m.event.get_position().map(|p| p.x), _ => None };
                if x.is_some_and(|x| group_relief::launcher_hit(caption, x)) {
                    if let Some(ribbon) = weak.upgrade() { ribbon.run(action); }
                }
            });
        }
        let weak=Rc::downgrade(self);
        self.ui.navigation_replace.on_text_updated(move |_| { if let Some(r)=weak.upgrade(){
            if !r.syncing.get(){r.sync_find_from_navigation();}
        }});
        // Text controls retain Ctrl+A/C/V/etc. Only Find/Replace is intercepted.
        for text in [self.ui.navigation_search,self.ui.navigation_replace,self.ui.typst_editor] {
            let weak=Rc::downgrade(self);
            text.on_key_down(move |event| { if let Some(r)=weak.upgrade(){
                if r.handle_find_shortcut(&event){return;}
            } event.skip(true); });
        }
        for button in self.buttons.iter().map(|b|b.button) {
            let weak=Rc::downgrade(self);
            button.on_key_down(move |event| { if let Some(r)=weak.upgrade(){
                if r.handle_find_shortcut(&event){return;}
            } event.skip(true); });
        }
        self.bind_scrollbar(self.ui.vertical_scroll, true);
        self.bind_scrollbar(self.ui.horizontal_scroll, false);
    }

    fn bind_scrollbar(self: &Rc<Self>, scrollbar: ScrollBar, vertical: bool) {
        let weak = Rc::downgrade(self);
        scrollbar.on_thumb_track(move |event| {
            if let Some(ribbon) = weak.upgrade() { if let Some(position) = event.get_position() { ribbon.scroll_to(vertical, position as f32); } }
        });
        let weak = Rc::downgrade(self);
        scrollbar.on_thumb_release(move |event| {
            if let Some(ribbon) = weak.upgrade() { if let Some(position) = event.get_position() { ribbon.scroll_to(vertical, position as f32); } }
        });
        // Native line/page events are deltas, not thumb positions.
        for (kind, delta) in [(0, -1.0_f32), (1, 1.0), (2, -1.0), (3, 1.0)] {
            let weak = Rc::downgrade(self);
            let callback = move |_| {
                if let Some(ribbon) = weak.upgrade() {
                    let (position, page, pixel_scale) = {
                        let state = ribbon.state.borrow();
                        let (position, page) = if vertical { (state.viewport.scroll_px.y, state.viewport.viewport_px.height) }
                            else { (state.viewport.scroll_px.x, state.viewport.viewport_px.width) };
                        (position, page, state.pixel_scale)
                    };
                    // View pixels are physical, so the 32-DIP line step is too.
                    let amount = if kind < 2 { ribbon.ui.paper_canvas.from_dip_int(32) as f32 * pixel_scale } else { page * 0.9 };
                    ribbon.scroll_to(vertical, position + delta * amount);
                }
            };
            match kind {
                0 => { scrollbar.on_scroll_lineup(callback); }
                1 => { scrollbar.on_scroll_linedown(callback); }
                2 => { scrollbar.on_scroll_pageup(callback); }
                _ => { scrollbar.on_scroll_pagedown(callback); }
            }
        }
        let weak = Rc::downgrade(self);
        scrollbar.on_scroll_top(move |_| { if let Some(ribbon) = weak.upgrade() { ribbon.scroll_to(vertical, 0.0); } });
        let weak = Rc::downgrade(self);
        scrollbar.on_scroll_bottom(move |_| { if let Some(ribbon) = weak.upgrade() { ribbon.scroll_to(vertical, f32::MAX); } });
    }

    fn run(&self, action: Action) {
        if self.syncing.get() { return; }
        self.state.borrow_mut().action_status = None;
        if let Err(error) = self.execute(action) { self.state.borrow_mut().action_status = Some(error); }
        self.last_key.borrow_mut().take();
        self.last_poll.set(Instant::now() - Duration::from_secs(1));
        self.poll();
    }

    fn execute(&self, action: Action) -> Result<(), String> {
        if self.state.borrow().revisions.is_original() && !matches!(action,
            Action::ReviewDisplay | Action::ReviewMarkup | Action::ReviewOriginalClose | Action::ReviewPane
            | Action::InterfaceLanguage | Action::SelectRibbonTab(_) | Action::ToggleRibbon | Action::Help
            | Action::New | Action::Open | Action::RecentFiles | Action::Recovery
            | Action::Save | Action::SaveAs | Action::View(_)
        ) {
            return Err("Close Original to edit the document.".into());
        }
        match action {
            Action::InterfaceLanguage => self.show_interface_languages(),
            Action::Brand => {},
            Action::SelectRibbonTab(index) => self.select_tab(index as usize),
            Action::CitationManager => citation_ui::open(self)?,
            Action::Editor(AppCommand::InsertInlineEquation) => inline_equation::show(self, None),
            Action::Editor(AppCommand::InsertEquation) => { self.open_equation(None, model::EquationDisplay::Inline)?; }
            Action::EquationClose => { equation_ui::close(&mut self.state.borrow_mut()); self.focus_document(); }
            Action::Recovery => { recover_from_ui(&self.ui.main_frame, &self.state)?; }
            Action::SelectCells(mode) => { objects_ui::select_cells(&mut self.state.borrow_mut(), mode)?; self.focus_document(); }
            Action::Table(op) => {
                let edit = crate::table_ui::operation(&self.state.borrow(), op)?;
                if matches!(op, crate::table_ui::TableOp::Float(_) | crate::table_ui::TableOp::WrapAround(_)) {
                    let id = crate::table_ui::context(&self.state.borrow()).ok_or("Place the caret in a table first")?.table_id;
                    self.edit_table(&id, edit)?;
                } else {
                    { let mut state = self.state.borrow_mut(); state.table_band = None; state.selected_object = None; }
                    self.edit(edit)?;
                }
            }
            Action::PictureAlign(value) => {
                let id = match self.state.borrow().selected_object.as_ref() {
                    Some(objects_ui::SelectedObject::Picture(id)) => id.clone(), _ => return Err("Select a picture first".into()),
                };
                let align = match value { 0 => model::ImageAlignment::Left, 1 => model::ImageAlignment::Center, _ => model::ImageAlignment::Right };
                self.edit(|e| e.align_picture(&id, align))?;
            }
            Action::PictureDelete => { objects_ui::delete_selection(&mut self.state.borrow_mut())?; self.sync_context(); self.focus_document(); }
            Action::ClosePictureTools => { objects_ui::deselect(&mut self.state.borrow_mut()); self.sync_context(); self.focus_document(); }
            Action::HeaderVariant(value) => {
                let context = self.state.borrow().editor.header_footer_context().ok_or("Enter a header or footer first")?;
                let variant = editor::HeaderFooterVariant::from_index(value);
                self.edit(|e| e.enter_header_footer_at(&context.section_id, context.header, variant))?;
            }
            Action::HeaderFooterGo(header) => {
                let context = self.state.borrow().editor.header_footer_context().ok_or("Enter a header or footer first")?;
                if context.header != header { self.edit(EditorState::switch_header_footer)?; }
            }
            Action::InsertField(index) => {
                let (kind, display_text) = crate::header_footer_ui::field(index);
                self.edit(move |e| e.insert_field(kind, display_text))?;
            }
            Action::InsertNote(endnote) => {
                let kind = if endnote { model::NoteKind::Endnote } else { model::NoteKind::Footnote };
                self.edit(|e| e.insert_note_here(kind))?;
            }
            Action::NextFootnote(forward) => self.edit(|e| e.go_to_adjacent_note(forward))?,
            Action::ShowNotes => self.edit(EditorState::toggle_note_view)?,
            Action::RemoveHeaderFooter => { self.edit(EditorState::remove_header_footer)?; self.state.borrow_mut().caret_page_hint = None; }
            Action::ExitHeaderFooter => { self.edit(EditorState::exit_header_footer)?; self.state.borrow_mut().caret_page_hint = None; }
            Action::Editor(command) => {
                let result = execute_editor_app_command(&mut self.state.borrow_mut(), command);
                self.focus_document(); result?;
            }
            Action::New => {
                let mut state = self.state.borrow_mut();
                documents::add(&mut state, documents::blank(), None)?;
                state.action_status = Some("New document ready".into());
            }
            Action::Open => {
                request_open(&self.ui.main_frame,&self.state)?;
            }
            Action::RecentFiles => self.show_recent_files()?,
            Action::Save | Action::SaveAs => background_io::queue_save(&self.ui.main_frame,&self.state,action==Action::SaveAs)?,
            Action::GrowFont | Action::ShrinkFont => {
                let current = self.state.borrow().formatting_summary().character.font_size_pt.unwrap_or(11.0);
                let next = (current + if action == Action::GrowFont { 1.0 } else { -1.0 }).clamp(1.0, 400.0);
                self.edit_formatting(|editor| editor.apply_character_properties(&CharacterProperties { font_size_pt: Some(next), ..Default::default() }))?;
            }
            Action::Typography => self.show_typography_menu(),
            Action::FontColor => {
                let button = self.ui.font_color_button;
                colour_menu::show(self, colour_menu::Kind::Text, crate::gtk_native::to_screen(&button,
                    wxdragon::geometry::Point::new(0, button.get_size().height)));
            }
            Action::Align(alignment) => self.edit_formatting(|e| e.apply_paragraph_properties(&ParagraphProperties { alignment: Some(alignment), ..Default::default() }))?,
            Action::Direction(direction) => self.edit_formatting(|e| e.apply_paragraph_properties(&ParagraphProperties { direction: Some(direction), ..Default::default() }))?,
            Action::Indent(delta) => self.edit_formatting(|e| {
                if e.current_list_level().is_some() {
                    e.adjust_list_level(if delta > 0.0 { 1 } else { -1 })
                } else { e.adjust_left_indent(delta) }
            })?,
            Action::PageBreak => self.edit(EditorState::insert_page_break)?,
            Action::ColumnBreak => self.edit(EditorState::insert_column_break)?,
            Action::SectionBreak => section_break_menu::show(self),
            Action::Bullets => self.edit_formatting(|editor| editor.toggle_list(ListFormat::Bullet))?,
            Action::Numbering => self.edit_formatting(|editor| editor.toggle_list(ListFormat::Decimal))?,
            Action::Columns => columns_menu::show(self, &self.ui.columns_button),
            Action::InsertTable => table_layout::show(self),
            Action::CloseTableTools => self.close_table_tools(),
            Action::InsertImage => {
                let existing = self.state.borrow().editor.inline_image_size_at_caret();
                let mut insert_new = existing.is_none();
                if let Some((width, height)) = existing {
                    let choices = ["Insert another picture…", "Scale to 50%", "Scale to 75%", "Scale to 125%", "Delete picture"];
                    let choices = choices.map(crate::i18n::text);
                    if let Some(index) = input_dialog::choice(&self.ui.main_frame, "Picture", "The caret is next to an inline picture.", &choices) {
                        match index {
                            0 => insert_new = true,
                            1 => self.edit(|e| e.resize_inline_image_at_caret(width * 0.5, height * 0.5))?,
                            2 => self.edit(|e| e.resize_inline_image_at_caret(width * 0.75, height * 0.75))?,
                            3 => self.edit(|e| e.resize_inline_image_at_caret(width * 1.25, height * 1.25))?,
                            4 => self.edit(EditorState::delete_inline_image_at_caret)?,
                            _ => {}
                        }
                    }
                }
                if insert_new {
                    if let Some(path) = choose_image_path(&self.ui.main_frame) {
                        let (media_type, data, width_mm, height_mm) = image_insert_payload(&path)?;
                        let name = path.file_name().and_then(|n| n.to_str()).map(|s| s.to_owned());
                        let alt = name.clone();
                        self.edit(move |editor| editor.insert_inline_image(media_type, name, data, width_mm, height_mm, alt))?;
                    }
                }
            }
            Action::PageNumber => self.edit(EditorState::insert_page_number_footer)?,
            Action::HeaderFooter(header) => header_footer_gallery::show(self, header)?,
            Action::Landscape(landscape) => self.edit(|editor| {
                let mut props = editor.section_properties_at_caret().ok_or("No active section")?;
                let short = props.page_width_mm.min(props.page_height_mm);
                let long = props.page_width_mm.max(props.page_height_mm);
                props.page_width_mm = if landscape { long } else { short };
                props.page_height_mm = if landscape { short } else { long };
                editor.set_section_properties(props)
            })?,
            Action::Symbol => symbol_picker::show(self),
            // Split toggles: pressed again, it returns to the document alone.
            Action::View(ViewMode::Split) if self.mode.get() == ViewMode::Split => self.set_view(ViewMode::Graphical),
            Action::View(mode) => self.set_view(mode),
            Action::FitWidth => self.fit(false), Action::FitPage => self.fit(true),
            Action::Zoom100 => self.zoom_to(1.0),
            Action::ZoomDialog => zoom_dialog::show(self),
            Action::ZoomStep(delta) => { let zoom = self.state.borrow().viewport.zoom; self.zoom_to(zoom + delta); }
            Action::ToggleRibbon => {
                let show = !self.ui.ribbon_notebook.is_shown();
                self.ui.ribbon_notebook.show(show); self.ui.root_panel.layout();
                self.ui.collapse_ribbon_button.set_tooltip(&crate::i18n::text(&(if show { "Hide Ribbon" } else { "Show Ribbon" })));
                self.focus_document();
            }
            Action::ToggleNavigation => self.show_navigation(!self.ui.navigator_panel.is_shown()),
            Action::ShowNavigation => { self.ui.navigation_search.change_value(""); self.navigation_revision.set(None); self.search_cursor.set(None); self.search_report.borrow_mut().take(); self.sync_find_from_navigation(); self.show_navigation(true); }
            Action::Find => self.open_find_dialog(false),
            Action::SearchNavigation => {self.search_due.set(Instant::now());self.navigation_revision.set(None);self.populate_navigation();},
            Action::SearchOptions => self.search_options_dialog(),
            Action::SearchStep(forward) => self.search_step(forward),
            Action::ReplaceOne => self.replace_found(false)?,
            Action::ReplaceAll => self.replace_found(true)?,
            Action::StyleManager => self.show_styles_pane(true),
            Action::ManageStyles => style_manager::show(self),
            Action::StyleGalleryMore => self.show_more_styles_gallery(),
            Action::StylesClose => self.show_styles_pane(false),
            Action::StyleNew => self.create_pane_style()?,
            Action::StyleModify => self.modify_pane_style()?,
            Action::StyleApply => self.apply_pane_style()?,
            Action::StyleInspect => self.inspect_pane_style(),
            Action::StyleDelete => self.delete_pane_style()?,
            Action::ClearDirect => self.edit_formatting(EditorState::clear_direct_formatting)?,
            Action::Highlight => self.highlight_dialog()?,
            Action::Hyphenation => self.show_hyphenation_menu(),
            Action::TextLanguage => self.language_dialog()?,
            Action::TablePosition => table_position::table_position(self)?,
            Action::Pdf => output_dialogs::export_pdf(self)?,
            Action::ImportPdf => {
                import_pdf(&self.ui.main_frame, &self.state, None)?;
            }
            Action::Print => output_dialogs::print(self)?,
            Action::WordCount => {
                let stats = navigation::count_body(&self.state.borrow().editor.document);
                document_info::word_count(&self.ui.main_frame, stats);
            }
            Action::RefreshSource => { self.source_revision.set(None); self.request_source(); }
            Action::CopySource => { self.source_copy_pending.set(true); self.request_source(); }
            Action::Help => document_info::about(&self.ui.main_frame),
            Action::Contents => contents_menu::show(self),
            Action::ContentsUpdate => { contents_menu::update(self)?; self.focus_document(); }
            Action::ContentsLevel => contents_menu::show_levels(self),
            Action::Footnotes => self.footnotes()?, Action::Bookmarks => self.bookmarks()?,
            Action::Comments => { let pane = self.comments.borrow().clone(); if let Some(p) = pane { p.show(!p.shown()); } }
            Action::CommentNew => { let pane = self.comments.borrow().clone(); if let Some(p) = pane { p.new_comment(); } }
            Action::CommentDelete | Action::CommentResolve => {
                let id = comment_notes::current(&self.state.borrow()).ok_or("Select a comment first")?;
                let pane = self.comments.borrow().clone();
                if let Some(p) = pane { if matches!(action, Action::CommentDelete) { p.delete(&id); } else { p.toggle_resolved(&id); } }
            }
            Action::CommentStep(forward) => { let pane = self.comments.borrow().clone(); if let Some(p) = pane { p.step(forward); } }
            Action::TrackChanges => self.toggle_document_tracking()?,
            Action::TextTracking => self.show_review_menu(),
            Action::ReviewDisplay => self.show_display_menu(),
            Action::ReviewOriginalClose => self.close_original(),
            Action::ReviewMarkup => self.show_markup_menu(),
            Action::ReviewPane => self.toggle_review_pane(),
            Action::RevisionResolve(accept) => self.resolve_current_revision(accept)?,
            Action::RevisionStep(forward) => self.step_revision(forward)?,
            Action::Spelling => self.start_spelling()?, Action::Compatibility => self.compatibility(),
            Action::Unavailable => return Err("This command is not available in the current alpha.".into()),
        }
        Ok(())
    }

    fn select_tab(&self, index: usize) {
        let pages=[self.ui.ribbon_file,self.ui.ribbon_home,self.ui.ribbon_insert,self.ui.ribbon_layout,
            self.ui.ribbon_references,self.ui.ribbon_review,self.ui.ribbon_view,self.ui.ribbon_typst];
        if index>=pages.len() { return; }
        self.ui.ribbon_panel_content.freeze();
        let context = if index == INSERT_TAB { self.insert_context_page() } else { None };
        for (i,page) in pages.iter().enumerate() { page.show(i==index && context.is_none()); }
        self.ui.table_context_tools.show(context == Some(TABLE_TOOLS_PAGE));
        self.ui.header_context_tools.show(context == Some(HEADER_TOOLS_PAGE));
        self.ui.picture_context_tools.show(context == Some(PICTURE_TOOLS_PAGE));
        self.tab_index.set(index);
        if context == Some(TABLE_TOOLS_PAGE) { if let Some(bar) = self.table_bar.borrow().as_ref() { bar.refresh(); } }
        if context == Some(PICTURE_TOOLS_PAGE) { if let Some(bar) = self.picture_bar.borrow().as_ref() { bar.refresh(); } }
        if let Some(strip) = self.tab_strip.borrow().as_ref() { strip.refresh(); }
        let page = context.unwrap_or(index);
        // DPI 0 means the Ribbon art has not been measured yet: always prepare.
        let first_visit = self.dpi.get() == 0 || !self.prepared_pages.borrow().contains(&page);
        if first_visit {
            self.fit_visible_buttons();
            for group in generated::groups(&self.ui) {
                if shown_in_hierarchy(&group) { group_relief::layout(group); }
            }
        }
        let layout_page = || {
            match context {
                Some(HEADER_TOOLS_PAGE) => self.ui.header_context_tools.layout(),
                Some(TABLE_TOOLS_PAGE) => self.ui.table_context_tools.layout(),
                Some(PICTURE_TOOLS_PAGE) => self.ui.picture_context_tools.layout(),
                _ => pages[index].layout(),
            }
            self.ui.ribbon_panel_content.layout();
        };
        layout_page();
        if first_visit {
            // Faces are drawn at the size layout settled on. Drawn before it,
            // most were drawn twice, as layout trims small buttons slightly.
            if self.install_visible_faces() { layout_page(); }
            // The Ribbon keeps its height, so the window below needs laying
            // out only when a page's controls were fitted for the first time.
            self.ui.ribbon_notebook.layout();
            self.ui.root_panel.layout();
            self.prepared_pages.borrow_mut().insert(page);
        }
        self.ui.ribbon_panel_content.thaw();
    }

    /// Prepare one Ribbon page that has not been shown yet, while the user is
    /// idle, so its first visit is as quick as a revisit. The Ribbon is frozen
    /// while the page is shown, fitted and hidden again, so nothing flickers.
    fn prepare_hidden_page_when_idle(&self) {
        if self.dpi.get() == 0 || self.last_activity.get().elapsed() < BACKGROUND_PREPARE_IDLE { return; }
        if self.state.borrow().equation_draft.is_some() { return; }
        let current = self.tab_index.get();
        let insert_replaced = self.insert_context_page().is_some();
        let next = (0..RIBBON_PAGES).find(|&page| page != current
            && !(page == INSERT_TAB && insert_replaced)
            && !self.prepared_pages.borrow().contains(&page));
        let Some(next) = next else { return; };
        self.ui.ribbon_notebook.freeze();
        self.select_tab(next);
        self.select_tab(current);
        self.ui.ribbon_notebook.thaw();
        // One page per idle tick keeps each pause short.
        self.last_activity.set(Instant::now() - BACKGROUND_PREPARE_IDLE / 2);
    }

    fn edit(&self, operation: impl FnOnce(&mut EditorState) -> Result<(), String>) -> Result<(), String> {
        let result = self.edit_without_focus(operation);
        self.focus_document();
        result
    }

    fn edit_table(&self, id: &NodeId, operation: impl FnOnce(&mut EditorState) -> Result<(), String>) -> Result<(), String> {
        let result = self.edit_canvas(|state| {
            operation(&mut state.editor)?;
            crate::table_ui::select_after_position(state, id);
            Ok(())
        });
        self.focus_document();
        result
    }

    fn edit_formatting(&self, operation: impl FnOnce(&mut EditorState) -> Result<(), String>) -> Result<(), String> {
        let result = self.edit_canvas(|state| state.apply_formatting(operation));
        self.focus_document();
        result
    }

    /// Modal review windows retain keyboard focus while applying document edits.
    fn edit_without_focus(&self, operation: impl FnOnce(&mut EditorState) -> Result<(), String>) -> Result<(), String> {
        self.edit_canvas(|state| operation(&mut state.editor))
    }

    fn edit_canvas(&self, operation: impl FnOnce(&mut CanvasState) -> Result<(), String>) -> Result<(), String> {
        if self.state.borrow().revisions.is_original() {
            return Err("Close Original to edit the document.".into());
        }
        let result = {
            let mut state = self.state.borrow_mut(); let before = state.editor.revision;let selection=state.editor.selection.clone();
            let result = operation(&mut state);
            if state.editor.revision != before { submit_layout(&mut state); }
            state.preferred_caret_x_pt = None; state.caret_visual_line = None;
            state.action_status = None; refresh_overlay(&mut state); wake_caret(&mut state);
            if result.is_ok()&&state.editor.selection!=selection{
                // A changed document is revealed once its new layout arrives.
                if state.editor.revision==before{reveal_review_caret(&mut state);}else{state.reveal_caret_at=Some(state.layout_generation);}
            }
            result
        };
        result
    }

    /// Opens the equation pane on the equation `id` (or the selected one), or
    /// for a new equation of the kind `new` (inline or on its own line).
    fn open_equation(&self, id: Option<NodeId>, new: model::EquationDisplay) -> Result<(), String> {
        let id = id.or_else(|| match self.state.borrow().selected_object.as_ref() {
            Some(objects_ui::SelectedObject::Equation(id)) => Some(id.clone()), _ => None,
        });
        equation_ui::open(&mut self.state.borrow_mut(), id, new)?;
        self.sync_context();
        let root = equation_ui::root(&self.state.borrow());
        let editor = self.equation.borrow().clone();
        if let Some(editor) = editor {
            editor.load(root.as_ref());
            self.set_view(ViewMode::Graphical); self.sync_context();
            editor.focus();
        }
        Ok(())
    }
    /// True when `text` differs from what `key`'s label last showed.
    fn label_changed(&self, key: &'static str, text: &str) -> bool {
        let mut labels = self.label_text.borrow_mut();
        if labels.get(key).is_some_and(|old| old == text) { return false; }
        labels.insert(key, text.to_owned());
        true
    }

    fn sync_context(&self) {
        // Walking the document for table/header context on every timer tick
        // is wasted work while neither the caret nor the document changed.
        let key = {
            let state = self.state.borrow();
            (state.editor.revision, state.editor.selection.clone(),
             matches!(state.selected_object, Some(objects_ui::SelectedObject::Picture(_))), state.equation_draft.is_some())
        };
        if !key.3 && self.context_key.borrow().as_ref() == Some(&key) { return; }
        self.context_key.replace(Some(key));
        if let Some(bar) = self.table_bar.borrow().as_ref() { bar.refresh(); }
        if let Some(bar) = self.picture_bar.borrow().as_ref() { bar.refresh(); }
        let (visible, message, can_apply) = {
            let state = self.state.borrow();
            let equation = state.equation_draft.as_ref();
            ((state.editor.table_cell_context().is_some() && equation.is_none(),
              matches!(state.selected_object, Some(objects_ui::SelectedObject::Picture(_))) && equation.is_none(),
              state.editor.is_editing_header_footer() && equation.is_none(), equation.is_some()),
             equation.map(|d| d.status()).unwrap_or_default(), equation.is_some_and(|d| d.up_to_date()))
        };
        if self.last_context.borrow().as_ref() != Some(&visible) {
            self.ui.equation_panel.show(visible.3);
            // Selecting a picture or entering a header or footer opens its
            // tools, as Word's contextual tabs; closing them returns to the
            // page shown before.
            let had_picture = self.picture_tools.replace(visible.1);
            let had_header = self.header_tools.replace(visible.2);
            if (visible.1 && !had_picture) || (visible.2 && !had_header) { self.open_context_tools(); }
            else if (had_picture && !visible.1) || (had_header && !visible.2) { self.close_context_tools(); }
            self.ui.workspace_panel.layout(); self.ui.body_panel.layout();
            self.last_context.replace(Some(visible)); self.last_key.borrow_mut().take();
            self.state.borrow_mut().needs_present = true;
        }
        if visible.3 {
            let _ = (message, can_apply);
            let editor = self.equation.borrow().clone();
            if let Some(editor) = editor { editor.poll(); }
        }
    }

    /// Takes the title bar's colours from the desktop theme when it changes,
    /// so the title bar blends with other windows.
    fn follow_theme(&self) {
        // The title bar matches GNOME's apps: the colours their library
        // (libadwaita) gives title bars in the current style, active or in the
        // background. Without libadwaita, the GTK theme's header bar.
        let active = self.window_active.get();
        let colours = match crate::gtk_native::title_bar_colours() {
            Some(c) => (if active { c.active } else { c.backdrop }, c.text),
            None => match crate::gtk_native::theme_title_colours() { Some(c) => c, None => return },
        };
        let accent = crate::gtk_native::accent_colour().unwrap_or([53, 132, 228]);
        let accent_changed = ACCENT.with(|cell| cell.replace(accent)) != accent;
        if TITLE_COLOURS.with(Cell::get) == colours && !accent_changed { return; }
        TITLE_COLOURS.with(|cell| cell.set(colours));
        let (bg, ink) = colours;
        crate::gtk_native::set_title_bar_colour(bg);
        self.ui.title_strip.set_background_color(Colour::rgb(bg[0], bg[1], bg[2]));
        self.ui.app_brand.set_foreground_color(Colour::rgb(ink[0], ink[1], ink[2]));
        self.install_faces_where(|spec| matches!(spec.kind, "quick" | "logo"));
        if let Some(switcher) = self.doc_switcher.borrow().as_ref() { switcher.repaint(); }
        self.ui.title_strip.refresh(true, None);
    }

    /// Another document is shown: views that follow one document start again.
    fn document_shown(&self) {
        self.source_revision.set(None); self.navigation_revision.set(None); self.styles_revision.set(None);
        self.pending_search.borrow_mut().take(); self.search_cursor.set(None); self.search_report.borrow_mut().take();
        self.context_key.replace(None);
        self.word_count.replace(Default::default());
        self.focus_document();
    }

    /// Closes an open document, asking first when it has unsaved changes,
    /// as Word does. The document is shown while it is asked about.
    pub(crate) fn close_document(&self, id: u64) -> Result<(), String> {
        let dirty = documents::rows(&self.state.borrow()).iter().any(|row| row.id == id && row.dirty);
        if !dirty { return documents::close(&mut self.state.borrow_mut(), id); }
        documents::switch_to(&mut self.state.borrow_mut(), id)?;
        self.ui.main_frame.update();
        let name = documents::active_name(&self.state.borrow());
        let dialog = MessageDialog::builder(&self.ui.main_frame, &crate::i18n::text(&(&format!("Save the changes to “{name}” before closing it?"))), &crate::i18n::text(&("Close document")))
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::Cancel | MessageDialogStyle::IconWarning).build();
        dialog.set_yes_no_labels(&crate::i18n::text(&("Save")), &crate::i18n::text(&("Don't save")));
        dialog.set_ok_cancel_labels(&crate::i18n::text("Save"), &crate::i18n::text("Cancel"));
        match {crate::placement::centre(&dialog); dialog.show_modal()} {
            wxdragon::id::ID_YES => {
                background_io::queue_save(&self.ui.main_frame, &self.state, false)?;
                documents::close_when_saved(&mut self.state.borrow_mut());
                Ok(())
            }
            wxdragon::id::ID_NO => documents::close(&mut self.state.borrow_mut(), id),
            _ => Ok(()),
        }
    }

    fn commit_field(&self, combo: ComboBox, field: Field) {
        self.commit_field_value(crate::i18n::control_value(&combo.get_name(), &combo.get_value()), field);
    }

    fn commit_field_value(&self, text: String, field: Field) {
        if self.syncing.get() { return; }
        let value = text.trim().to_string();
        let apply = |editor: &mut EditorState| match field {
            Field::Family => editor.apply_character_properties(&CharacterProperties { font_family: Some(value), ..Default::default() }),
            Field::Size => editor.apply_character_properties(&CharacterProperties { font_size_pt: Some(number(&value)?), ..Default::default() }),
            Field::LineSpacing => editor.apply_paragraph_properties(&ParagraphProperties { line_spacing: Some(number(&value)?), ..Default::default() }),
            Field::Before | Field::After => {
                let amount = number(&value)?;
                if !(0.0..=1000.0).contains(&amount) { return Err("Paragraph spacing must be between 0 and 1000 points.".into()); }
                let mut patch = ParagraphProperties::default();
                if matches!(field, Field::Before) { patch.space_before_pt = Some(amount); } else { patch.space_after_pt = Some(amount); }
                editor.apply_paragraph_properties(&patch)
            }
            Field::Paper | Field::Margins => {
                let mut props = editor.section_properties_at_caret().ok_or("No active section")?;
                if matches!(field, Field::Paper) {
                    let (w, h) = match value.as_str() {
                        "A4" => (210.0, 297.0), "Letter" => (215.9, 279.4), "Legal" => (215.9, 355.6), "A5" => (148.0, 210.0),
                        _ => return Err("Choose a supported paper size.".into()),
                    };
                    let landscape = props.page_width_mm > props.page_height_mm;
                    props.page_width_mm = if landscape { h } else { w }; props.page_height_mm = if landscape { w } else { h };
                } else {
                    let margin = match value.as_str() {
                        "Normal (25.4 mm)" => 25.4, "Narrow (12.7 mm)" => 12.7, "Wide (38.1 mm)" => 38.1,
                        _ => return Err("Choose a margin preset.".into()),
                    };
                    props.margin_left_mm = margin; props.margin_right_mm = margin;
                    props.margin_top_mm = margin; props.margin_bottom_mm = margin;
                }
                editor.set_section_properties(props)
            }
        };
        let result = if matches!(field, Field::Paper | Field::Margins) {
            self.edit(apply)
        } else {
            self.edit_formatting(apply)
        };
        if let Err(error) = result { self.state.borrow_mut().action_status = Some(error); }
        self.last_key.borrow_mut().take();
    }

    fn commit_kerning_mode(&self, mode: KerningMode) {
        if let Err(error) = self.edit_formatting(|editor| editor.set_kerning_mode(mode)) {
            self.state.borrow_mut().action_status = Some(error);
        }
        self.last_key.borrow_mut().take();
    }

    fn commit_ligature_mode(&self, mode: LigatureMode) {
        if let Err(error) = self.edit_formatting(|editor| editor.set_ligature_mode(mode)) {
            self.state.borrow_mut().action_status = Some(error);
        }
        self.last_key.borrow_mut().take();
    }

    fn show_typography_menu(&self) {
        let mut menu=Menu::builder()
            .append_item(KERNING_HEADING_ID,&crate::i18n::text(&("Kerning")),&crate::i18n::text(&("")))
            .append_check_item(KERNING_OPTICAL_ID,&crate::i18n::text(&("Optical")),&crate::i18n::text(&("Adjust conspicuously uneven glyph outlines")))
            .append_check_item(KERNING_METRICS_ID,&crate::i18n::text(&("Metrics")),&crate::i18n::text(&("Use kerning supplied by the font")))
            .append_check_item(KERNING_OFF_ID,&crate::i18n::text(&("Off")),&crate::i18n::text(&("Disable kerning")))
            .append_separator()
            .append_item(LIGATURE_HEADING_ID,&crate::i18n::text(&("Ligatures")),&crate::i18n::text(&("")))
            .append_check_item(LIGATURE_OFF_ID,&crate::i18n::text(&("Off")),&crate::i18n::text(&("Do not form OpenType ligatures")))
            .append_check_item(LIGATURE_STANDARD_ID,&crate::i18n::text(&("Standard")),&crate::i18n::text(&("Use standard ligatures supplied by the font")))
            .append_check_item(LIGATURE_DISCRETIONARY_ID,&crate::i18n::text(&("Standard + discretionary")),&crate::i18n::text(&("Use optional ligatures supplied by the font")))
            .build();
        debug_assert_eq!(menu.get_item_count(),9);
        menu.enable_item(KERNING_HEADING_ID,false);
        menu.enable_item(LIGATURE_HEADING_ID,false);
        let format=self.state.borrow().formatting_summary();
        menu.check_item(match format.kerning_mode {
            Some(KerningMode::Optical)=>KERNING_OPTICAL_ID,
            Some(KerningMode::Off)=>KERNING_OFF_ID,
            _=>KERNING_METRICS_ID,
        },true);
        menu.check_item(match format.ligatures {
            Some(LigatureMode::None)=>LIGATURE_OFF_ID,
            Some(LigatureMode::Discretionary)=>LIGATURE_DISCRETIONARY_ID,
            _=>LIGATURE_STANDARD_ID,
        },true);
        let button=self.ui.typography_menu_button;
        let anchor=crate::gtk_native::to_screen(&button,wxdragon::geometry::Point::new(0,button.get_size().height));
        self.ui.main_frame.popup_menu(&mut menu,Some(anchor));
    }

    /// Hyphenation, as the Typography menu: for the selected paragraphs (the
    /// caret's checked) or the whole document, applied when chosen.
    fn show_hyphenation_menu(&self) {
        let mut menu=Menu::builder()
            .append_item(HYPHENATION_SELECTION_HEADING_ID,&crate::i18n::text(&("Selected paragraphs")),&crate::i18n::text(&("")))
            .append_check_item(HYPHENATION_SELECTION_ON_ID,&crate::i18n::text(&("Hyphenate automatically")),&crate::i18n::text(&("Break words at line ends where the language allows")))
            .append_check_item(HYPHENATION_SELECTION_OFF_ID,&crate::i18n::text(&("Don't hyphenate")),&crate::i18n::text(&("Keep words whole")))
            .append_separator()
            .append_item(HYPHENATION_DOCUMENT_HEADING_ID,&crate::i18n::text(&("Whole document")),&crate::i18n::text(&("")))
            .append_item(HYPHENATION_DOCUMENT_ON_ID,&crate::i18n::text(&("Hyphenate automatically")),&crate::i18n::text(&("Hyphenate every paragraph")))
            .append_item(HYPHENATION_DOCUMENT_OFF_ID,&crate::i18n::text(&("Don't hyphenate")),&crate::i18n::text(&("Hyphenate no paragraph")))
            .build();
        menu.enable_item(HYPHENATION_SELECTION_HEADING_ID,false);
        menu.enable_item(HYPHENATION_DOCUMENT_HEADING_ID,false);
        let on=hyphenation::hyphenates_at_caret(&self.state.borrow().editor);
        menu.check_item(if on {HYPHENATION_SELECTION_ON_ID} else {HYPHENATION_SELECTION_OFF_ID},true);
        let button=self.ui.hyphenation_button;
        let anchor=crate::gtk_native::to_screen(&button,wxdragon::geometry::Point::new(0,button.get_size().height));
        self.ui.main_frame.popup_menu(&mut menu,Some(anchor));
    }

    fn show_more_styles_gallery(&self) {
        gallery_popup::show(self);
    }


    /// Which contextual tools replace the Insert page, if any: the Picture
    /// tools for a selected picture, else the Header & Footer tools while one
    /// is edited, else the Table tools while open.
    fn insert_context_page(&self) -> Option<usize> {
        if self.picture_tools.get() { Some(PICTURE_TOOLS_PAGE) }
        else if self.header_tools.get() { Some(HEADER_TOOLS_PAGE) }
        else if self.table_tools.get() { Some(TABLE_TOOLS_PAGE) }
        else { None }
    }

    /// Shows contextual tools that opened by themselves, remembering the tab
    /// they replace.
    fn open_context_tools(&self) {
        if self.tab_before_tools.get().is_none() { self.tab_before_tools.set(Some(self.tab_index.get())); }
        self.select_tab(INSERT_TAB);
    }

    /// After contextual tools closed: shows the next tools still open, or
    /// returns to the tab shown before they opened. A tab the user chose
    /// meanwhile stays.
    fn close_context_tools(&self) {
        let still_open = self.picture_tools.get() || self.header_tools.get();
        let current = self.tab_index.get();
        let back = if still_open { current } else {
            let before = self.tab_before_tools.take();
            if current == INSERT_TAB { before.unwrap_or(current) } else { current }
        };
        self.select_tab(back);
    }

    fn open_table_tools(&self) {
        self.table_tools.set(true);
        self.select_tab(INSERT_TAB);
        self.focus_document();
    }

    fn close_table_tools(&self) {
        self.table_tools.set(false);
        self.select_tab(self.tab_index.get());
        self.focus_document();
    }

    fn focus_document(&self) {
        if self.find_has_focus() { return; }
        if self.ui.original_panel.is_shown() { self.ui.original_surface.set_focus(); }
        else if self.mode.get() == ViewMode::Typst { self.ui.typst_editor.set_focus(); }
        else { focus_canvas(&self.ui.paper_canvas); self.ui.paper_canvas.refresh(false, None); }
    }

    fn set_view(&self, mode: ViewMode) {
        if mode != ViewMode::Graphical && self.state.borrow().revisions.is_original() {
            self.close_original();
        }
        self.mode.set(mode);
        self.ui.workspace_panel.freeze();
        self.ui.source_panel.show(mode != ViewMode::Graphical);
        if mode != ViewMode::Graphical { self.install_face(&self.ui.source_close_button); }
        self.ui.canvas_container.show(mode != ViewMode::Typst && !self.ui.original_panel.is_shown());
        self.ui.split_handle.show(mode == ViewMode::Split);
        self.apply_split();
        self.ui.workspace_panel.layout();
        self.ui.body_panel.layout(); self.ui.workspace_panel.thaw();
        self.last_key.borrow_mut().take();
        if mode != ViewMode::Graphical { self.request_source(); }
        self.state.borrow_mut().needs_present = true;
        self.focus_document();
    }

    /// Shares the width between the document and its source as the handle
    /// last left it; outside Split view each side takes what it needs.
    fn apply_split(&self) {
        let (canvas, source) = (self.ui.canvas_container, self.ui.source_panel);
        if self.mode.get() != ViewMode::Split {
            canvas.set_min_size(Size::new(-1, -1)); source.set_min_size(Size::new(-1, -1));
            return;
        }
        let mut total = self.ui.workspace_panel.get_client_size().width - self.ui.split_handle.get_size().width;
        for side in [self.ui.equation_panel, self.ui.styles_panel, self.ui.styles_resize_handle] {
            if side.is_shown() { total -= side.get_size().width; }
        }
        let minimum = self.ui.workspace_panel.from_dip_int(240);
        if total < 2 * minimum { return; }
        let document = ((total as f32 * self.split_ratio.get()).round() as i32).clamp(minimum, total - minimum);
        canvas.set_min_size(Size::new(document, -1));
        source.set_min_size(Size::new(total - document, -1));
    }

    /// The handle between the document and its source in Split view, dragged
    /// like the Styles pane's.
    fn bind_split_handle(self: &Rc<Self>) {
        let handle = self.ui.split_handle;
        handle.hide();
        handle.set_background_style(BackgroundStyle::Paint);
        handle.on_paint(move |_| {
            let dc = AutoBufferedPaintDC::new(&handle);
            dc.set_background(Colour::rgb(232, 236, 242)); dc.clear();
            let size = handle.get_client_size();
            let stroke = handle.from_dip(Size::new(1, 1)).width.max(1);
            dc.set_brush(Colour::rgb(202, 211, 223), BrushStyle::Solid);
            dc.set_pen(Colour::rgb(202, 211, 223), stroke, PenStyle::Solid);
            dc.draw_rectangle((size.width - stroke) / 2, 0, stroke, size.height);
        });
        // Measured on the screen, as the handle itself moves while dragged.
        let drag = Rc::new(Cell::new(None::<(i32, i32, i32)>));
        { let drag = Rc::clone(&drag); let weak = Rc::downgrade(self);
          handle.on_mouse_left_down(move |event| {
            if let (WindowEventData::MouseButton(mouse), Some(r)) = (&event, weak.upgrade()) {
                if let Some(point) = mouse.get_position() {
                    let screen = crate::gtk_native::to_screen(&handle, point);
                    let document = r.ui.canvas_container.get_size().width;
                    let total = document + r.ui.source_panel.get_size().width;
                    drag.set(Some((screen.x, document, total)));
                    if !handle.has_capture() { handle.capture_mouse(); }
                    crate::native_input::set_pointer(crate::objects_ui::Pointer::ResizeColumn);
                }
            }
            event.skip(false);
          }); }
        { let drag = Rc::clone(&drag); let weak = Rc::downgrade(self);
          handle.on_mouse_motion(move |event| {
            crate::native_input::set_pointer(crate::objects_ui::Pointer::ResizeColumn);
            if let (Some((start_x, start_width, total)), WindowEventData::MouseMotion(mouse), Some(r)) = (drag.get(), &event, weak.upgrade()) {
                if let Some(point) = mouse.get_position() {
                    let screen = crate::gtk_native::to_screen(&handle, point);
                    let width = start_width + screen.x - start_x;
                    r.split_ratio.set((width as f32 / total.max(1) as f32).clamp(0.1, 0.9));
                    r.apply_split();
                    r.ui.workspace_panel.layout(); r.ui.body_panel.layout();
                    r.state.borrow_mut().needs_present = true;
                }
            }
            event.skip(false);
          }); }
        { let drag = Rc::clone(&drag); handle.on_mouse_left_up(move |event| {
            drag.set(None); if handle.has_capture() { handle.release_mouse(); }
            crate::native_input::set_pointer(crate::objects_ui::Pointer::Arrow);
            event.skip(false);
          }); }
        { let drag = Rc::clone(&drag); handle.on_mouse_leave(move |event| {
            if drag.get().is_none() { crate::native_input::set_pointer(crate::objects_ui::Pointer::Arrow); }
            event.skip(true);
          }); }
        // The split keeps its proportions as the window is resized.
        let weak = Rc::downgrade(self);
        self.ui.workspace_panel.on_size(move |event| {
            if let Some(r) = weak.upgrade() { if r.mode.get() == ViewMode::Split { r.apply_split(); } }
            event.skip(true);
        });
    }

    fn zoom_to(&self, zoom: f32) {
        {
            let mut state = self.state.borrow_mut(); let document = Arc::clone(&state.vector_document);
            let anchor = Point::new(state.viewport.viewport_px.width * 0.5, state.viewport.viewport_px.height * 0.5);
            state.viewport.zoom_at(anchor, zoom, &document); state.needs_present = true;
        }
        self.ui.paper_canvas.refresh(false, None);
    }

    fn fit_zoom(&self, whole_page: bool) -> Option<f32> {
            let state = self.state.borrow();
            let index=state.overlay.caret.as_ref().map_or(0,|caret|caret.page_index);
            let page = state.vector_document.pages.get(index).or_else(||state.vector_document.pages.first())?;
            let scale = state.viewport.dpi / 72.0;
            let width = (state.viewport.viewport_px.width - 2.0 * state.viewport.outer_margin_px).max(1.0);
            let height = (state.viewport.viewport_px.height - 2.0 * state.viewport.outer_margin_px).max(1.0);
            let w = width / (page.size.width * scale);
            Some((if whole_page { w.min(height / (page.size.height * scale)) } else { w }).clamp(render::ViewportState::MIN_ZOOM,render::ViewportState::MAX_ZOOM))
    }

    fn fit(&self, whole_page: bool) {
        let Some(zoom)=self.fit_zoom(whole_page) else{return;};
        self.zoom_to(zoom);
        if whole_page {
            let mut state=self.state.borrow_mut();
            let index=state.overlay.caret.as_ref().map_or(0,|caret|caret.page_index);
            let document=Arc::clone(&state.vector_document);
            if let Some(rect)=state.viewport.page_rect_px(&document,index) {
                let offset=rect.y-state.viewport.outer_margin_px;
                state.viewport.scroll_by(0.0,offset,&document);
                state.needs_present=true;
            }
        }
        self.focus_document();
    }

    fn scroll_to(&self, vertical: bool, position: f32) {
        if self.syncing.get() { return; }
        {
            let mut state = self.state.borrow_mut(); let document = Arc::clone(&state.vector_document);
            if vertical { state.viewport.scroll_px.y = position; } else { state.viewport.scroll_px.x = position; }
            state.viewport.clamp_scroll(&document); state.needs_present = true;
        }
        self.ui.paper_canvas.refresh(false, None);
    }

    fn show_navigation(&self, show: bool) {
        self.ui.navigator_panel.show(show); self.ui.body_panel.layout();
        if show { self.populate_navigation(); } else if !self.find_visible() { let mut state=self.state.borrow_mut();state.search_marks=None;refresh_overlay(&mut state); }
        self.last_key.borrow_mut().take();
    }

    fn populate_navigation(&self) { self.start_search(); }

    fn navigate_selected(&self) {
        let entry = self.ui.navigation_list.get_selection().and_then(|index| self.navigation.borrow().get(index as usize).cloned());
        let Some(entry) = entry else { return; };
        self.search_cursor.set(self.ui.navigation_list.get_selection().map(|i|i as usize));
        self.navigate_entry(entry);
    }

    fn navigate_entry(&self, entry:navigation::Entry) {
        if self.mode.get() == ViewMode::Typst { self.set_view(ViewMode::Split); }
        {
            let mut state = self.state.borrow_mut();
            state.editor.set_caret(entry.node.clone(), entry.start);
            if entry.end > entry.start { state.editor.extend_selection(entry.node.clone(), entry.end); }
            state.caret_visual_line = None; state.preferred_caret_x_pt = None;
            let document = Arc::clone(&state.vector_document);
            let target = document.pages.iter().enumerate().find_map(|(pi, page)| {
                page.hit_regions.iter().find(|r| r.node_id == entry.node.0 && r.byte_range.end >= entry.start)
                    .and_then(|r| state.viewport.page_point_to_view(&document, pi, Point::new(r.rect.x, r.rect.y)))
            });
            if let Some(target) = target {
                let dy = target.y - state.viewport.outer_margin_px;
                state.viewport.scroll_by(0.0, dy, &document);
            }
            refresh_overlay(&mut state); wake_caret(&mut state);
        }
        self.last_key.borrow_mut().take(); self.focus_document();
    }

    fn request_source(&self) {
        let revision = self.state.borrow().editor.revision;
        if self.source_revision.get() == Some(revision) {
            if self.source_copy_pending.replace(false) {
                Clipboard::get().set_text(&self.ui.typst_editor.get_value());
                self.state.borrow_mut().action_status = Some("Generated Typst source copied.".into());
            }
            return;
        }
        if self.source_job.borrow().is_some() { return; }
        let document = crate::document_snapshot(&mut self.state.borrow_mut());
        let (sender, receiver) = mpsc::channel();
        self.source_job.replace(Some(receiver));
        self.ui.source_caption.set_label(&crate::i18n::text(&("Generating Typst · read-only")));
        std::thread::spawn(move || {
            let source = format_typst::export_structured(&document);
            let _ = sender.send((revision, source));
        });
    }

    fn poll_source(&self) {
        let message = self.source_job.borrow().as_ref().map(|rx| rx.try_recv());
        match message {
            Some(Ok((revision, source))) => {
                self.source_job.borrow_mut().take();
                if revision == self.state.borrow().editor.revision {
                    typst_highlight::show(self.ui.typst_editor, &source);
                    self.ui.source_caption.set_label(&crate::i18n::text(&("Generated Typst · read-only")));
                    self.source_revision.set(Some(revision));
                }
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.source_job.borrow_mut().take();
                self.state.borrow_mut().action_status = Some("Typst source worker stopped unexpectedly.".into());
            }
            _ => {}
        }
        if self.mode.get() != ViewMode::Graphical || self.source_copy_pending.get() { self.request_source(); }
    }

    fn refresh_art(&self) {
        let dpi = self.ui.main_frame.from_dip_int(96).max(72);
        if self.dpi.replace(dpi) == dpi { return; }
        self.prepared_pages.borrow_mut().clear();
        self.ui.root_panel.freeze();
        generated::scale_controls(&self.ui);
        font_preview::align_with_size(self.ui.font_family_combo, self.ui.font_size_combo);
        // The wxDragon face already paints its own icon. Building a second
        // seven-resolution bundle for every ribbon button duplicated startup
        // bitmap work and was immediately overwritten by the face bundle.
        // Startup: size the buttons for their faces before drawing them.
        self.fit_visible_buttons();
        self.install_visible_faces();
        generated::layout_groups(&self.ui);
        let group_height = generated::group_height(&self.ui);
        // Reserve space for a horizontal scrollbar in narrow windows. Never
        // shrink command groups or overlap their captions.
        let page_height = self.ui.main_frame.from_dip_int(generated::PAGE_HEIGHT_DIP)
            .max(group_height + self.ui.main_frame.from_dip_int(24));
        for page in [self.ui.ribbon_file, self.ui.ribbon_home, self.ui.ribbon_insert, self.ui.ribbon_layout,
            self.ui.ribbon_references, self.ui.ribbon_review, self.ui.ribbon_view, self.ui.ribbon_typst] {
            page.set_min_size(Size::new(1, page_height));
            page.set_scroll_rate(page.from_dip_int(16), 0);
            page.layout();
        }
        self.ui.ribbon_notebook.set_min_size(Size::new(1, page_height + self.ui.main_frame.from_dip_int(36)));
        self.button_states.replace(vec![None; self.buttons.len()]);
        self.last_key.borrow_mut().take();
        self.ui.root_panel.layout();
        self.ui.main_frame.layout();
        self.ui.root_panel.thaw();
    }

    /// Paints the face of one button outside the Ribbon pages, such as the
    /// Styles pane's close button, which is hidden when the Ribbon starts.
    pub(super) fn install_face(&self, button: &Button) {
        self.install_faces_where(|spec| spec.button.get_handle() == button.get_handle());
    }
    /// Paints the faces of the buttons `chosen` picks, such as a context
    /// strip that was hidden when the Ribbon started.
    fn install_faces_where(&self, chosen: impl Fn(&ButtonSpec) -> bool) {
        for spec in self.buttons.iter().filter(|spec| chosen(spec)) {
            self.fit_button(spec, true);
            if !skin::install(spec) { spec.button.set_label(&crate::i18n::text(&(spec.label))); self.fit_button(spec, false); }
        }
    }
    /// Sizes the shown buttons for their faces, before the page is laid out.
    fn fit_visible_buttons(&self) {
        for spec in self.buttons.iter().filter(|spec| shown_in_hierarchy(&spec.button)) { self.fit_button(spec, true); }
    }

    /// Draws the faces of the shown buttons at their current size; true when
    /// a button fell back to its plain caption and was resized for it.
    fn install_visible_faces(&self) -> bool {
        let mut missing = Vec::new();
        for spec in &self.buttons {
            if !shown_in_hierarchy(&spec.button) { continue; }
            if !skin::install(spec) {
                spec.button.set_label(&crate::i18n::text(&(spec.label)));
                self.fit_button(spec, false);
                missing.push(spec.name);
            }
        }
        if !missing.is_empty() {
            self.state.borrow_mut().action_status = Some(format!("Ribbon fallback: {}", missing.join(", ")));
        }
        self.button_states.replace(vec![None; self.buttons.len()]);
        !missing.is_empty()
    }

    fn fit_button(&self, spec: &ButtonSpec, has_art: bool) {
        let button = &spec.button;
        let desired = button.from_dip(Size::new(spec.width_dip, spec.height_dip));
        let actual_label = spec.label;
        let localized_label = crate::i18n::text(actual_label);
        let actual_label = localized_label.as_str();
        let label = if has_art { skin::label_size(spec) } else { button.get_text_extent(actual_label) };
        let art = if has_art {
            button.from_dip(Size::new(spec.art_width_dip + 2 * art::INSET_DIP as i32,
                spec.art_height_dip + 2 * art::INSET_DIP as i32))
        } else { Size::new(0, 0) };
        if has_art {
            // Our painter fills the complete native control. Native wx bitmap
            // margins no longer dictate its size or a separate selection square.
            let art = button.from_dip(Size::new(spec.art_width_dip, spec.art_height_dip));
            let top = matches!(spec.kind, "large" | "wide" | "gallery" | "file-main");
            let gap = if actual_label.is_empty() { 0 } else { button.from_dip_int(4) };
            let (w,h) = if actual_label.is_empty() { (art.width,art.height) }
                else if top { (art.width.max(label.width),art.height+gap+label.height) }
                else { (art.width+gap+label.width,art.height.max(label.height)) };
            let pad = button.from_dip_int(if top { 5 } else { 4 });
            let fitted = Size::new(desired.width.max(w+2*pad),desired.height.max(h+2*pad));
            button.set_min_size(fitted); button.set_size(fitted);
            return;
        }
        let native = button.get_best_size();
        let gap = if has_art && !actual_label.is_empty() { button.from_dip_int(6) } else { 0 };
        let (content_w, content_h) = if actual_label.is_empty() { (art.width, art.height) }
            else if matches!(spec.kind, "large" | "wide" | "gallery" | "file-main") {
                (art.width.max(label.width), art.height + gap + label.height)
            } else { (art.width + gap + label.width, art.height.max(label.height)) };
        let inset_x = button.from_dip_int(if matches!(spec.kind, "large" | "wide" | "gallery" | "file-main") { 8 } else { 5 });
        let inset_y = button.from_dip_int(if matches!(spec.kind, "large" | "wide" | "gallery" | "file-main") { 8 } else { 4 });
        let fitted = Size::new(desired.width.max(native.width).max(content_w + 2 * inset_x),
            desired.height.max(native.height).max(content_h + 2 * inset_y));
        button.set_min_size(fitted);
        button.set_size(fitted);
    }

    /// Called by the existing UI timer with NO CanvasState borrow held. Native
    /// events may be synchronous; all widget updates run outside editor borrows.
    pub fn poll(&self) {
        self.sync_output_progress();
        let request=self.state.borrow_mut().documents.request.take();
        if let Some(documents::Request::Close(id))=request{
            if let Err(error)=self.close_document(id){self.state.borrow_mut().action_status=Some(error);}
        }
        let epoch=self.state.borrow().documents.epoch;
        if self.document_epoch.replace(epoch)!=epoch{self.document_shown();}
        self.sync_original_preview();
        self.sync_revision_controls();
        self.doc_switcher.borrow().as_ref().map(|switcher|switcher.refresh());
        dropdown::poll();font_preview::poll();
        content_controls::poll(self);
        typst_highlight::poll(self.ui.typst_editor);
        if self.last_theme_check.get().elapsed() >= Duration::from_secs(2) { self.last_theme_check.set(Instant::now()); self.follow_theme(); }
        let print=std::mem::take(&mut self.state.borrow_mut().pending_print);
        if print { self.run(Action::Print); }
        let spelling = std::mem::take(&mut self.state.borrow_mut().pending_spelling);
        if spelling { self.run(Action::Spelling); }
        let find=self.state.borrow_mut().pending_find.take();
        if let Some(replace)=find { self.open_find_dialog(replace); }
        self.poll_search();
        self.poll_find_dialog();
        let pending = self.state.borrow_mut().pending_equation.take();
        if let Some(id) = pending {
            if let Err(error) = self.open_equation(Some(id), model::EquationDisplay::Inline) { self.state.borrow_mut().action_status = Some(error); }
        }
        // After opening the pane on a clicked equation: its cursor goes to
        // the slot that was clicked.
        let slot = self.state.borrow_mut().equation_slot_click.take();
        if let (Some((fx, fy)), Some(editor)) = (slot, self.equation.borrow().clone()) { editor.click_at(fx, fy); }
        let inline = self.state.borrow_mut().inline_equation_request.take();
        if let Some(id) = inline { inline_equation::show(self, id); }
        let pending = self.state.borrow_mut().pending_new_equation.take();
        if let Some(display) = pending {
            if let Err(error) = self.open_equation(None, display) { self.state.borrow_mut().action_status = Some(error); }
        }
        let new_comment = std::mem::take(&mut self.state.borrow_mut().pending_new_comment);
        if new_comment { self.run(Action::CommentNew); }
        let pending_comment=self.state.borrow_mut().pending_comment.take();
        if let Some(id)=pending_comment{let pane=self.comments.borrow().clone();if let Some(p)=pane{p.select(Some(id));}}
        let layout_menu=self.state.borrow_mut().layout_menu_request.take();
        if let Some(rect)=layout_menu{
            // View pixels are physical; screen positions are logical on GTK.
            let canvas=self.ui.paper_canvas;
            let scale=crate::display_scale::window_scale(&canvas).max(0.01);
            let origin=crate::gtk_native::to_screen(&canvas,wxdragon::geometry::Point::new((rect.x/scale) as i32,(rect.y/scale) as i32));
            let screen=wxdragon::geometry::Rect::new(origin.x,origin.y,(rect.width/scale) as i32,(rect.height/scale) as i32);
            picture_layout::show_wrap_menu(self,screen);
        }
        self.sync_context();
        if self.last_poll.get().elapsed() < Duration::from_millis(80) { return; }
        self.last_poll.set(Instant::now());
        self.refresh_art(); self.poll_source(); self.poll_spelling(); self.poll_dictionary_install(); self.poll_dictionary_manager(); self.sync_spelling_status();
        if let Some(notes) = self.comments.borrow().as_ref() { notes.sync_view(); }
        if let Some(ruler) = self.tab_ruler.borrow().as_ref() { ruler.refresh(); }
        let (key, title, status, zoom, metrics, can_undo, can_redo, page, pages, section, editing_header_footer) = {
            let state = self.state.borrow();
            let cells = match &state.selected_object {
                Some(objects_ui::SelectedObject::Cells { table_id, cell_ids }) => Some((table_id.clone(), cell_ids.clone())),
                _ => None,
            };
            let key = EditKey { revision: state.editor.revision, tracking: state.editor.document.review.tracking_enabled(),
                selection: state.editor.selection.clone(),
                typing: state.editor.typing_properties.clone(), cells };
            let pages = state.vector_document.pages.len().max(1);
            let visible = state.viewport.visible_pages(&state.vector_document);
            let page = state.overlay.caret.as_ref().map(|caret| caret.page_index)
                .or(state.caret_page_hint).filter(|index| visible.contains(index))
                .or_else(|| visible.first().copied()).unwrap_or(0) + 1;
            let section = state.editor.section_properties_at_caret();
            let editing_header_footer = state.editor.header_footer_context().map(|context| context.title());
            let status = if let Some(error) = &state.layout_error { format!("Layout: {error}") }
                else if let Some(error) = &state.recovery.error { error.clone() }
                else if state.layout_inflight.is_some() && (state.display_generation.is_none() && state.current_path.is_some()
                    || visible.iter().any(|&index| !state.vector_document.pages[index].has_paint)) { crate::i18n::text("Laying out document…") }
                else if let Some(message) = &state.action_status { message.clone() }
                else {
                    (if self.mode.get() == ViewMode::Typst { "Read-only source" } else if state.editor.document.review.track_text { "Tracking paragraph text" } else { "Ready" }).to_string()
                };
            let canvas = state.viewport.canvas_size_px(&state.vector_document);
            let metrics = (
                scroll_metrics(state.viewport.scroll_px.y, state.viewport.viewport_px.height, canvas.height),
                scroll_metrics(state.viewport.scroll_px.x, state.viewport.viewport_px.width, canvas.width),
            );
            (key, document_title(&state), status, (state.viewport.zoom * 100.0).round() as i32, metrics,
                state.editor.can_undo(), state.editor.can_redo(), page, pages, section, editing_header_footer)
        };
        if *self.last_title.borrow() != title {
            self.ui.main_frame.set_title(&crate::i18n::text(&(&format!("Impress — {title}"))));
            self.ui.document_name_label.set_label(&crate::i18n::text(&(&format!("{title} · Impress"))));
            self.last_title.replace(title);
        }
        if *self.last_status.borrow() != status {
            // A long error should not force the status sizer wider than the window.
            let translated = crate::i18n::text(&status);
            self.ui.status_label.set_tooltip(&translated);
            let brief = translated.chars().take(48).collect::<String>();
            self.ui.status_label.set_label(&brief);
            self.last_status.replace(status);
            // A label does not move its neighbours; the sizer has to.
            self.ui.status_panel.layout();
        }
        if self.label_changed("section", "Section") { self.ui.section_context_label.set_label(&crate::i18n::text(&("Section"))); }
        let (paper, columns) = match section.as_ref() {
            Some(props) => (paper_label(props).to_string(), format!("{} Column{}", props.column_count.max(1), if props.column_count == 1 { "" } else { "s" })),
            None => ("Custom".to_string(), "1 Column".to_string()),
        };
        if self.label_changed("paper", &paper) { self.ui.paper_context_label.set_label(&crate::i18n::text(&(&paper))); }
        if self.label_changed("columns", &columns) { self.ui.columns_context_label.set_label(&crate::i18n::text(&(&columns))); }
        let editing_note={let s=self.state.borrow();s.editor.selection.as_ref().is_some_and(|v|s.editor.note_for_paragraph(&v.focus.node_id).is_some())};
        let editing = if editing_note { "Footnote · Esc to return".to_string() }
            else if let Some(title) = &editing_header_footer { format!("{title} · Esc to return") }
            else { "Body".to_string() };
        if self.label_changed("editing", &editing) { self.ui.editing_context_label.set_label(&crate::i18n::text(&(&editing))); }
        let page_text = format!("Page {page} of {pages}");
        if self.label_changed("page", &page_text) {
            self.ui.page_context_label.set_label(&crate::i18n::text(&(&page_text)));
            self.ui.status_page_count.set_label(&crate::i18n::text(&(&page_text)));
            self.ui.status_panel.layout();
        }
        let word_text = {
            let state = self.state.borrow();
            let cells = key.cells.as_ref().map(|(_, cells)| cells.as_slice());
            let mut count = self.word_count.borrow_mut();
            count.update(&state.editor, self.document_epoch.get(), cells);
            crate::i18n::text(&count.label())
        };
        if self.label_changed("words", &word_text) {
            self.ui.status_word_count.set_label(&word_text);
            self.ui.status_panel.layout();
        }
        self.syncing.set(true);
        self.ui.zoom_out_button.enable(zoom>(render::ViewportState::MIN_ZOOM*100.0) as i32);
        self.ui.zoom_in_button.enable(zoom<(render::ViewportState::MAX_ZOOM*100.0) as i32);
        if self.ui.zoom_slider.value() != zoom { self.ui.zoom_slider.set_value(zoom); }
        let zoom_text = format!("{zoom}%");
        if self.label_changed("zoom", &zoom_text) { self.ui.zoom_label.set_label(&crate::i18n::text(&(&zoom_text))); }
        if self.last_scroll.replace(Some(metrics)) != Some(metrics) {
            for (bar, (position, thumb, range)) in [(self.ui.vertical_scroll, metrics.0), (self.ui.horizontal_scroll, metrics.1)] {
                bar.set_scrollbar(position, thumb, range, thumb, true); bar.enable(range > thumb);
            }
        }
        let changed = self.last_key.borrow().as_ref() != Some(&key) || self.state.borrow().equation_draft.is_some();
        if changed {
            if let Some(gallery) = self.style_gallery.borrow().as_ref() { gallery.refresh(); }
            self.sync_style_caption();
            self.refresh_styles_pane();
            let (format, section) = { let state = self.state.borrow(); (state.formatting_summary(), state.editor.section_properties_at_caret()) };
            self.sync_format(&format, section.as_ref());
            self.sync_buttons(&format, section.as_ref(), can_undo, can_redo, key.cells.is_some() || key.selection.as_ref().is_some_and(|s| !s.is_collapsed()));
            self.last_key.replace(Some(key.clone()));
            self.last_activity.set(Instant::now());
        }
        self.syncing.set(false);
        self.prepare_hidden_page_when_idle();
        if self.search_visible() && self.navigation_revision.get() != Some(key.revision) { self.populate_navigation(); }
    }

    fn sync_format(&self, format: &FormattingSummary, section: Option<&SectionProperties>) {
        let enabled = self.mode.get() != ViewMode::Typst;
        let rgb = |value:Option<&str>|value.and_then(styles::hex_colour).map(|c|[c.r,c.g,c.b]);
        skin::colour(&self.ui.font_color_button, art::ColourIndicator::Colour(rgb(format.character.color.as_deref()).unwrap_or([0,0,0])));
        skin::colour(&self.ui.highlight_button, rgb(format.character.highlight.as_deref()).map(art::ColourIndicator::Colour).unwrap_or(art::ColourIndicator::NoColour));
        self.ui.font_family_combo.enable(enabled);
        self.ui.font_family_input.enable(enabled);
        for combo in [self.ui.font_size_combo, self.ui.line_spacing_combo, self.ui.space_before_combo, self.ui.space_after_combo] { combo.enable(enabled); }
        let family = format.character.font_family.clone().unwrap_or_default();
        if !self.ui.font_family_input.has_focus() && self.ui.font_family_input.get_value() != family {
            self.ui.font_family_input.change_value(&family);
        }
        sync_combo(self.ui.font_size_combo, format_number(format.character.font_size_pt));
        sync_combo(self.ui.line_spacing_combo, format_number(format.line_spacing));
        sync_combo(self.ui.space_before_combo, format_number(format.space_before_pt));
        sync_combo(self.ui.space_after_combo, format_number(format.space_after_pt));
        if let Some(props) = section {
            sync_combo(self.ui.paper_size_combo, crate::i18n::text(paper_label(props)));
            sync_combo(self.ui.margins_combo, crate::i18n::text(margins_label(props)));
        }
    }

    fn sync_buttons(&self, format: &FormattingSummary, section: Option<&SectionProperties>, undo: bool, redo: bool, selection: bool) {
        let mut states = self.button_states.borrow_mut();
        for (index, spec) in self.buttons.iter().enumerate() {
            if matches!(spec.action, Action::ReviewPane | Action::RevisionResolve(_) | Action::RevisionStep(_)) {
                continue;
            }
            let selected = match spec.action {
                Action::SelectRibbonTab(index) => self.tab_index.get()==index as usize,
                Action::Editor(AppCommand::Bold) => format.character.bold == Some(true),
                Action::Editor(AppCommand::Italic) => format.character.italic == Some(true),
                Action::Editor(AppCommand::Underline) => format.character.underline == Some(true),
                Action::Editor(AppCommand::StyleNormal) => format.style.as_ref().is_some_and(|id| id.0 == model::STYLE_NORMAL),
                Action::Editor(AppCommand::StyleHeading1) => format.style.as_ref().is_some_and(|id| id.0 == model::STYLE_HEADING_1),
                Action::Editor(AppCommand::StyleTitle) => format.style.as_ref().is_some_and(|id| id.0 == model::STYLE_TITLE),
                Action::Align(value) => format.alignment == Some(value),
                Action::Direction(value) => format.direction == Some(value),
                Action::Landscape(value) => section.is_some_and(|p| (p.page_width_mm > p.page_height_mm) == value),
                Action::View(mode) => self.mode.get() == mode,
                Action::ToggleNavigation | Action::ShowNavigation => self.ui.navigator_panel.is_shown(),
                Action::Comments => self.comments.borrow().as_ref().is_some_and(|notes| notes.shown()),
                Action::TrackChanges => self.state.borrow().editor.document.review.tracking_enabled(),
                _ => false,
            };
            let enabled = !spec.disabled && match spec.action {
                Action::StyleApply | Action::StyleModify | Action::StyleInspect | Action::StyleDelete => self.styles_view.borrow().has_selection(),
                Action::Editor(AppCommand::Undo) => undo,
                Action::CommentDelete => comment_notes::current(&self.state.borrow()).is_some(),
                Action::CommentResolve => {
                    let state = self.state.borrow();
                    comment_notes::current(&state).and_then(|id|
                        state.editor.document.review.comments.get(&id)).is_some_and(|c| !c.is_closed())
                }
                Action::CommentStep(_) => !self.state.borrow().editor.document.review.comments.is_empty(),
                Action::Editor(AppCommand::Redo) => redo,
                Action::Editor(AppCommand::Cut | AppCommand::Copy) => selection,
                Action::Contents | Action::ContentsLevel => self.mode.get() != ViewMode::Typst,
                Action::ContentsUpdate => self.mode.get() != ViewMode::Typst
                    && !self.state.borrow().editor.document.review.contents_paragraphs.is_empty(),
                Action::Editor(_) | Action::GrowFont | Action::ShrinkFont | Action::FontColor | Action::Typography |
                    Action::Align(_) | Action::Direction(_) | Action::Indent(_) | Action::Symbol | Action::PageBreak | Action::SectionBreak => self.mode.get() != ViewMode::Typst,
                _ => true,
            };
            if states[index] != Some((selected, enabled)) {
                spec.button.enable(enabled);
                skin::selected(&spec.button, selected);
                spec.button.set_accessibility_description(&crate::i18n::text(&(&format!("{}{}", spec.tip, if selected { " · selected" } else { "" }))));
                states[index] = Some((selected, enabled));
            }
        }
    }
}

fn shown_in_hierarchy(window: &impl WxWidget) -> bool {
    if !window.is_shown() { return false; }
    let mut parent = window.get_parent();
    while let Some(window) = parent {
        if !window.is_shown() { return false; }
        parent = window.get_parent();
    }
    true
}

fn sync_combo(combo: ComboBox, value: String) {
    if !combo.has_focus() && combo.get_value() != value { combo.change_value(&value); }
}
fn number(value: &str) -> Result<f32, String> {
    let number = value.replace(',', ".").parse::<f32>().map_err(|_| "Enter a numeric value.".to_string())?;
    if !number.is_finite() { return Err("Enter a finite numeric value.".into()); }
    Ok(number)
}
fn format_number(value: Option<f32>) -> String {
    value.map(|v| format!("{v:.2}").trim_end_matches('0').trim_end_matches('.').to_string()).unwrap_or_default()
}
fn scroll_metrics(position: f32, visible: f32, canvas: f32) -> ScrollMetrics {
    let thumb = visible.round().clamp(1.0, i32::MAX as f32 / 2.0) as i32;
    let range = canvas.ceil().max(thumb as f32).min(i32::MAX as f32 / 2.0) as i32;
    ((position.round() as i32).clamp(0, range - thumb), thumb, range)
}
fn paper_label(p: &SectionProperties) -> &'static str {
    let w = p.page_width_mm.min(p.page_height_mm); let h = p.page_width_mm.max(p.page_height_mm);
    for (label, pw, ph) in [("A4",210.0_f32,297.0_f32),("Letter",215.9,279.4),("Legal",215.9,355.6),("A5",148.0,210.0)] {
        if (w - pw).abs() < 0.1 && (h - ph).abs() < 0.1 { return label; }
    }
    "Custom"
}
fn margins_label(p: &SectionProperties) -> &'static str {
    for (label, value) in [("Normal (25.4 mm)",25.4_f32),("Narrow (12.7 mm)",12.7),("Wide (38.1 mm)",38.1)] {
        if [p.margin_left_mm,p.margin_right_mm,p.margin_top_mm,p.margin_bottom_mm].iter().all(|margin| (margin - value).abs() < 0.1) { return label; }
    }
    "Custom"
}

