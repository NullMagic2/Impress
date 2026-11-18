#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod i18n;
mod placement;
mod commands;
mod document_defaults;
mod wheel;
mod file_io;
mod save_options;
mod recent_files;
mod equation_ui;
mod equation_editor;
mod objects_ui;
mod picture_chrome;
mod header_footer_ui;
mod display_scale;
mod slot_ui;
mod crisp;
mod footnote_ui;
#[cfg(windows)] mod native_print;
mod table_ui;
mod table_snap;
mod native_input;
mod wx_canvas;
mod system_clipboard;
#[cfg(target_os = "linux")]
mod cairo_frame;
mod data_dir;
mod recovery;
mod spelling;
mod dictionary_install;
mod proofing;
mod inline_spelling;
mod ribbon;
mod output_ui;
mod background_io;
mod documents;
mod title_bar;
mod gtk_native;
mod clipboard_jobs;
mod shortcuts;
mod hyperlinks;

use commands::AppCommand;
use file_io::{ensure_extension_for_filter, load_document, save_document, document_open_wildcard, document_save_wildcard, save_filter_index};
use model::{Block, Document, Equation, EquationNode, NodeId, Paragraph};
use editor::{ DocumentPosition, EditorState, Selection};
use format::FormatRegistry;
use docx::DocxFormat;
use format_native::NativeImpressFormat;
use format_typst::TypstFormat;
use layout::{LayoutOptions, LayoutScheduler};
use render::{
    overlay_for_selection_with_caret_line, visual_vertical_target, CanvasOverlay, Point, SourcePosition, SourceSelection,
    VectorDocument, ViewportState, VisualMoveDirection,
};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use wxdragon::{
    clipboard::Clipboard,
    dc::AutoBufferedPaintDC,
    dialogs::file_dialog::{FileDialog, FileDialogStyle},
    dialogs::message_dialog::{MessageDialog, MessageDialogStyle},
    event::WindowEventData,
    keycode::{
        WXK_BACK, WXK_DELETE, WXK_DOWN, WXK_END, WXK_ESCAPE, WXK_HOME, WXK_LEFT, WXK_NUMPAD_ENTER,
        WXK_ADD, WXK_NUMPAD_ADD, WXK_NUMPAD_SUBTRACT, WXK_PAGEDOWN, WXK_PAGEUP,
        WXK_RETURN, WXK_RIGHT, WXK_SUBTRACT, WXK_TAB, WXK_UP, WXK_CONTROL,
    },
    prelude::*,
    timer::Timer,
    BackgroundStyle,
};

wxdragon::include_xrc!("../ui/main.xrc", MainUi);
wxdragon::include_xrc!("../ui/find_dialog.xrc", FindDialogUi);
wxdragon::include_xrc!("../ui/layout_dialog.xrc", LayoutDialogUi);
wxdragon::include_xrc!("../ui/citation_dialog.xrc", CitationDialogUi);
wxdragon::include_xrc!("../ui/style_editor.xrc", StyleEditorUi);
// Format ▾ dialogs of Modify Style (tools/build_format_dialogs.py).
wxdragon::include_xrc!("../ui/format_font.xrc", FontDialogUi);
wxdragon::include_xrc!("../ui/pdf_export.xrc", PdfExportDialogUi);
wxdragon::include_xrc!("../ui/pdf_import.xrc", PdfImportDialogUi);
wxdragon::include_xrc!("../ui/print_dialog.xrc", PrintDialogUi);
wxdragon::include_xrc!("../ui/table_position.xrc", TablePositionDialogUi);
wxdragon::include_xrc!("../ui/note_dialog.xrc", NoteDialogUi);
wxdragon::include_xrc!("../ui/columns_dialog.xrc", ColumnsDialogUi);
wxdragon::include_xrc!("../ui/recovery_dialog.xrc", RecoveryDialogUi);
wxdragon::include_xrc!("../ui/format_paragraph.xrc", ParagraphDialogUi);
wxdragon::include_xrc!("../ui/format_tabs.xrc", TabsDialogUi);
wxdragon::include_xrc!("../ui/format_borders.xrc", BordersDialogUi);
wxdragon::include_xrc!("../ui/format_language.xrc", LanguageDialogUi);
wxdragon::include_xrc!("../ui/format_frame.xrc", FrameDialogUi);
wxdragon::include_xrc!("../ui/format_numbering.xrc", NumberingDialogUi);
wxdragon::include_xrc!("../ui/format_shortcut.xrc", ShortcutDialogUi);
wxdragon::include_xrc!("../ui/format_text_effects.xrc", TextEffectsDialogUi);

struct CanvasState {
    editor: EditorState,
    snapshot: Option<(u64, Arc<Document>)>,
    output_job: Option<output_ui::Job>,
    file_job: Option<background_io::Job>,
    pending_open: Option<PathBuf>,
    /// Files dropped on the window, opened one after another.
    dropped_files: std::collections::VecDeque<PathBuf>,
    clipboard_job: Option<clipboard_jobs::Job>,
    pending_close: bool,
    close_ready: bool,
    search_marks: Option<(u64,u64,Vec<render::ColoredOverlayRect>)>,
    formats: Arc<FormatRegistry>,
    current_path: Option<PathBuf>,
    layout_generation: u64,
    layout_inflight: Option<u64>,
    typing_layout_pending: bool,
    display_generation: Option<u64>,
    caret_page_hint: Option<usize>,
    /// Scroll the caret into view when the layout of this revision arrives,
    /// for commands that both change the document and move the caret (such
    /// as entering a footer that did not exist yet).
    reveal_caret_at: Option<u64>,
    selected_object: Option<objects_ui::SelectedObject>,
    object_drag: Option<objects_ui::Drag>,
    equation_draft: Option<equation_ui::EquationDraft>,
    pending_equation: Option<NodeId>,
    /// Ctrl+M (inline) or Ctrl+Shift+M (display): the Ribbon opens the
    /// equation pane for a new equation on its next poll.
    pending_new_equation: Option<model::EquationDisplay>,
    /// The in-line equation box to open on the Ribbon's next poll: on this
    /// equation, or (None) for a new one at the caret.
    inline_equation_request: Option<Option<NodeId>>,
    /// A click on the equation the pane edits, as a fraction of its width and
    /// height: the pane puts its cursor in the slot there.
    equation_slot_click: Option<(f32, f32)>,
    /// The slot of an equation selected on the page, being typed into.
    equation_slot: Option<slot_ui::SlotSelection>,
    /// The equation the equation pane edits and the slot its cursor is in,
    /// shown on the page with the same blue box as a slot selected there.
    equation_pane_slot: Option<(NodeId, Vec<(usize, usize)>)>,
    /// The comment selected in the Comments pane.
    comment_selected: Option<NodeId>,
    pending_comment: Option<NodeId>,
    /// The Layout Options button beside a selected picture was pressed: its
    /// rectangle in view pixels, for the Ribbon to open the Wrap Text menu below.
    layout_menu_request: Option<render::Rect>,
    pending_find: Option<bool>,
    /// Ctrl+P: the Ribbon opens the Print dialog on its next poll.
    pending_print: bool,
    pending_spelling: bool,
    inline_spelling: inline_spelling::InlineSpelling,
    input: Rc<native_input::InputState>,
    recovery: recovery::Recovery,
    preferred_caret_x_pt: Option<f32>,
    caret_visual_line: Option<usize>,
    scheduler: LayoutScheduler,
    vector_document: Arc<VectorDocument>,
    viewport: ViewportState,
    /// Physical pixels per logical canvas pixel. The viewport and everything
    /// in view pixels is physical, so the page renders at the screen's own
    /// resolution; wx mouse positions are logical and go through `view_point`.
    pixel_scale: f32,
    overlay: CanvasOverlay,
    dragging_selection: bool,
    caret_visible: bool,
    last_caret_toggle: Instant,
    needs_present: bool,
    /// Wheel scrolling still to glide through, in pixels (x, y).
    wheel_glide: (f32, f32),
    layout_error: Option<String>,
    action_status: Option<String>,
    renderer: wx_canvas::WxCanvasRenderer,
    /// Last pointer position over the canvas, in view pixels.
    table_pointer: Option<Point>,
    table_hover: Option<table_ui::Hover>,
    /// The picture and picture control under the pointer.
    picture_hover: Option<picture_chrome::Hover>,
    /// Row, column or table selected through its on-page handle.
    table_band: Option<(String, table_ui::Band)>,
    table_drag: Option<table_ui::TableDrag>,
    band_drag: Option<table_ui::BandDrag>,
    /// The cell where a mouse selection started: (table, cell).
    cell_drag_anchor: Option<(NodeId, NodeId)>,
    /// Where the pointer is during a selection drag (view pixels), for
    /// scrolling while it is held past the top or bottom of the page view.
    drag_pointer: Option<Point>,
    /// Shift was held for the current left press: it extends the selection.
    shift_click: bool,
    /// A held key being repeated by the UI timer.
    key_repeat: Option<KeyRepeat>,
    /// Key code of the last key-down that produced a character event.
    last_key_down: Option<i32>,
    /// The other documents open in this window.
    documents: documents::Stack,
}

fn main() {
    i18n::initialize();
    let mut formats = FormatRegistry::default();
    formats.register(NativeImpressFormat);
    formats.register(TypstFormat);
    formats.register(DocxFormat);
    formats.register(format_doc::DocFormat);
    formats.register(format_rtf::RtfFormat);
    formats.register(format_pdf::PdfFormat::new(document_defaults::new_document));
    let formats = Arc::new(formats);

    let _ = wxdragon::main(|_| {
        i18n::install_native();
        // Nothing here handles idle events. By default wxWidgets dispatches one
        // to every window (hundreds of ribbon controls) after each timer tick.
        wxdragon::event::IdleEvent::set_mode(wxdragon::event::IdleMode::ProcessSpecified);
        // Nor update-UI events, which Impress does not use: by default every
        // window gets one after each batch of events (wxUPDATE_UI_PROCESS_SPECIFIED = 1).
        unsafe { wxd_UpdateUIEvent_SetMode(1); }
        let ui = Rc::new(crate::i18n::localized(MainUi::new(None, false)));
        if let Ok(image) = image::load_from_memory_with_format(
            include_bytes!("../assets/ribbon/logo.png"), image::ImageFormat::Png,
        ) {
            let image = image.into_rgba8();
            if let Some(icon) = wxdragon::Bitmap::from_rgba(image.as_raw(), image.width(), image.height()) {
                ui.main_frame.set_icon(&icon);
            }
        }
        ribbon::Ribbon::prepare(&ui);
        title_bar::install(&ui);
        ui.main_frame.show(true);
        ui.main_frame.centre();
        // The main window ends the app when it closes. Without a top window,
        // wxWidgets waits for every top-level window to go, and the hidden
        // dialogs loaded at startup kept the process running after Close.
        if let Some(app) = wxdragon::app::get_app_instance() { app.set_top_window(&ui.main_frame); app.set_exit_on_frame_delete(true); }
        placement::set_app(&ui.main_frame);
        ui.paper_canvas.set_background_style(BackgroundStyle::Paint);
        // Containers their children cover completely (expanding, no borders,
        // see main.xrc) need no background of their own: each would fill the
        // whole repainted area before its children paint over it, several
        // full-window fills per maximize or restore on a HiDPI screen.
        for panel in [ui.root_panel, ui.body_panel, ui.workspace_panel] { panel.set_background_style(BackgroundStyle::Paint); }

        let mut document = document_defaults::new_document();
        let first_paragraph = Paragraph::plain("");
        let first_node = first_paragraph.id.clone();
        document.sections[0].blocks.push(Block::Paragraph(first_paragraph));
        let mut editor = EditorState::new(document);
        editor.set_caret(first_node, 0);

        let scheduler = match LayoutScheduler::new(LayoutOptions::default()) {
            Ok(scheduler) => scheduler,
            Err(error) => {
                ui.status_label.set_label(&crate::i18n::text(&(&format!("Layout initialization failed: {error}"))));
                return;
            }
        };
        scheduler.submit(editor.revision, Arc::new(editor.document.clone()));

        let client = ui.paper_canvas.get_client_size();
        let mut viewport = ViewportState::default();
        let pixel_scale = fit_viewport(&mut viewport, &ui.paper_canvas, client.width, client.height);

        ui.status_label.set_label(&crate::i18n::text(&("wxDragon canvas · laying out document…")));

        let input = native_input::install(ui.paper_canvas.get_handle());
        let state = Rc::new(RefCell::new(CanvasState {
            editor,
            snapshot: None, output_job:None, file_job:None, pending_open:None, dropped_files:Default::default(), clipboard_job:None, pending_close:false, close_ready:false, search_marks:None,
            formats,
            current_path: None,
            layout_generation: 0,
            layout_inflight: Some(0),
            typing_layout_pending: false,
            display_generation: None,
            caret_page_hint: None,
            reveal_caret_at: None,
            selected_object: None,
            object_drag: None,
            equation_draft: None,
            pending_equation: None,
            pending_new_equation: None,
            inline_equation_request: None,
            equation_slot_click: None,
            equation_slot: None,
            comment_selected: None,
            equation_pane_slot: None,
            pending_comment: None,
            layout_menu_request: None,
            pending_find: None,
            pending_print: false,
            pending_spelling: false,
            inline_spelling: Default::default(),
            input,
            recovery: recovery::Recovery::new(),
            preferred_caret_x_pt: None,
            caret_visual_line: None,
            scheduler,
            vector_document: Arc::new(VectorDocument::default()),
            viewport,
            pixel_scale,
            overlay: CanvasOverlay::default(),
            dragging_selection: false,
            caret_visible: true,
            last_caret_toggle: Instant::now(),
            needs_present: true,
            wheel_glide: (0.0, 0.0),
            layout_error: None,
            action_status: None,
            renderer: wx_canvas::WxCanvasRenderer::default(),
            table_pointer: None, table_hover: None, picture_hover: None, table_band: None, table_drag: None, band_drag: None, cell_drag_anchor: None, drag_pointer: None, shift_click: false, key_repeat: None, last_key_down: None,
            documents: Default::default(),
        }));

        bind_canvas_events(&ui, Rc::clone(&state));
        let ribbon = ribbon::Ribbon::new(Rc::clone(&ui), Rc::clone(&state));
        // Offer recovered documents once the window is fully built and drawn,
        // never over a half-made frame. A one-shot timer cannot fire again
        // while its dialog is open.
        {
            let once: &'static Timer<Frame> = Box::leak(Box::new(Timer::new(&ui.main_frame)));
            let (frame, state) = (ui.main_frame.clone(), Rc::clone(&state));
            once.on_tick(move |_| {
                // Let a document named on the command line finish opening first.
                if state.borrow().file_job.is_some() { once.start(200, true); return; }
                frame.update();
                if let Err(error) = recover_from_ui(&frame, &state) { state.borrow_mut().action_status = Some(error); }
            });
            once.start(150, true);
        }
        // `impress <file>` opens a document, as file managers expect.
        let mut args = std::env::args_os().skip(1);
        let target = args.next().and_then(|arg| if arg == "--" { args.next() } else { Some(arg) });
        if let Some(arg) = target {
            state.borrow_mut().documents.startup_placeholder = true;
            if let Err(error) = file_io::launch_path(arg).and_then(|path| background_io::queue_open(path, &state)) {
                state.borrow_mut().action_status = Some(error);
            }
        }

        // A 60 Hz UI poll only checks for completed background layouts and asks
        // wxWidgets to invalidate when needed. Heavy layout never runs here.
        let timer: &'static Timer<Frame> = Box::leak(Box::new(Timer::new(&ui.main_frame)));
        bind_close(&ui, Rc::clone(&state), timer);
        bind_file_drop(&ui, &state);
        let timer_state = Rc::clone(&state);
        let timer_canvas = ui.paper_canvas.clone();
        let timer_ribbon = Rc::clone(&ribbon);
        let timer_frame = ui.main_frame.clone();
        let initial_focus = Cell::new(true);
        timer.on_tick(move |_| {
            // The first event-loop tick follows ribbon setup and any recovery
            // dialog. Focusing before those steps left a ribbon control active.
            if initial_focus.replace(false) { timer_canvas.set_focus(); }
            drain_native_input(&mut timer_state.borrow_mut());
            let mut state = timer_state.borrow_mut();
            if run_key_repeat(&mut state) { timer_canvas.refresh(false, None); }
            let glided = wheel::glide(&mut state);
            // A selection drag follows the page under the pointer as it scrolls.
            if drag_scroll(&mut state) || glided {
                if let Some(point) = state.drag_pointer.filter(|_| state.dragging_selection) { drag_selection_to(&mut state, point); }
                timer_canvas.refresh(false, None);
            }
            equation_ui::poll_preview(&mut state);
            inline_spelling::poll(&mut state);
            output_ui::poll(&mut state);
            background_io::poll(&mut state);
            open_dropped_files(&mut state);
            clipboard_jobs::poll(&mut state);
            {
                let snapshot=state.snapshot.as_ref().filter(|(r,_)|*r==state.editor.revision).map(|(_,d)|Arc::clone(d));
                let CanvasState { recovery, editor, current_path, formats, .. } = &mut *state;
                recovery.poll_snapshot(editor, current_path.as_deref(), formats,snapshot);
            }
            documents::poll_parked_recovery(&mut state);
            let mut layout_changed = false;
            while let Some(result) = state.scheduler.try_recv() {
                if state.layout_inflight != Some(result.revision) { continue; }
                state.layout_inflight = None;
                let is_current = result.revision == state.layout_generation;
                let is_current_preview = state.equation_draft.as_ref().is_none_or(|draft|
                    !draft.preview_live || draft.preview_generation == Some(result.revision));
                match result.layout {
                    Ok(layout) if is_current_preview => {
                        state.vector_document = layout.document;
                        let document = Arc::clone(&state.vector_document);
                        state.viewport.clamp_scroll(&document);
                        state.display_generation = Some(result.revision);
                        refresh_overlay(&mut state);
                        // A laid-out equation change goes into the document.
                        equation_ui::commit_preview(&mut state, result.revision);
                        if is_current && state.reveal_caret_at.take_if(|r| *r == result.revision).is_some() { reveal_review_caret(&mut state); }
                        if is_current { state.layout_error = layout.display_warning; }
                        state.needs_present = true;
                        layout_changed = true;
                    }
                    Err(error) => {
                        // Cancellation is expected while typing quickly and should not
                        // replace useful status text with a warning.
                        if is_current && !error.contains("newer document revision") {
                            if equation_ui::previewing(&state) { equation_ui::preview_failed(&mut state, result.revision, error); }
                            else { state.layout_error = Some(error); }
                        }
                    }
                    Ok(_) => {}
                }
                if state.typing_layout_pending {
                    state.typing_layout_pending = false;
                    let snapshot = document_snapshot(&mut state);
                    let generation = state.layout_generation;
                    state.scheduler.submit_for_display(generation, snapshot);
                    state.layout_inflight = Some(generation);
                }
            }

            let caret_is_collapsed = state.editor.selection.as_ref().is_some_and(Selection::is_collapsed);
            // A blink alone repaints just the caret's rectangle: a whole-canvas
            // repaint fills, copies and redraws every pixel of the view, which
            // on a HiDPI screen is a hiccup twice a second.
            let mut blink_only = None;
            if caret_is_collapsed && state.last_caret_toggle.elapsed() >= Duration::from_millis(530) {
                if !state.needs_present && !layout_changed { blink_only = caret_window_rect(&state); }
                state.caret_visible = !state.caret_visible;
                state.last_caret_toggle = Instant::now();
                state.needs_present = true;
            } else if !caret_is_collapsed {
                state.caret_visible = true;
            }

            if let Some(rect) = blink_only {
                timer_canvas.refresh(false, Some(&rect));
            } else if layout_changed || state.needs_present {
                timer_canvas.refresh(false, None);
            } else if state.layout_inflight.is_none() {
                // Idle: prepare the screens above and below for Page Up/Down.
                let document = Arc::clone(&state.vector_document);
                let viewport = state.viewport;
                state.renderer.prefetch(&document, &viewport);
            }

            // Releasing this borrow is essential: native layout and focus events
            // can dispatch synchronously while the Ribbon updates its controls.
            let close_ready=std::mem::take(&mut state.close_ready);
            drop(state);
            if close_ready {timer_frame.close(false);return;}
            timer_ribbon.poll();
            position_native_input(&timer_canvas, &timer_state);
        });
        timer.start(16, false);
    });
}

#[cfg(not(windows))]
thread_local! { static CANVAS_POINTER: Cell<Option<objects_ui::Pointer>> = const { Cell::new(None) }; }

/// The circular-arrow cursor shown over a picture's rotate handle, with its
/// hotspot in the middle; the stock crosshair if it cannot be made.
#[cfg(not(windows))]
fn rotate_cursor() -> Option<wxdragon::cursor::Cursor> {
    let (bits, mask) = picture_chrome::rotate_cursor_bits();
    let size = picture_chrome::ROTATE_CURSOR_PX as i32;
    wxdragon::cursor::Cursor::from_data(&bits, size, size, size / 2, size / 2, Some(&mask))
        .or_else(|| wxdragon::cursor::Cursor::from_stock(wxdragon::cursor::StockCursor::Cross))
}

fn bind_canvas_events(ui: &MainUi, state: Rc<RefCell<CanvasState>>) {
    let canvas = ui.paper_canvas.clone();
    system_clipboard::set_owner(ui.main_frame.get_handle());

    // wxDragon owns the canvas paint on every supported platform.
    let paint_state = Rc::clone(&state);
    let paint_canvas = canvas.clone();
    canvas.on_paint(move |_event| {
        // The persistent cairo frame already buffers pixels on Linux. Avoid
        // allocating and copying a second full-window bitmap on every key.
        #[cfg(target_os = "linux")]
        let dc = wxdragon::dc::PaintDC::new(&paint_canvas);
        #[cfg(not(target_os = "linux"))]
        let dc = AutoBufferedPaintDC::new(&paint_canvas);
        let mut state = paint_state.borrow_mut();
        let document = Arc::clone(&state.vector_document);
        let viewport = state.viewport;
        let mut overlay = state.overlay.clone();
        if !state.caret_visible { overlay.caret = None; }
        overlay.chrome = header_footer_ui::chrome(&state);
        overlay.chrome.extend(table_ui::chrome(&state));
        overlay.chrome.extend(slot_ui::chrome(&state));
        overlay.chrome.extend(picture_chrome::chrome(&state));
        let dirty = state.needs_present;
        // Keep painting on the next ticks while visible tiles are still missing.
        let scale = state.pixel_scale;
        state.needs_present = state.renderer.render(&dc, &document, &viewport, &overlay, dirty, scale);
    });

    canvas.on_erase_background(|_event| {
        // The persistent frame supplies every pixel. Avoid a separate
        // background erase between frames, which would cause visible flashing.
    });

    let resize_state = Rc::clone(&state);
    let resize_canvas = canvas.clone();
    canvas.on_size(move |event| {
        if let WindowEventData::Size(size_event) = &event {
            if let Some(size) = size_event.get_size() {
                let mut state = resize_state.borrow_mut();
                let prior_view = state.viewport;
                let prior_scale = state.pixel_scale;
                state.pixel_scale = fit_viewport(&mut state.viewport, &resize_canvas, size.width, size.height);
                if state.viewport == prior_view && state.pixel_scale == prior_scale { event.skip(true); return; }
                let document = Arc::clone(&state.vector_document);
                state.viewport.clamp_scroll(&document);
                state.needs_present = true;
                resize_canvas.refresh(false, None);
            }
        }
        event.skip(true);
    });

    let wheel_state = Rc::clone(&state);
    let wheel_canvas = canvas.clone();
    canvas.on_mouse_wheel(move |event| {
        // Installed Win32 subclass owns wheel delivery. This is the portable or
        // subclass-install-failure fallback only.
        if wheel_state.borrow().input.installed() { event.skip(false); return; }
        if let Some(mouse) = mouse_data(&event) {
            // Same semantics as the native path: Ctrl zooms, Shift or a
            // horizontal wheel scrolls sideways.
            let delta = mouse.get_wheel_delta().max(1);
            // wxDragon reports modifiers for key events only; GTK has them.
            let (shift, control) = gtk_native::scroll_modifiers();
            let input = wheel::Input {
                rotation: mouse.get_wheel_rotation() * 120 / delta,
                horizontal: mouse.get_wheel_axis() == wxdragon::event::MouseWheelAxis::Horizontal,
                shift: shift || mouse.event.shift_down(), control: control || mouse.event.control_down(), units: 3, page_scroll: false,
            };
            wheel::apply(&mut wheel_state.borrow_mut(), input);
            wheel_canvas.refresh(false, None);
        }
        event.skip(false);
    });

    let magnify_state = Rc::clone(&state);
    let magnify_canvas = canvas.clone();
    canvas.on_magnify(move |event| {
        if let Some(mouse) = mouse_data(&event) {
            let magnification = mouse.get_magnification();
            let mut state = magnify_state.borrow_mut();
            let anchor = mouse.get_position().map(|p| logical_to_view(p, state.pixel_scale)).unwrap_or_else(|| {
                Point::new(state.viewport.viewport_px.width * 0.5, state.viewport.viewport_px.height * 0.5)
            });
            let document = Arc::clone(&state.vector_document);
            let requested = state.viewport.zoom * (1.0 + magnification).clamp(0.5, 1.5);
            state.viewport.zoom_at(anchor, requested, &document);
            state.needs_present = true;
            magnify_canvas.refresh(false, None);
        }
        event.skip(false);
    });

    let down_state = Rc::clone(&state);
    let down_canvas = canvas.clone();
    canvas.on_mouse_left_down(move |event| {
        if let WindowEventData::MouseButton(mouse) = &event {
            if let Some(position) = mouse.get_position() {
                let capture = {
                    let mut state = down_state.borrow_mut();
                    drain_native_input(&mut state);
                    if state.input.active() { event.skip(false); return; }
                    let view_point = logical_to_view(position, state.pixel_scale);
                    if (mouse.event.event.control_down() || wxdragon::utils::get_key_state(WXK_CONTROL)) && !mouse.event.event.alt_down() {
                        if let Some(target)=hyperlinks::under(&state,view_point) {
                            state.dragging_selection=false;state.cell_drag_anchor=None;state.drag_pointer=None;
                            state.key_repeat=None;
                            drop(state);
                            if let Err(error)=hyperlinks::follow(&down_state,&target) {down_state.borrow_mut().action_status=Some(error);}
                            down_canvas.refresh(false,None);event.skip(false);return;
                        }
                    }
                    // A click ends typing into an equation slot (a click on
                    // another slot selects that one below).
                    slot_ui::finish(&mut state);
                    // A click on the equation being edited goes to its slot in
                    // the equation pane, such as an empty box to fill.
                    if objects_ui::draft_equation_click(&mut state, view_point) {
                        drop(state); down_canvas.refresh(false, None); event.skip(false); return;
                    }
                    // Anywhere else, the equation pane's slot is no longer shown.
                    if state.equation_pane_slot.take().is_some() { state.needs_present = true; }
                    // Clicking elsewhere in the document edits it; the equation editor stays open.
                    equation_ui::show_document(&mut state);
                    state.shift_click = state.input.shift_down() || gtk_native::press_with_shift();
                    // Windows reports double-clicks through the native input
                    // bridge; elsewhere two quick clicks in one spot are one.
                    if !state.input.installed() && !state.shift_click && is_double_click(position) {
                        objects_ui::double_click(&mut state, view_point);
                        drop(state);
                        down_canvas.set_focus(); down_canvas.refresh(false, None);
                        event.skip(false); return;
                    }
                    state.table_pointer = Some(view_point);
                    if picture_chrome::down(&mut state, view_point) {
                        drop(state);
                        down_canvas.set_focus(); down_canvas.refresh(false, None);
                        event.skip(false); return;
                    }
                    if table_ui::down(&mut state, view_point) {
                        let capture = state.table_drag.is_some() || state.band_drag.is_some();
                        drop(state);
                        if capture && !down_canvas.has_capture() { down_canvas.capture_mouse(); }
                        down_canvas.set_focus(); down_canvas.refresh(false, None);
                        event.skip(false); return;
                    }
                    state.table_band = None;
                    if !objects_ui::down(&mut state, view_point) {
                        state.selected_object = None;
                        state.comment_selected = None;
                        state.dragging_selection = false;
                        state.cell_drag_anchor = None;
                        if let Some(hit) = state.viewport.hit_test(&state.vector_document, view_point) {
                            // Shift+click extends the selection from where it began
                            // (its anchor) to the click, as in Word and Windows text
                            // boxes; holding the button and dragging goes on extending.
                            if state.shift_click && state.editor.selection.is_some() {
                                state.editor.extend_selection(NodeId::from(hit.node_id.as_str()), hit.byte_offset);
                            } else {
                                state.editor.set_caret(NodeId::from(hit.node_id.as_str()), hit.byte_offset);
                            }
                            state.preferred_caret_x_pt = None;
                            state.caret_visual_line = Some(hit.visual_line);
                            state.caret_page_hint = Some(hit.page_index);
                            state.dragging_selection = true;
                            state.cell_drag_anchor = state.editor.selection.as_ref().and_then(|s| state.editor.table_cell_of(&s.anchor.node_id));
                        } else {
                            state.editor.selection = None;
                            state.table_hover = None;
                            state.table_drag = None;
                            state.band_drag = None;
                            state.object_drag = None;
                            state.action_status = None;
                        }
                    }
                    wake_caret(&mut state); refresh_overlay(&mut state);
                    state.dragging_selection || state.object_drag.is_some()
                };
                // Capture/focus may synchronously dispatch native callbacks.
                if capture && !down_canvas.has_capture() { down_canvas.capture_mouse(); }
                down_canvas.set_focus(); down_canvas.refresh(false, None);
            }
        }
        event.skip(false);
    });

    let motion_state = Rc::clone(&state);
    let motion_canvas = canvas.clone();
    canvas.on_mouse_motion(move |event| {
        let WindowEventData::MouseMotion(mouse) = &event else { return; };
        let Some(position) = mouse.get_position() else { return; };
        let mut state = motion_state.borrow_mut();
        drain_native_input(&mut state);
        let view_point = logical_to_view(position, state.pixel_scale);
        let unlock = state.input.shift_down();
        if table_ui::motion(&mut state, view_point, unlock) {}
        else if !objects_ui::motion(&mut state, view_point, unlock) && state.dragging_selection {
            state.drag_pointer = Some(view_point);
            drag_selection_to(&mut state, view_point);
        }
        let guide = objects_ui::resize_guide(&state);
        state.table_pointer = Some(view_point);
        let table_changed = table_ui::hover(&mut state, Some(view_point));
        let picture_changed = picture_chrome::hover(&mut state, Some(view_point));
        if objects_ui::resize_guide(&state) != guide { state.needs_present = true; }
        let link=(!state.dragging_selection && state.object_drag.is_none()).then(||hyperlinks::under(&state,view_point)).flatten();
        let note_tip = if state.dragging_selection { None } else {
            link.as_ref().map(|target|format!("{}\n{target}",i18n::text("Ctrl+click to follow link")))
                .or_else(||footnote_ui::tooltip(&state, view_point))
        };
        let tooltip = (table_changed || picture_changed)
            .then(|| picture_chrome::tooltip(&state).unwrap_or_else(|| table_ui::tooltip(&state)).to_string())
            .or_else(|| (NOTE_TOOLTIP.with(|t| t.replace(note_tip.clone())) != note_tip).then(|| note_tip.clone().unwrap_or_default()));
        let mut pointer = objects_ui::hover(&state, view_point);
        if pointer==objects_ui::Pointer::Arrow && link.is_some() && wxdragon::utils::get_key_state(WXK_CONTROL) {
            pointer=objects_ui::Pointer::Hand;
        }
        let needs_present = state.needs_present;
        drop(state);
        native_input::set_pointer(pointer);
        if let Some(tip) = tooltip { motion_canvas.set_tooltip(&crate::i18n::text(&(&tip))); }
        #[cfg(not(windows))]
        if CANVAS_POINTER.with(|p| p.replace(Some(pointer))) != Some(pointer) {
            use objects_ui::Pointer::*;
            let stock = match pointer {
                Arrow => wxdragon::cursor::StockCursor::IBeam, Hand => wxdragon::cursor::StockCursor::Hand,
                ResizeColumn => wxdragon::cursor::StockCursor::SizeWE, ResizeRow => wxdragon::cursor::StockCursor::SizeNS,
                ResizeNwSe => wxdragon::cursor::StockCursor::SizeNWSE, ResizeNeSw => wxdragon::cursor::StockCursor::SizeNESW,
                Move => wxdragon::cursor::StockCursor::Sizing,
                // wx has no stock rotate cursor; this one is drawn below.
                Rotate => wxdragon::cursor::StockCursor::Cross,
            };
            let cursor = if pointer == Rotate { rotate_cursor() } else { wxdragon::cursor::Cursor::from_stock(stock) };
            if let Some(cursor) = cursor { motion_canvas.set_cursor(Some(&cursor)); }
        }
        if needs_present { motion_canvas.refresh(false, None); }
    });

    let leave_state = Rc::clone(&state);
    let leave_canvas = canvas.clone();
    canvas.on_mouse_leave(move |event| {
        let mut state = leave_state.borrow_mut();
        if state.object_drag.is_none() {
            let guide = objects_ui::resize_guide(&state).is_some();
            state.table_pointer = None;
            let picture_changed = picture_chrome::hover(&mut state, None);
            if table_ui::hover(&mut state, None) || picture_changed || guide { state.needs_present = true; drop(state); leave_canvas.refresh(false, None); }
        }
        event.skip(true);
    });

    let up_state = Rc::clone(&state);
    let up_canvas = canvas.clone();
    canvas.on_mouse_left_up(move |event: WindowEventData| {
        {
            let mut state = up_state.borrow_mut();
            drain_native_input(&mut state);
            if table_ui::up(&mut state) { up_canvas.refresh(false, None); }
            else if let Err(error) = objects_ui::up(&mut state) { state.action_status = Some(error); }
            state.dragging_selection = false;
            state.cell_drag_anchor = None;
            state.drag_pointer = None;
        }
        if up_canvas.has_capture() { up_canvas.release_mouse(); }
        event.skip(false);
    });

    let key_state = Rc::clone(&state);
    let key_canvas = canvas.clone();
    let key_frame = ui.main_frame.clone();
    canvas.on_key_down(move |event| {
        let WindowEventData::Keyboard(key) = &event else {
            event.skip(true);
            return;
        };
        // Some non-Latin key-down events have no wx key code. Native
        // hardware translation can still identify their command key.
        let shortcut_letter = shortcuts::letter(key);
        let Some(code) = key.get_key_code().or_else(|| shortcut_letter.map(|c| c as i32)) else {
            key_state.borrow_mut().last_key_down = None;
            event.skip(true);
            return;
        };

        if code == wxdragon::keycode::WXK_F7 && !key.cmd_down() && !key.alt_down() {
            key_state.borrow_mut().pending_spelling = true;
            event.skip(false);
            return;
        }
        drain_native_input(&mut key_state.borrow_mut());
        if key_state.borrow().input.active() { event.skip(true); return; }
        // Typing in the document edits it; the equation editor stays open.
        equation_ui::show_document(&mut key_state.borrow_mut());
        // A selected equation slot takes the editing keys.
        if slot_ui::key_down(&mut key_state.borrow_mut(), code, key.cmd_down()) {
            key_canvas.refresh(false, None); event.skip(false); return;
        }
        if code==WXK_ESCAPE && key_state.borrow().clipboard_job.is_some(){
            clipboard_jobs::cancel(&mut key_state.borrow_mut());event.skip(false);return;
        }
        if code==WXK_ESCAPE && key_state.borrow().pending_close{
            let mut s=key_state.borrow_mut();s.pending_close=false;s.action_status=Some("Close cancelled; the background save may finish normally.".into());event.skip(false);return;
        }
        if code == WXK_ESCAPE && table_ui::cancel_drag(&mut key_state.borrow_mut()) {
            if key_canvas.has_capture() { key_canvas.release_mouse(); }
            key_canvas.refresh(false, None); event.skip(false); return;
        }
        if code == WXK_ESCAPE && key_state.borrow().object_drag.is_some() {
            objects_ui::cancel_drag(&mut key_state.borrow_mut());
            if key_canvas.has_capture() { key_canvas.release_mouse(); }
            event.skip(false); return;
        }
        // Esc on a selected picture deselects it, which closes the Picture tools.
        if code == WXK_ESCAPE && objects_ui::deselect(&mut key_state.borrow_mut()) {
            key_canvas.refresh(false, None); event.skip(false); return;
        }
        // Word's Insert Footnote (Ctrl+Alt+F) and Insert Endnote (Ctrl+Alt+D).
        if key.cmd_down() && key.alt_down() && matches!(shortcut_letter, Some('F' | 'D')) {
            let kind = if shortcut_letter == Some('F') { model::NoteKind::Footnote } else { model::NoteKind::Endnote };
            let mut state = key_state.borrow_mut();
            match state.editor.insert_note_here(kind) {
                Ok(()) => { submit_layout(&mut state); refresh_overlay(&mut state); reveal_review_caret(&mut state); wake_caret(&mut state); }
                Err(error) => state.action_status = Some(error),
            }
            drop(state);
            key_canvas.refresh(false, None); event.skip(false); return;
        }
        // Alt+1…9 shows the document at that place in the switcher.
        if key.alt_down() && !key.cmd_down() && (i32::from(b'1')..=i32::from(b'9')).contains(&code) {
            let mut s = key_state.borrow_mut();
            if let Some(id) = s.documents.id_at((code - i32::from(b'1')) as usize) {
                if let Err(error) = documents::switch_to(&mut s, id) { s.action_status = Some(error); }
            }
            drop(s);
            key_canvas.refresh(false, None); event.skip(false); return;
        }
        if key.cmd_down() && !key.alt_down() {
            if let Some(letter) = shortcut_letter {
                if matches!(letter,'F'|'H') {key_state.borrow_mut().pending_find=Some(letter=='H');event.skip(false);return;}
                if letter=='P' {key_state.borrow_mut().pending_print=true;event.skip(false);return;}
                if letter=='M' {
                    // Both insert at the caret: Ctrl+Shift+M in the equation
                    // pane, Ctrl+M typed in place.
                    let mut s=key_state.borrow_mut();
                    if key.shift_down() {s.pending_new_equation=Some(model::EquationDisplay::Inline);} else {s.inline_equation_request=Some(None);}
                    event.skip(false);return;
                }
                if letter=='W' && !key.shift_down() {
                    let mut s=key_state.borrow_mut();let id=s.documents.active_id();s.documents.request=Some(documents::Request::Close(id));
                    event.skip(false);return;
                }

                let command = match letter {
                    'O' => Some(AppCommand::Open),
                    'S' if key.shift_down() => Some(AppCommand::SaveAs),
                    'S' => Some(AppCommand::Save),
                    'Z' if key.shift_down() => Some(AppCommand::Redo),
                    'Z' => Some(AppCommand::Undo),
                    'Y' => Some(AppCommand::Redo),
                    'X' => Some(AppCommand::Cut),
                    'C' => Some(AppCommand::Copy),
                    'V' if key.shift_down() => Some(AppCommand::PastePlain),
                    'V' => Some(AppCommand::Paste),
                    'A' => Some(AppCommand::SelectAll),
                    'B' => Some(AppCommand::Bold),
                    'I' => Some(AppCommand::Italic),
                    'U' => Some(AppCommand::Underline),
                    _ => None,
                };
                if let Some(command) = command {
                    match command {
                        AppCommand::Open => {if let Err(error)=request_open(&key_frame,&key_state){key_state.borrow_mut().action_status=Some(error);}},
                        AppCommand::Save => { if let Err(e)=background_io::queue_save(&key_frame,&key_state,false){key_state.borrow_mut().action_status=Some(e);} },
                        AppCommand::SaveAs => { if let Err(e)=background_io::queue_save(&key_frame,&key_state,true){key_state.borrow_mut().action_status=Some(e);} },
                        other => {
                            let result = execute_editor_app_command(&mut key_state.borrow_mut(), other);
                            if let Err(error) = result { key_state.borrow_mut().action_status = Some(error); }
                        }
                    }
                    key_canvas.refresh(false, None);
                    event.skip(false);
                    return;
                }
            }

            // Ctrl/Cmd +/-/0 changes only the viewport transform; it never
            // schedules layout or reshapes glyphs.
            let factor = zoom_key_factor(code);
            if let Some(factor) = factor {
                let mut state = key_state.borrow_mut();
                let anchor = Point::new(state.viewport.viewport_px.width * 0.5, state.viewport.viewport_px.height * 0.5);
                let document = Arc::clone(&state.vector_document);
                let requested = if factor == 0.0 { 1.0 } else { state.viewport.zoom * factor };
                state.viewport.zoom_at(anchor, requested, &document);
                state.needs_present = true;
                key_canvas.refresh(false, None);
                event.skip(false);
                return;
            }
        }

        if let Some(chord) = ribbon::shortcut_text(key) {
            let target = key_state.borrow().editor.document.styles.style_for_shortcut(&chord);
            if let Some(target) = target {
                let mut state = key_state.borrow_mut();
                let old_revision = state.editor.revision;
                if let Err(error) = state.editor.apply_named_style(&target, false) {
                    state.action_status = Some(format!("Style shortcut failed: {error}"));
                }
                if state.editor.revision != old_revision { submit_layout(&mut state); }
                refresh_overlay(&mut state);
                wake_caret(&mut state);
                key_canvas.refresh(false, None);
                event.skip(false);
                return;
            }
        }

        let mut state = key_state.borrow_mut();
        if matches!(code, WXK_BACK | WXK_DELETE) && state.selected_object.is_some() {
            match objects_ui::delete_selection(&mut state) {
                Ok(true) => { event.skip(false); return; },
                Err(error) => { state.action_status = Some(error); event.skip(false); return; },
                Ok(false) => {}
            }
        }
        // Moving the caret leaves a selected picture, which closes its tools.
        if matches!(code, WXK_LEFT | WXK_RIGHT | WXK_UP | WXK_DOWN | WXK_HOME | WXK_END | WXK_PAGEUP | WXK_PAGEDOWN) {
            objects_ui::deselect(&mut state);
        }
        let shift = key.shift_down();
        let cmd = key.cmd_down();
        // A held key repeats from the UI timer (see `KeyRepeat`); the
        // system's own repeats of it are ignored.
        if state.key_repeat.as_ref().is_some_and(|r| r.key == RepeatKey::Code(code)) { event.skip(false); return; }
        let (consumed, repaint) = apply_key(&mut state, code, shift, cmd);
        if consumed {
            if repeatable(code) { state.key_repeat = Some(KeyRepeat::new(RepeatKey::Code(code), code, shift, cmd)); }
            if repaint { key_canvas.refresh(false, None); }
            event.skip(false);
        } else {
            state.last_key_down = Some(code);
            event.skip(true);
        }
    });

    // Printable Unicode text is handled through CHAR rather than KEY_DOWN so
    // keyboard layouts and composed Unicode input reach the semantic editor.
    let up_key_state = Rc::clone(&state);
    canvas.on_key_up(move |event| {
        if let WindowEventData::Keyboard(key) = &event {
            let code = key.get_key_code();
            let mut state = up_key_state.borrow_mut();
            if state.key_repeat.as_ref().is_some_and(|r| r.physical == code || r.key == RepeatKey::Code(code.unwrap_or(-1))) { state.key_repeat = None; }
        }
        event.skip(true);
    });
    let focus_state = Rc::clone(&state);
    canvas.on_kill_focus(move |event| { focus_state.borrow_mut().key_repeat = None; event.skip(true); });

    let char_state = Rc::clone(&state);
    canvas.on_char(move |event| {
        let WindowEventData::Keyboard(key) = &event else {
            event.skip(true);
            return;
        };
        if key.cmd_down() && !key.alt_down() {
            event.skip(true);
            return;
        }
        let Some(code) = key.get_unicode_key() else {
            event.skip(true);
            return;
        };
        let Some(ch) = char::from_u32(code as u32) else {
            event.skip(true);
            return;
        };
        if ch.is_control() {
            event.skip(true);
            return;
        }
        let mut state = char_state.borrow_mut();
        drain_native_input(&mut state);
        if state.input.active() { event.skip(false); return; }
        equation_ui::show_document(&mut state);
        if slot_ui::typed(&mut state, ch) { state.needs_present = true; event.skip(false); return; }
        if state.key_repeat.as_ref().is_some_and(|r| r.key == RepeatKey::Char(ch)) { event.skip(false); return; }
        state.selected_object = None;
        let physical = state.last_key_down.take();
        state.key_repeat = physical.map(|code| KeyRepeat::new(RepeatKey::Char(ch), code, false, false));
        match state.editor.insert_text(&ch.to_string()) {
            Ok(()) => {
                state.preferred_caret_x_pt = None;
                state.caret_visual_line = None;
                // A closing `$` turns typed Typst math into an equation.
                let converted = ch == '$' && state.editor.convert_typed_math().unwrap_or(false);
                if converted { state.key_repeat = None; submit_layout(&mut state); } else { submit_typing_layout(&mut state); }
                wake_caret(&mut state);
                state.action_status = converted.then(|| "Typst math became an equation; Ctrl+Z gives the text back".into());
                event.skip(false);
            }
            Err(error) => {
                state.action_status = Some(format!("Typing failed: {error}"));
                event.skip(false);
            }
        }
    });
}

/// Mouse details of wheel and gesture events, whichever category wxDragon
/// assigned them to.
fn mouse_data(event: &WindowEventData) -> Option<&wxdragon::event::event_data::MouseEventData> {
    match event {
        WindowEventData::MouseButton(e) => Some(&e.event),
        WindowEventData::MouseMotion(e) => Some(&e.event),
        WindowEventData::Magnify(e) => Some(&e.event),
        _ => None,
    }
}

fn execute_editor_app_command(state: &mut CanvasState, command: AppCommand) -> Result<(), String> {
    equation_ui::show_document(state);
    let old_revision = state.editor.revision;
    match command {
        AppCommand::Cut|AppCommand::Copy|AppCommand::Paste|AppCommand::PastePlain=>return clipboard_jobs::queue(state,command),
        AppCommand::InsertInlineEquation | AppCommand::InsertEquation => {
            return Err("Use Insert Equation to open the equation editor.".into());
        }
        AppCommand::Undo
        | AppCommand::Redo
        | AppCommand::SelectAll
        | AppCommand::Bold
        | AppCommand::Italic
        | AppCommand::Underline
        | AppCommand::StyleNormal
        | AppCommand::StyleHeading1
        | AppCommand::StyleTitle => {
            let editor_command = command
                .editor_command()
                .ok_or_else(|| format!("no editor command mapping for {command:?}"))?;
            state.editor.execute(editor_command)?;
        }
        AppCommand::Open | AppCommand::Save | AppCommand::SaveAs => {}
    }

    if state.editor.revision != old_revision {
        state.preferred_caret_x_pt = None;
        state.caret_visual_line = None;
        submit_layout(state);
        state.action_status = None;
    }
    refresh_overlay(state);
    wake_caret(state);
    Ok(())
}

fn document_snapshot(state:&mut CanvasState)->Arc<Document>{
    if let Some((revision,doc))=&state.snapshot {if *revision==state.editor.revision{return Arc::clone(doc);}}
    let doc=Arc::new(state.editor.document.clone());state.snapshot=Some((state.editor.revision,Arc::clone(&doc)));doc
}
fn submit_layout(state: &mut CanvasState) {
    // Laying out the document itself ends an equation preview; it is laid
    // out again after the next pause.
    equation_ui::interrupt_preview(state);
    state.layout_generation = state.layout_generation.wrapping_add(1);
    let snapshot=document_snapshot(state);
    state.scheduler.submit_for_display(state.layout_generation,snapshot);
    state.layout_inflight = Some(state.layout_generation);
    state.typing_layout_pending = false;
    state.needs_present = true;
}

/// Key repeat must not cancel every layout before it can finish. Keep the
/// running result available and replace the pending snapshot with the latest
/// text once that result reaches the UI thread.
fn submit_typing_layout(state: &mut CanvasState) {
    equation_ui::interrupt_preview(state);
    state.layout_generation = state.layout_generation.wrapping_add(1);
    if state.layout_inflight.is_some() {
        state.typing_layout_pending = true;
    } else {
        let snapshot = document_snapshot(state);
        state.scheduler.submit_for_display(state.layout_generation, snapshot);
        state.layout_inflight = Some(state.layout_generation);
    }
}

/// The caret's area in window pixels, with a margin, for repainting it alone.
fn caret_window_rect(state: &CanvasState) -> Option<wxdragon::geometry::Rect> {
    let caret = state.overlay.caret.as_ref()?;
    let page = *state.viewport.page_rects_px(&state.vector_document).get(caret.page_index)?;
    let scale = state.viewport.points_to_pixels();
    // View pixels are physical; the window works in logical pixels.
    let k = state.pixel_scale.max(1.0);
    let x = (page.x + caret.rect.x * scale) / k;
    let y = (page.y + caret.rect.y * scale) / k;
    let (w, h) = (caret.rect.width * scale / k, caret.rect.height * scale / k);
    let (x0, y0) = (x.floor() as i32 - 2, y.floor() as i32 - 2);
    Some(wxdragon::geometry::Rect::new(x0, y0, (x + w).ceil() as i32 + 2 - x0, (y + h).ceil() as i32 + 2 - y0))
}

unsafe extern "C" {
    /// From the vendored wxdragon-sys (see vendor/wxdragon-sys/IMPRESS_CHANGES.md).
    fn wxd_UpdateUIEvent_SetMode(mode: i32);
}

fn refresh_overlay(state: &mut CanvasState) {
    state.overlay = editor_overlay(&state.editor.selection, &state.vector_document, state.caret_visual_line);
    if state.overlay.caret.is_none() && state.display_generation != Some(state.layout_generation) {
        // While key repeat is ahead of the latest painted layout, keep the
        // caret at the last laid-out character instead of making it vanish.
        if let Some(focus) = state.editor.selection.as_ref().filter(|s| s.is_collapsed()).map(|s| &s.focus) {
            let last = state.vector_document.pages.iter().enumerate()
                .flat_map(|(page, p)| p.hit_regions.iter().filter(move |r| r.node_id == focus.node_id.0)
                    .map(move |r| (page, r.byte_range.end)))
                .max_by_key(|(_, end)| *end);
            if let Some((page, end)) = last {
                state.overlay.caret = render::caret_on_page(&state.vector_document,
                    &SourcePosition { node_id: focus.node_id.0.clone(), byte_offset: end }, page, None);
            }
        }
    }
    // Repeated headers share source IDs. Keep the caret on the physical page
    // the user clicked instead of jumping to the first instance of that stream.
    if state.editor.is_editing_header_footer() {
        if let Some(page) = state.caret_page_hint {
            if let Some(focus) = state.editor.focus_position() {
                let source = SourcePosition { node_id: focus.node_id.0, byte_offset: focus.byte_offset };
                if let Some(caret) = render::caret_on_page(&state.vector_document, &source, page, state.caret_visual_line) {
                    state.overlay.caret = Some(caret);
                }
            }
            state.overlay.selections.retain(|rect| rect.page_index == page);
        }
    }
    objects_ui::adorn(state);
    inline_spelling::adorn(state);
    ribbon::comment_marks(state);
    if let Some((revision,generation,marks))=&state.search_marks { if *revision==state.editor.revision && Some(*generation)==state.display_generation {state.overlay.review_marks.extend(marks.iter().cloned());} }
    if equation_ui::previewing(state) { state.overlay.caret = None; }
    state.needs_present = true;
}

/// Reference navigation uses the current semantic hit geometry, including notes.
pub(crate) fn reveal_review_caret(state:&mut CanvasState){
    let Some(focus)=state.editor.focus_position()else{return;};let document=Arc::clone(&state.vector_document);
    let target=document.pages.iter().enumerate().find_map(|(pi,page)|page.hit_regions.iter().find(|r|r.node_id==focus.node_id.0&&(r.byte_range.contains(&focus.byte_offset)||r.caret_stops.iter().any(|p|p.byte_offset==focus.byte_offset))).and_then(|r|state.viewport.page_point_to_view(&document,pi,Point::new(r.rect.x,r.rect.y))));
    if let Some(target)=target{let margin=state.viewport.outer_margin_px.max(24.0);let dy=if target.y<margin{target.y-margin}else if target.y>state.viewport.viewport_px.height-margin{target.y-(state.viewport.viewport_px.height-margin)}else{0.0};state.viewport.scroll_by(0.0,dy,&document);state.needs_present=true;}
}
fn wake_caret(state: &mut CanvasState) {
    state.caret_visible = true;
    state.last_caret_toggle = Instant::now();
}

/// What a PDF conversion could not do (scans left as pictures, …), as a
/// sentence or two to append to the status message; empty when nothing.
pub(crate) fn conversion_notes(document:&Document)->String{
    document.compatibility.iter().filter(|issue|issue.code=="pdf.conversion").map(|issue|format!(" {}",issue.message)).collect()
}

/// Shows a document built by Import PDF on top of the stack. It starts untitled,
/// so Save asks where to keep it.
fn install_imported_document(state:&Rc<RefCell<CanvasState>>,source:&Path,document:Document)->Result<(),String>{
    let notes=conversion_notes(&document);
    let mut s=state.borrow_mut();
    documents::add(&mut s,document,None)?;
    s.action_status=Some(format!("Imported {} — Save to keep it as an editable document.{notes}",source.file_name().map(|n|n.to_string_lossy().to_string()).unwrap_or_default()));
    Ok(())
}

fn choose_open_path(parent: &impl WxWidget) -> Option<PathBuf> {
    let wildcard = document_open_wildcard();
    let dialog = FileDialog::builder(parent)
        .with_message(&crate::i18n::text(&("Open document")))
        .with_wildcard(&crate::i18n::wildcard(&(&wildcard)))
        .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
        .build();
    dialog.set_filter_index(0);
    if ({crate::placement::centre(&dialog);dialog.show_modal()}) != wxdragon::id::ID_OK { return None; }
    dialog.get_path().map(PathBuf::from)
}

/// Open from either the ribbon or Ctrl+O. Pick the requested file first so
/// cancelling the picker never disturbs an unsaved document.
/// Documents dragged onto the window open in it, as in Word: each is added to
/// the window's list (one already open is shown instead).
fn bind_file_drop(ui: &MainUi, state: &Rc<RefCell<CanvasState>>) {
    // GTK delivers a drop to the widget under the pointer, so the page, the
    // panel around it and the frame each accept files.
    // Only the drop is handled: wxDragon passes drag results from Rust to
    // wxWidgets with mismatched numbers (its "Copy" arrives as wxDragNone), so
    // enter/over callbacks would refuse every drag. wxWidgets' own handling
    // accepts files.
    fn accept(target: &impl WxWidget, state: &Rc<RefCell<CanvasState>>) {
        let state = Rc::clone(state);
        wxdragon::dnd::FileDropTarget::builder(target)
            .with_on_drop_files(move |files: Vec<String>, _, _| {
                let Ok(mut state) = state.try_borrow_mut() else { return false; };
                state.dropped_files.extend(files.into_iter().map(PathBuf::from).filter(|p| p.is_file()));
                true
            })
            .build();
    }
    accept(&ui.paper_canvas, state);
    accept(&ui.root_panel, state);
    accept(&ui.main_frame, state);
}

/// Opens the next dropped file once no other file operation is running.
fn open_dropped_files(state: &mut CanvasState) {
    if state.file_job.is_some() || state.clipboard_job.is_some() || state.pending_close { return; }
    let Some(path) = state.dropped_files.pop_front() else { return; };
    if let Some(id) = documents::find_path(state, &path) {
        if let Err(error) = documents::switch_to(state, id) { state.action_status = Some(error); }
        return;
    }
    if let Err(error) = background_io::start_open(state, path) { state.action_status = Some(error); }
}

fn request_open(parent:&impl WxWidget,state:&Rc<RefCell<CanvasState>>)->Result<(),String>{
    if state.borrow().file_job.is_some()||state.borrow().clipboard_job.is_some()||state.borrow().pending_close {
        return Err("Finish the current file operation before opening another document".into());
    }
    equation_ui::close(&mut state.borrow_mut());
    let Some(path)=choose_open_path(parent) else{return Ok(());};
    // A document already open in this window is shown rather than opened twice.
    let open=documents::find_path(&state.borrow(),&path);
    if let Some(id)=open{let mut s=state.borrow_mut();return documents::switch_to(&mut s,id);}
    background_io::queue_open(path,state)
}

fn choose_image_path(parent: &impl WxWidget) -> Option<PathBuf> {
    let dialog = FileDialog::builder(parent)
        .with_message(&crate::i18n::text(&("Insert picture")))
        .with_wildcard(&crate::i18n::wildcard(&("Image files (*.png;*.jpg;*.jpeg;*.gif;*.bmp)|*.png;*.jpg;*.jpeg;*.gif;*.bmp|PNG (*.png)|*.png|JPEG (*.jpg;*.jpeg)|*.jpg;*.jpeg|GIF (*.gif)|*.gif|Bitmap (*.bmp)|*.bmp")))
        .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
        .build();
    if ({crate::placement::centre(&dialog);dialog.show_modal()}) != wxdragon::id::ID_OK { return None; }
    dialog.get_path().map(PathBuf::from)
}

fn image_insert_payload(path: &Path) -> Result<(String, Vec<u8>, f32, f32), String> {
    let data = std::fs::read(path).map_err(|e| format!("Could not read picture: {e}"))?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let media_type = match ext.as_str() {
        "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "gif" => "image/gif", "bmp" => "image/bmp",
        _ => return Err("Unsupported picture format. Use PNG, JPEG, GIF or BMP.".into()),
    }.to_string();
    let (px_w, px_h) = image_pixel_dimensions(&data, &ext).ok_or("Could not determine picture dimensions.")?;
    if px_w == 0 || px_h == 0 { return Err("Picture dimensions are invalid.".into()); }
    // 96 DPI is only an initial physical-size hint. Preserve aspect ratio and
    // cap the inserted width so very large camera images fit the page.
    let mut width_mm = px_w as f32 / 96.0 * 25.4;
    let mut height_mm = px_h as f32 / 96.0 * 25.4;
    if width_mm > 120.0 {
        let scale = 120.0 / width_mm; width_mm *= scale; height_mm *= scale;
    }
    if height_mm > 180.0 {
        let scale = 180.0 / height_mm; width_mm *= scale; height_mm *= scale;
    }
    if width_mm < 8.0 { let scale = 8.0 / width_mm; width_mm *= scale; height_mm *= scale; }
    Ok((media_type, data, width_mm, height_mm))
}

fn image_pixel_dimensions(data: &[u8], ext: &str) -> Option<(u32, u32)> {
    match ext {
        "png" if data.len() >= 24 && &data[..8] == b"\x89PNG\r\n\x1a\n" => {
            Some((u32::from_be_bytes(data[16..20].try_into().ok()?), u32::from_be_bytes(data[20..24].try_into().ok()?)))
        }
        "gif" if data.len() >= 10 && (&data[..6] == b"GIF87a" || &data[..6] == b"GIF89a") => {
            Some((u16::from_le_bytes(data[6..8].try_into().ok()?) as u32, u16::from_le_bytes(data[8..10].try_into().ok()?) as u32))
        }
        "bmp" if data.len() >= 26 && &data[..2] == b"BM" => {
            let w = i32::from_le_bytes(data[18..22].try_into().ok()?).unsigned_abs();
            let h = i32::from_le_bytes(data[22..26].try_into().ok()?).unsigned_abs(); Some((w, h))
        }
        "jpg" | "jpeg" if data.len() >= 4 && data[0] == 0xff && data[1] == 0xd8 => jpeg_dimensions(data),
        _ => None,
    }
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 9 < data.len() {
        if data[i] != 0xff { i += 1; continue; }
        while i < data.len() && data[i] == 0xff { i += 1; }
        if i >= data.len() { break; }
        let marker = data[i]; i += 1;
        if marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker) { continue; }
        if i + 2 > data.len() { break; }
        let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
        if len < 2 || i + len > data.len() { break; }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) && len >= 7 {
            let h = u16::from_be_bytes([data[i + 3], data[i + 4]]) as u32;
            let w = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            return Some((w, h));
        }
        i += len;
    }
    None
}

fn choose_save_path(parent: &impl WxWidget, current: Option<&Path>, previous_embed: bool) -> Result<Option<(PathBuf, bool)>, String> {
    let wildcard = document_save_wildcard();
    let dialog = FileDialog::builder(parent)
        .with_message(&crate::i18n::text(&("Save document")))
        .with_wildcard(&crate::i18n::wildcard(&(&wildcard)))
        .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
        .build();
    if let Some(current) = current {
        if let Some(parent) = current.parent() {
            dialog.set_directory(&parent.to_string_lossy());
        }
        if let Some(name) = current.file_name().and_then(|n| n.to_str()) {
            dialog.set_filename(name);
        }
    } else {
        dialog.set_filename("Untitled.impress");
    }
    dialog.set_filter_index(save_filter_index(current));
    crate::save_options::attach(&dialog, previous_embed)?;
    if ({crate::placement::centre(&dialog);dialog.show_modal()}) != wxdragon::id::ID_OK { return Ok(None); }
    let Some(path) = dialog.get_path() else { return Ok(None); };
    let path = PathBuf::from(path);
    let destination = ensure_extension_for_filter(&path, dialog.get_filter_index());
    // The native dialog may have confirmed the typed name, not our normalized
    // extension. Never overwrite the corrected destination without confirmation.
    if destination != path && destination.exists() {
        let message = format!("The selected file format uses this destination:\n{}\n\nThis file already exists. Replace it?", destination.display());
        let confirm = MessageDialog::builder(parent, &crate::i18n::text(&(&message)), &crate::i18n::text(&("Confirm replacement")))
            .with_style(cautious_question()).build();
        confirm.set_yes_no_labels(&crate::i18n::text(&("Replace")), &crate::i18n::text(&("Cancel")));
        if ({crate::placement::centre(&confirm);confirm.show_modal()}) != wxdragon::id::ID_YES { return Ok(None); }
    }
    let embed = matches!(destination.extension().and_then(|value| value.to_str()), Some("impress" | "docx"))
        && crate::save_options::selected(&dialog);
    Ok(Some((destination, embed)))
}

/// A warning with two choices whose second, safe one is the default, so
/// Enter never discards or overwrites anything (wxNO_DEFAULT, which wxDragon
/// does not name).
pub(crate) fn cautious_question() -> MessageDialogStyle {
    let style = MessageDialogStyle::YesNo | MessageDialogStyle::IconWarning;
    MessageDialogStyle::from_bits_retain(style.bits() | 0x80)
}

fn document_title(state: &CanvasState) -> String {
    let name = documents::active_name(state);
    if state.editor.dirty { format!("{name} *") } else { name }
}

fn zoom_key_factor(code: i32) -> Option<f32> {
    match code {
        43 | 61 | 187 | WXK_ADD | WXK_NUMPAD_ADD => Some(1.10),
        45 | 95 | 189 | WXK_SUBTRACT | WXK_NUMPAD_SUBTRACT => Some(1.0 / 1.10),
        48 => Some(0.0),
        _ => None,
    }
}

#[cfg(test)] #[path = "../tests/unit/main/hidpi_tests.rs"] mod hidpi_tests;


/// Extends a mouse selection to `view_point`. A drag that began in a table
/// cell selects whole cells once it reaches another cell; within its own cell
/// it keeps to that cell's text, on every page the cell spans.
fn drag_selection_to(state: &mut CanvasState, view_point: Point) {
    if objects_ui::drag_cells(state, view_point) { return; }
    let document = Arc::clone(&state.vector_document);
    let hit = state.cell_drag_anchor.as_ref()
        .and_then(|(_, cell)| state.viewport.hit_test_in_cell(&document, view_point, &cell.0))
        .or_else(|| state.viewport.hit_test_nearest(&document, view_point));
    if let Some(hit) = hit {
        state.editor.extend_selection(NodeId::from(hit.node_id.as_str()), hit.byte_offset);
        state.caret_visual_line = Some(hit.visual_line); state.caret_page_hint = Some(hit.page_index);
        wake_caret(state); refresh_overlay(state);
    }
}

/// Scrolls while a selection drag holds the pointer above or below the page
/// view, faster the further out it is. Returns whether the view moved.
fn drag_scroll(state: &mut CanvasState) -> bool {
    let Some(point) = state.drag_pointer.filter(|_| state.dragging_selection) else { return false; };
    let height = state.viewport.viewport_px.height;
    let beyond = if point.y < 0.0 { point.y } else if point.y > height { point.y - height } else { return false; };
    let step = beyond.signum() * (beyond.abs() * 0.15).clamp(2.0, 30.0);
    let document = Arc::clone(&state.vector_document);
    let before = state.viewport.scroll_px;
    state.viewport.scroll_by(0.0, step, &document);
    if state.viewport.scroll_px == before { return false; }
    state.needs_present = true;
    true
}

fn editor_overlay(
    selection: &Option<Selection>,
    document: &VectorDocument,
    caret_visual_line: Option<usize>,
) -> CanvasOverlay {
    let source = selection.as_ref().map(|selection| SourceSelection {
        anchor: SourcePosition {
            node_id: selection.anchor.node_id.0.clone(),
            byte_offset: selection.anchor.byte_offset,
        },
        focus: SourcePosition {
            node_id: selection.focus.node_id.0.clone(),
            byte_offset: selection.focus.byte_offset,
        },
    });
    overlay_for_selection_with_caret_line(document, source.as_ref(), 0.85, caret_visual_line)
}

// Native callbacks never borrow CanvasState: drain on the UI thread outside FFI.
fn drain_native_input(state: &mut CanvasState) {
    for event in state.input.drain() {
        if let native_input::InputEvent::Wheel(input) = &event {
            wheel::apply(state, *input);
            continue; // Scrolling must not reshape text, wake the caret or end composition.
        }
        let before = state.editor.revision;
        let result = match event {
            native_input::InputEvent::Wheel(_) => Ok(()), // handled above
            native_input::InputEvent::Begin => state.editor.begin_composition(),
            native_input::InputEvent::Preedit(text) => state.editor.update_composition(text),
            native_input::InputEvent::Commit(text) => {
                let closes_math = text.ends_with('$');
                state.editor.update_composition(text).and_then(|_| state.editor.commit_composition())
                    .and_then(|_| if closes_math { state.editor.convert_typed_math().map(|_| ()) } else { Ok(()) })
            }
            native_input::InputEvent::Cancel => { state.editor.cancel_composition(); Ok(()) },
            native_input::InputEvent::DoubleClick(x, y) => {
                let point = logical_to_view(wxdragon::geometry::Point::new(x, y), state.pixel_scale);
                objects_ui::double_click(state, point); Ok(())
            },
            native_input::InputEvent::CaptureLost => { state.dragging_selection = false; state.cell_drag_anchor = None; state.band_drag = None; objects_ui::cancel_drag(state); Ok(()) },
        };
        if let Err(error) = result { state.action_status = Some(format!("Input: {error}")); }
        if state.editor.revision != before { state.selected_object = None; submit_layout(state); }
        refresh_overlay(state); wake_caret(state);
    }
}
/// Longest gap between two clicks of a double-click, and how far apart they
/// may be (logical pixels).
const DOUBLE_CLICK_TIME: Duration = Duration::from_millis(450);
const DOUBLE_CLICK_SLOP: i32 = 4;

thread_local! {
    static LAST_CLICK: Cell<Option<(Instant, i32, i32)>> = const { Cell::new(None) };
    static NOTE_TOOLTIP: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Whether a click at `position` completes a double-click.
fn is_double_click(position: wxdragon::geometry::Point) -> bool {
    let now = Instant::now();
    let previous = LAST_CLICK.with(|last| last.replace(Some((now, position.x, position.y))));
    let double = previous.is_some_and(|(at, x, y)| now.duration_since(at) <= DOUBLE_CLICK_TIME
        && (x - position.x).abs() <= DOUBLE_CLICK_SLOP && (y - position.y).abs() <= DOUBLE_CLICK_SLOP);
    if double { LAST_CLICK.with(|last| last.set(None)); }
    double
}

/// Size the viewport in physical pixels for a canvas of `width` x `height`
/// logical pixels, and return the pixel scale it used.
fn fit_viewport(viewport: &mut ViewportState, canvas: &impl WxWidget, width: i32, height: i32) -> f32 {
    let scale = display_scale::window_scale(canvas);
    size_viewport(viewport, width, height, canvas.from_dip_int(96).max(72) as f32, scale);
    scale
}

/// The viewport for a canvas of `width` x `height` logical pixels at
/// `logical_dpi`, measured in physical pixels (`scale` per logical pixel).
fn size_viewport(viewport: &mut ViewportState, width: i32, height: i32, logical_dpi: f32, scale: f32) {
    viewport.set_viewport_px(width.max(1) as f32 * scale, height.max(1) as f32 * scale);
    viewport.dpi = logical_dpi * scale;
    viewport.outer_margin_px = ViewportState::default().outer_margin_px * scale;
}

/// A wx mouse position (logical pixels) in the canvas's physical view pixels.
fn logical_to_view(position: wxdragon::geometry::Point, scale: f32) -> Point {
    Point::new(position.x as f32 * scale, position.y as f32 * scale)
}

fn position_native_input(canvas: &impl WxWidget, shared: &Rc<RefCell<CanvasState>>) {
    let (input, point, height) = {
        let state = shared.borrow();
        let Some(caret) = state.overlay.caret.as_ref() else { return; };
        let Some(point) = state.viewport.page_point_to_view(&state.vector_document, caret.page_index, Point::new(caret.rect.x, caret.rect.y)) else { return; };
        // The native input layer works in logical window pixels.
        let scale = state.pixel_scale;
        (Rc::clone(&state.input), Point::new(point.x / scale, point.y / scale), caret.rect.height * state.viewport.points_to_pixels() / scale)
    };
    native_input::position(canvas.get_handle(), &input, point.x.round() as i32, point.y.round() as i32, height.ceil() as i32);
}
fn recover_from_ui(frame: &impl WxWidget, shared: &Rc<RefCell<CanvasState>>) -> Result<(), String> {
    if shared.borrow().file_job.is_some() || shared.borrow().clipboard_job.is_some() {return Err("A file or clipboard operation is still running; recover after it finishes.".into());}
    equation_ui::close(&mut shared.borrow_mut());
    let Some(document) = ribbon::recovery_dialog(frame, shared)? else { return Ok(()); };
    let mut state = shared.borrow_mut();
    documents::add(&mut state, document, None)?;
    state.editor.mark_recovered();
    state.action_status = Some("Recovered document — use Save As to choose a destination".into());
    Ok(())
}
fn bind_close(ui: &MainUi, shared: Rc<RefCell<CanvasState>>, timer: &'static Timer<Frame>) {
    use wxdragon::dialogs::message_dialog::{MessageDialog,MessageDialogStyle};
    let frame = ui.main_frame.clone();
    ui.main_frame.on_close(move |event| {
        let can_veto = match &event {
            WindowEventData::General(e) => e.can_veto(),
            WindowEventData::Session(e) => e.can_veto(),
            _ => false,
        };
        let mut close = true;
        if !can_veto {
            let state = shared.borrow();
            let _ = state.recovery.emergency_snapshot(&state.editor, state.current_path.as_deref(), &state.formats);
            documents::emergency_snapshots(&state);
        } else if shared.borrow().file_job.is_some() || shared.borrow().output_job.is_some() || shared.borrow().clipboard_job.is_some() {
            shared.borrow_mut().action_status=Some("A file/output/clipboard operation is still running. Finish it or cancel it before closing.".into()); close=false;
        } else {
            // Word asks about each document with unsaved changes in turn.
            loop {
                let dirty = documents::first_dirty(&shared.borrow());
                let Some(id) = dirty else { break; };
                {
                    let mut s = shared.borrow_mut();
                    if let Err(error) = documents::switch_to(&mut s, id) { s.action_status = Some(error); close = false; break; }
                }
                frame.update();
                let name = documents::active_name(&shared.borrow());
                let dialog = MessageDialog::builder(&frame,
                    &crate::i18n::text(&(&format!("“{name}” has unsaved changes. What would you like to do?"))), &crate::i18n::text(&("Close Impress")))
                    .with_style(MessageDialogStyle::YesNo|MessageDialogStyle::Cancel|MessageDialogStyle::IconWarning)
                    .build();
                dialog.set_yes_no_labels(&crate::i18n::text(&("Save and close")), &crate::i18n::text(&("Discard changes")));
                dialog.set_ok_cancel_labels("Save and close", "Keep editing");
                let choice = match {crate::placement::centre(&dialog);dialog.show_modal()} {wxdragon::id::ID_YES=>0,wxdragon::id::ID_NO=>1,_=>2};
                match choice {
                    0 => {
                        // Closing resumes once the save has finished.
                        match background_io::queue_save(&frame,&shared,false){
                            Ok(())=>{let mut s=shared.borrow_mut();s.pending_close=s.file_job.is_some();if s.pending_close{s.action_status=Some("Saving before close… Escape cancels closing; newer edits keep the window open.".into());}},
                            Err(error)=>shared.borrow_mut().action_status=Some(error),
                        }
                        close = false; break;
                    },
                    1 => {
                        let mut s = shared.borrow_mut();
                        s.recovery.mark_clean();
                        let others = s.documents.len() > 1;
                        if others { if let Err(error) = documents::close(&mut s, id) { s.action_status = Some(error); close = false; break; } }
                        else { break; }
                    },
                    _ => { close = false; break; },
                }
            }
        }
        if close {
            // The timer is owned by a leaked Rust wrapper, while wxWidgets owns
            // its event target. Stop it before destroying that target.
            timer.stop();
            // The document already holds the equation being edited.
            equation_ui::close(&mut shared.borrow_mut());
            shared.borrow().scheduler.request_stop();
            event.skip(true);
            if let Some(app) = wxdragon::app::get_app_instance() { app.exit_main_loop(); }
        }
        else { match &event { WindowEventData::General(e) => e.veto(), WindowEventData::Session(e) => e.veto(), _ => {} }; event.skip(false); }
    });
}


/// One press of an editing or navigation key. Returns whether the key was
/// handled and whether the canvas should repaint now (typing deletions wait
/// for their layout instead of copying a stale viewport).
fn apply_key(state: &mut CanvasState, code: i32, shift: bool, cmd: bool) -> (bool, bool) {
    let mut consumed = true;
    let old_revision = state.editor.revision;
    let result = match code {
        WXK_BACK => state.editor.backspace(),
        WXK_DELETE => state.editor.delete_forward(),
        WXK_RETURN | WXK_NUMPAD_ENTER => state.editor.split_paragraph(),
        WXK_TAB if cmd => state.editor.insert_text("\t"),
        WXK_TAB => match state.editor.move_table_cell(!shift) {
            Ok(true) => Ok(()),
            Ok(false) if state.editor.current_list_level().is_some() => {
                state.editor.adjust_list_level(if shift { -1 } else { 1 })
            }
            Ok(false) if shift => { consumed = false; Ok(()) },
            Ok(false) => state.editor.insert_text("\t"),
            Err(error) => Err(error),
        },
        WXK_ESCAPE if state.editor.selection.as_ref().is_some_and(|s|state.editor.note_for_paragraph(&s.focus.node_id).is_some()) => {let result=state.editor.leave_footnote();if result.is_ok(){reveal_review_caret(state);}result},
        WXK_ESCAPE if state.editor.is_editing_header_footer() => state.editor.exit_header_footer(),
        WXK_LEFT => {
            if cmd { state.editor.move_word_left(shift); }
            else { state.editor.move_left(shift); }
            state.preferred_caret_x_pt = None;
            state.caret_visual_line = None;
            Ok(())
        },
        WXK_RIGHT => {
            if cmd { state.editor.move_word_right(shift); }
            else { state.editor.move_right(shift); }
            state.preferred_caret_x_pt = None;
            state.caret_visual_line = None;
            Ok(())
        },
        WXK_UP | WXK_DOWN if cmd => {
            if code == WXK_UP { state.editor.move_paragraph_up(shift); }
            else { state.editor.move_paragraph_down(shift); }
            state.preferred_caret_x_pt = None;
            state.caret_visual_line = None;
            Ok(())
        },
        WXK_UP | WXK_DOWN => {
            let direction = if code == WXK_UP { VisualMoveDirection::Up } else { VisualMoveDirection::Down };
            let focus = state.editor.focus_position();
            let document = Arc::clone(&state.vector_document);
            let target = focus.as_ref().and_then(|position| {
                visual_vertical_target(
                    &document,
                    &SourcePosition { node_id: position.node_id.0.clone(), byte_offset: position.byte_offset },
                    state.caret_visual_line,
                    direction,
                    state.preferred_caret_x_pt,
                )
            });
            if let Some(target) = target {
                state.preferred_caret_x_pt = Some(target.preferred_x_pt);
                state.caret_visual_line = Some(target.visual_line);
                state.editor.move_to_position(
                    DocumentPosition::new(NodeId::from(target.position.node_id.as_str()), target.position.byte_offset),
                    shift,
                );
            } else if direction == VisualMoveDirection::Up {
                state.editor.move_up(shift);
                state.caret_visual_line = None;
            } else {
                state.editor.move_down(shift);
                state.caret_visual_line = None;
            }
            Ok(())
        },
        WXK_HOME => { state.editor.move_home(shift); state.preferred_caret_x_pt = None; state.caret_visual_line = None; Ok(()) },
        WXK_END => { state.editor.move_end(shift); state.preferred_caret_x_pt = None; state.caret_visual_line = None; Ok(()) },
        WXK_PAGEUP => {
            let amount = state.viewport.viewport_px.height * 0.85;
            let document = Arc::clone(&state.vector_document);
            state.viewport.scroll_by(0.0, -amount, &document);
            Ok(())
        }
        WXK_PAGEDOWN => {
            let amount = state.viewport.viewport_px.height * 0.85;
            let document = Arc::clone(&state.vector_document);
            state.viewport.scroll_by(0.0, amount, &document);
            Ok(())
        }
        _ => { consumed = false; Ok(()) },
    };
    if let Err(error) = result {
        state.action_status = Some(format!("Edit failed: {error}"));
    }
    if !consumed { return (false, false); }
    let mut waiting_for_typing_layout = false;
    if state.editor.revision != old_revision {
        state.selected_object = None;
        state.preferred_caret_x_pt = None;
        state.caret_visual_line = None;
        if matches!(code, WXK_BACK | WXK_DELETE | WXK_TAB) {
            submit_typing_layout(state);
            waiting_for_typing_layout = true;
        } else {
            submit_layout(state);
        }
    } else if state.typing_layout_pending && matches!(code,
        WXK_LEFT | WXK_RIGHT | WXK_UP | WXK_DOWN | WXK_HOME | WXK_END) {
        // Navigation should catch up to the current text immediately;
        // otherwise several arrows can appear stuck at the old page's
        // last caret stop while a held key is still being laid out.
        submit_layout(state);
    }
    // The old page has no geometry for the new text. Paint the new
    // scene when its layout arrives instead of copying a full stale
    // viewport bitmap for every repeat event.
    if !waiting_for_typing_layout { refresh_overlay(state); }
    wake_caret(state);
    (true, !waiting_for_typing_layout)
}

/// Keys that repeat while held.
fn repeatable(code: i32) -> bool {
    matches!(code, WXK_BACK | WXK_DELETE | WXK_RETURN | WXK_NUMPAD_ENTER | WXK_LEFT | WXK_RIGHT | WXK_UP | WXK_DOWN | WXK_PAGEUP | WXK_PAGEDOWN)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RepeatKey { Code(i32), Char(char) }

/// Key repeat driven by the UI timer instead of the system's repeat events.
/// System repeats arrive at the user's (often slow) setting and Windows folds
/// them together whenever a frame is late, so held keys crawled. Here a held
/// key repeats every `KEY_REPEAT_INTERVAL` after `KEY_REPEAT_DELAY`, catching
/// up after a slow frame, until the key is released.
struct KeyRepeat { key: RepeatKey, physical: Option<i32>, shift: bool, cmd: bool, start: Instant, done: u32 }
const KEY_REPEAT_DELAY: Duration = Duration::from_millis(320);
const KEY_REPEAT_INTERVAL: Duration = Duration::from_millis(22);
impl KeyRepeat {
    fn new(key: RepeatKey, physical: i32, shift: bool, cmd: bool) -> Self {
        Self { key, physical: Some(physical), shift, cmd, start: Instant::now(), done: 0 }
    }
    /// Repeats due since the last call (at most a few per tick).
    fn due(&mut self) -> u32 {
        let elapsed = self.start.elapsed();
        if elapsed < KEY_REPEAT_DELAY { return 0; }
        let total = 1 + ((elapsed - KEY_REPEAT_DELAY).as_millis() / KEY_REPEAT_INTERVAL.as_millis()) as u32;
        let n = total.saturating_sub(self.done).min(4);
        self.done = total.max(self.done);
        n
    }
}

/// Whether a held key is still physically down. Windows can tell for any key.
/// GTK can only tell for modifier keys and raises a wxWidgets assertion for
/// others (such as letters or arrows), so there the repeat stops on the
/// key-up and focus-loss events alone.
fn key_still_down(code: i32) -> bool {
    !cfg!(windows) || wxdragon::utils::get_key_state(code)
}

/// Runs the repeats of a held key. Returns true when the canvas should repaint.
fn run_key_repeat(state: &mut CanvasState) -> bool {
    let Some(mut repeat) = state.key_repeat.take() else { return false; };
    // Released without a key-up (focus moved, a dialog opened): stop.
    if repeat.physical.is_some_and(|code| !key_still_down(code)) { return false; }
    if equation_ui::previewing(state) || state.input.active() { return false; }
    let n = repeat.due();
    let mut repaint = false;
    if n > 0 {
        match repeat.key {
            RepeatKey::Code(code) if matches!(code, WXK_BACK | WXK_DELETE) => {
                let before = state.editor.revision;
                match state.editor.delete_repeated(code == WXK_BACK, n as usize) {
                    Ok(()) if state.editor.revision != before => {
                        state.selected_object = None;
                        state.preferred_caret_x_pt = None;
                        state.caret_visual_line = None;
                        submit_typing_layout(state);
                        wake_caret(state);
                    }
                    Ok(()) => {}
                    Err(error) => { state.action_status = Some(format!("Deletion failed: {error}")); return false; }
                }
            }
            RepeatKey::Code(code) => for _ in 0..n { repaint |= apply_key(state, code, repeat.shift, repeat.cmd).1; },
            RepeatKey::Char(ch) => {
                state.selected_object = None;
                match state.editor.insert_text(&ch.to_string().repeat(n as usize)) {
                    Ok(()) => { state.preferred_caret_x_pt = None; state.caret_visual_line = None; submit_typing_layout(state); wake_caret(state); }
                    Err(error) => { state.action_status = Some(format!("Typing failed: {error}")); return false; }
                }
            }
        }
    }
    state.key_repeat = Some(repeat);
    repaint
}
