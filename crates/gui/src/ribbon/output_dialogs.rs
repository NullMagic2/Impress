//! Export PDF, Import PDF and Print: one dialog each, with a live preview
//! beside the options (layout: tools/build_output_dialogs.py).
use super::*;
use super::format_dialogs::Chrome;
use crate::{PdfExportDialogUi, PdfImportDialogUi, PrintDialogUi};
use std::sync::atomic::AtomicBool;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Transform};
use wxdragon::{
    dc::AutoBufferedPaintDC, event::window_events::WindowEvents,
    font::{Font, FontFamily, FontStyle, FontWeight},
    timer::Timer, widgets::panel::Panel, window::BackgroundStyle, DeviceContext,
};

const PREVIEW_BG: (u8, u8, u8) = (238, 242, 247);
const MUTED: (u8, u8, u8) = (107, 119, 138);
const ACCENT: (u8, u8, u8) = (29, 61, 120);
const ERROR: (u8, u8, u8) = (180, 35, 24);

fn colour(c: (u8, u8, u8)) -> Colour { Colour::rgb(c.0, c.1, c.2) }
fn d(window: &impl WxWidget, v: i32) -> i32 { window.from_dip_int(v) }

/// "3 pages · 1, 3–4"
fn describe(pages: &[usize]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < pages.len() {
        let start = pages[i];
        let mut end = start;
        while i + 1 < pages.len() && pages[i + 1] == end + 1 { i += 1; end = pages[i]; }
        parts.push(if end > start { format!("{}–{}", start + 1, end + 1) } else { (start + 1).to_string() });
        i += 1;
    }
    let count = pages.len();
    format!("{count} page{} · {}", if count == 1 { "" } else { "s" }, parts.join(", "))
}

/// Pages chosen by the All / Current / Pages radios.
fn chosen(all: bool, current: Option<usize>, range: &str, total: usize) -> Result<Vec<usize>, String> {
    if all { return Ok((0..total).collect()); }
    if let Some(page) = current { return Ok(vec![page.min(total.saturating_sub(1))]); }
    if range.trim().is_empty() { return Err("Type the pages to use, for example 1, 3-5".into()); }
    output::page_ranges(&range.replace(['–', '—'], "-"), total).map_err(|e| format!("{e}."))
}

// Page preview ---------------------------------------------------------------------------------

/// A painted page preview with ‹ Page n of N › underneath: a miniature of the
/// laid-out document, as it will print.
pub(super) struct PagePreview {
    panel: Panel,
    document: RefCell<Arc<VectorDocument>>,
    pages: RefCell<Vec<usize>>,
    index: Cell<usize>,
    cache: RefCell<Option<((usize, i32, i32), Pixmap)>>,
    loader: RefCell<Option<PreviewLoader>>,
}

struct PreviewLoader {
    source: Arc<Document>,
    scheduler: layout::LayoutScheduler,
    timer: Timer<Panel>,
    generation: u64,
    requested: Option<usize>,
    rendered: Option<Arc<VectorDocument>>,
}

impl PagePreview {
    pub(super) fn install(panel: Panel, document: Arc<VectorDocument>) -> Rc<Self> {
        let preview = Rc::new(Self { panel, document: RefCell::new(document), pages: RefCell::new(Vec::new()), index: Cell::new(0), cache: RefCell::new(None), loader: RefCell::new(None) });
        panel.set_background_style(BackgroundStyle::Paint);
        panel.on_erase_background(|_| {});
        let p = Rc::downgrade(&preview);
        panel.on_paint(move |_| if let Some(p) = p.upgrade() { p.paint(); });
        panel.on_size(move |e| { panel.refresh(false, None); e.skip(true); });
        let p = Rc::downgrade(&preview);
        panel.on_mouse_left_down(move |event| {
            let (Some(p), WindowEventData::MouseButton(m)) = (p.upgrade(), &event) else { return; };
            let Some(pos) = m.event.get_position() else { return; };
            let size = p.panel.get_client_size();
            if pos.y < size.height - d(&p.panel, 30) { return; }
            if pos.x < size.width / 3 { p.step(false); }
            else if pos.x > size.width * 2 / 3 { p.step(true); }
        });
        preview
    }

    pub(super) fn step(&self, forward: bool) {
        let index = self.index.get();
        let max = self.pages.borrow().len().saturating_sub(1);
        let next = if forward { (index + 1).min(max) } else { index.saturating_sub(1) };
        if next != index {
            self.index.set(next);
            self.panel.refresh(false, None);
        }
    }

    fn install_for_output(panel: Panel, scene: Arc<VectorDocument>, source: Arc<Document>) -> Result<Rc<Self>, String> {
        let preview = Self::install(panel, scene.clone());
        preview.set_output(scene, source, 0)?;
        Ok(preview)
    }

    pub(super) fn clear_output(&self) {
        if let Some(loader) = self.loader.borrow_mut().take() {
            loader.timer.stop();
        }
        self.show_document(Arc::new(VectorDocument::default()));
    }

    pub(super) fn set_output(self: &Rc<Self>, scene: Arc<VectorDocument>, source: Arc<Document>, start: usize) -> Result<(), String> {
        self.clear_output();
        if scene.pages.iter().any(|page| !page.has_paint) {
            let scheduler = layout::LayoutScheduler::new(layout::LayoutOptions {
                worker_threads: 2, ..Default::default()
            }).map_err(|error| error.to_string())?;
            let timer = Timer::new(&self.panel);
            let weak = Rc::downgrade(self);
            timer.on_tick(move |_| {
                let Some(preview) = weak.upgrade() else { return; };
                let mut state = preview.loader.borrow_mut();
                let Some(loader) = state.as_mut() else { return; };
                if let Some(result) = loader.scheduler.try_recv() {
                    loader.timer.stop();
                    if let Ok(layout) = result.layout {
                        loader.rendered = Some(layout.document);
                        preview.cache.borrow_mut().take();
                        drop(state);
                        preview.panel.refresh(false, None);
                    }
                }
            });
            *self.loader.borrow_mut() = Some(PreviewLoader {
                source, scheduler, timer, generation: 0, requested: None, rendered: None,
            });
        }
        let start = start.min(scene.pages.len().saturating_sub(1));
        self.show_document(scene);
        self.index.set(start);
        Ok(())
    }

    fn painted_page(&self, current: usize, initial: &Arc<VectorDocument>) -> Option<Arc<VectorDocument>> {
        if initial.pages.get(current)?.has_paint { return Some(Arc::clone(initial)); }
        let mut state = self.loader.borrow_mut();
        let loader = state.as_mut()?;
        if let Some(rendered) = &loader.rendered {
            if rendered.pages.get(current).is_some_and(|page| page.has_paint) {
                return Some(Arc::clone(rendered));
            }
        }
        if loader.requested != Some(current) {
            loader.requested = Some(current);
            loader.generation = loader.generation.wrapping_add(1);
            loader.scheduler.submit_preview(loader.generation, Arc::clone(&loader.source), current..current + 1, false);
            loader.timer.start(30, false);
        }
        None
    }

    /// Shows another laid-out document, from its first page.
    pub(super) fn show_document(&self, document: Arc<VectorDocument>) {
        let count = document.pages.len();
        *self.document.borrow_mut() = document;
        self.cache.borrow_mut().take();
        *self.pages.borrow_mut() = (0..count).collect();
        self.index.set(0);
        self.panel.refresh(false, None);
    }

    pub(super) fn set_pages(&self, pages: Vec<usize>) {
        let keep = self.pages.borrow().get(self.index.get()).copied();
        let index = keep.and_then(|k| pages.iter().position(|&p| p == k)).unwrap_or(0);
        *self.pages.borrow_mut() = pages;
        self.index.set(index);
        self.panel.refresh(false, None);
    }

    fn paint(&self) {
        let dc = AutoBufferedPaintDC::new(&self.panel);
        let size = self.panel.get_client_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        let nav = d(&self.panel, 30);
        let pad = d(&self.panel, 16);
        // Drawn at the screen's resolution, so the miniature is sharp on HiDPI.
        let k = crate::display_scale::window_scale(&self.panel);
        let (pw, ph) = ((w as f32 * k).round() as i32, (h as f32 * k).round() as i32);
        let (pnav, ppad) = ((nav as f32 * k).round() as i32, (pad as f32 * k).round() as i32);
        let Some(mut pixmap) = Pixmap::new(pw as u32, ph as u32) else { return; };
        pixmap.fill(tiny_skia::Color::from_rgba8(PREVIEW_BG.0, PREVIEW_BG.1, PREVIEW_BG.2, 255));
        let pages = self.pages.borrow();
        let current = pages.get(self.index.get()).copied();
        let document = Arc::clone(&self.document.borrow());
        if let Some(index) = current.filter(|&index| index < document.pages.len()) {
            let painted = self.painted_page(index, &document);
            let page = &painted.as_ref().unwrap_or(&document).pages[index];
            let (aw, ah) = ((pw - 2 * ppad) as f32, (ph - 2 * ppad - pnav) as f32);
            let s = (aw / page.size.width).min(ah / page.size.height).max(0.01);
            let (tw, th) = ((page.size.width * s).round().max(1.0) as i32, (page.size.height * s).round().max(1.0) as i32);
            let (x, y) = ((pw - tw) / 2, ppad + (ph - 2 * ppad - pnav - th) / 2);
            let key = (current.unwrap_or(0), tw, th);
            let mut cache = self.cache.borrow_mut();
            if painted.is_some() && cache.as_ref().is_none_or(|(k, _)| *k != key) {
                let matrix = render::Affine2D { sx: s, sy: s, ..render::Affine2D::IDENTITY };
                let thumb = output::raster::BandRenderer::default().render(page, tw as u32, th as u32, matrix, &AtomicBool::new(false)).ok()
                    .and_then(|mut data| { for p in data.chunks_exact_mut(4) { p.swap(0, 2); } Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(tw as u32, th as u32)?) });
                *cache = thumb.map(|t| (key, t));
            }
            // Soft shadow, then the page.
            for (grow, alpha) in [(4.0f32, 18u8), (2.0, 30), (1.0, 45)] {
                let grow = grow * k;
                fill_rect(&mut pixmap, (x as f32 - grow, y as f32 - grow + 2.0 * k, tw as f32 + 2.0 * grow, th as f32 + 2.0 * grow), (40, 60, 90), alpha);
            }
            fill_rect(&mut pixmap, (x as f32, y as f32, tw as f32, th as f32), (255, 255, 255), 255);
            if let Some((_, thumb)) = cache.as_ref().filter(|(cached, _)| *cached == key) {
                pixmap.draw_pixmap(x, y, thumb.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            }
        }
        if let Some(bitmap) = wxdragon::Bitmap::from_rgba(pixmap.data(), pw as u32, ph as u32) {
            let reduction = 1.0 / f64::from(k);
            dc.set_user_scale(reduction, reduction);
            dc.draw_bitmap(&bitmap, 0, 0, false);
            dc.set_user_scale(1.0, 1.0);
        }
        dc.set_background_mode(wxdragon::dc::BackgroundMode::Transparent);
        if let Some(font) = Font::new_with_details(9, FontFamily::Default as i32, FontStyle::Normal as i32, FontWeight::Normal as i32, false, "Segoe UI") { dc.set_font(&font); }
        let label = match current {
            Some(page) => format!("Page {} of {}   ·   document page {}", self.index.get() + 1, pages.len(), page + 1),
            None => "No pages selected".into(),
        };
        dc.set_text_foreground(colour(MUTED));
        let (lw, lh) = dc.get_text_extent(&crate::i18n::text(&(&label)));
        let ny = h - nav + (nav - lh) / 2;
        dc.draw_text(&crate::i18n::text(&(&label)), (w - lw) / 2, ny);
        let can_back = self.index.get() > 0;
        let can_next = self.index.get() + 1 < pages.len();
        if let Some(font) = Font::new_with_details(14, FontFamily::Default as i32, FontStyle::Normal as i32, FontWeight::Normal as i32, false, "Segoe UI") { dc.set_font(&font); }
        for (text, x, on) in [("‹", d(&self.panel, 14), can_back), ("›", w - d(&self.panel, 22), can_next)] {
            dc.set_text_foreground(if on { colour(ACCENT) } else { Colour::rgb(196, 204, 216) });
            let (_, th) = dc.get_text_extent(&crate::i18n::text(&(text)));
            dc.draw_text(&crate::i18n::text(&(text)), x, h - nav + (nav - th) / 2 - 1);
        }
    }
}

fn fill_rect(pixmap: &mut Pixmap, r: (f32, f32, f32, f32), rgb: (u8, u8, u8), alpha: u8) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, alpha);
    paint.anti_alias = true;
    if let Some(rect) = tiny_skia::Rect::from_xywh(r.0, r.1, r.2.max(0.1), r.3.max(0.1)) {
        pixmap.fill_path(&PathBuilder::from_rect(rect), &paint, FillRule::Winding, Transform::identity(), None);
    }
}

fn set_summary(label: &wxdragon::widgets::StaticText, pages: &Result<Vec<usize>, String>) {
    match pages {
        Ok(pages) => { label.set_foreground_color(colour(MUTED)); label.set_label(&crate::i18n::text(&(&describe(pages)))); }
        Err(error) => { label.set_foreground_color(colour(ERROR)); label.set_label(&crate::i18n::text(&(error))); }
    }
}

fn current_page(state: &CanvasState) -> usize {
    state.caret_page_hint.or_else(|| state.viewport.visible_pages(&state.vector_document).first().copied()).unwrap_or(0)
}

// Export PDF -----------------------------------------------------------------------------------

pub(super) fn export_pdf(ribbon: &Ribbon) -> Result<(), String> {
    let (scene, source, revision, current, title, author, subject, default_path) = {
        let mut s = ribbon.state.borrow_mut();
        let (scene, doc) = crate::output_ui::snapshot(&mut s)?;
        let stem = s.current_path.as_ref().and_then(|p| p.file_stem()).map(|x| x.to_string_lossy().to_string());
        let title = doc.metadata.title.clone().or(stem.clone()).unwrap_or_default();
        let dir = s.current_path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)).unwrap_or_default();
        let path = dir.join(format!("{}.pdf", stem.unwrap_or_else(|| "Document".into())));
        (scene, Arc::clone(&doc), s.editor.revision, current_page(&s), title, doc.metadata.author.clone().unwrap_or_default(),
         doc.metadata.subject.clone().unwrap_or_default(), path)
    };
    let total = scene.pages.len();
    let ui = Rc::new(crate::i18n::localized(PdfExportDialogUi::new(Some(&ribbon.ui.main_frame), false)));
    let chrome = Chrome::new(ui.pdf_export_dialog, ui.pdf_export_card, &[], &[], (ui.pdf_export_ok, "Export"), Some(ui.pdf_export_cancel), &[], &[(ui.pdf_export_browse, "Browse…")]);
    let preview = PagePreview::install_for_output(ui.pdf_export_preview, scene, source)?;
    ui.pdf_export_path.set_value(&default_path.to_string_lossy());
    ui.pdf_export_title.set_value(&title);
    ui.pdf_export_author.set_value(&author);
    ui.pdf_export_subject.set_value(&subject);
    ui.pdf_export_links.set_value(true);
    ui.pdf_export_all.set_value(true);
    ui.pdf_export_current.set_label(&crate::i18n::text(&(&format!("Current page ({})", current + 1))));
    let pages = { let ui = Rc::clone(&ui); move || chosen(ui.pdf_export_all.get_value(), ui.pdf_export_current.get_value().then_some(current), &ui.pdf_export_pages.get_value(), total) };
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); let preview = Rc::clone(&preview); let pages = pages.clone(); Rc::new(move || {
        let result = pages();
        set_summary(&ui.pdf_export_summary, &result);
        if let Ok(list) = result { preview.set_pages(list); }
        ui.pdf_export_ok.enable(pages().is_ok() && !ui.pdf_export_path.get_value().trim().is_empty());
    }) };
    for radio in [ui.pdf_export_all, ui.pdf_export_current, ui.pdf_export_range] { let r = Rc::clone(&refresh); radio.on_selected(move |_| r()); }
    { let r = Rc::clone(&refresh); let ui2 = Rc::clone(&ui); ui.pdf_export_pages.on_text_updated(move |_| { if !ui2.pdf_export_range.get_value() && !ui2.pdf_export_pages.get_value().is_empty() { ui2.pdf_export_range.set_value(true); } r(); }); }
    { let r = Rc::clone(&refresh); ui.pdf_export_path.on_text_updated(move |_| r()); }
    { let ui2 = Rc::clone(&ui); ui.pdf_export_browse.on_click(move |_| {
        let current = PathBuf::from(ui2.pdf_export_path.get_value());
        let dialog = FileDialog::builder(&ui2.pdf_export_dialog).with_message(&crate::i18n::text(&("Export PDF to")))
            .with_wildcard(&crate::i18n::wildcard(&("PDF document (*.pdf)|*.pdf"))).with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .with_default_dir(&current.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default())
            .with_default_file(&current.file_name().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()).build();
        if dialog.show_modal() == wxdragon::id::ID_OK { if let Some(path) = dialog.get_path() { ui2.pdf_export_path.set_value(&path); } }
    }); }
    refresh();
    let request = loop {
        if !chrome.show() { ui.pdf_export_dialog.destroy(); return Ok(()); }
        let mut path = PathBuf::from(ui.pdf_export_path.get_value().trim());
        if path.extension().is_none() { path.set_extension("pdf"); }
        let problem = if !path.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("pdf")) { Some("The file name must end in .pdf".to_string()) }
            else if !path.parent().is_none_or(|p| p.as_os_str().is_empty() || p.is_dir()) { Some("That folder does not exist".into()) }
            else { None };
        match (problem, pages()) {
            (Some(problem), _) | (None, Err(problem)) => {
                ui.pdf_export_summary.set_foreground_color(colour(ERROR)); ui.pdf_export_summary.set_label(&crate::i18n::text(&(&problem)));
            }
            (None, Ok(pages)) => break crate::output_ui::PdfRequest {
                path, pages, links: ui.pdf_export_links.get_value(), open: ui.pdf_export_open.get_value(),
                info: output::pdf::PdfInfo { title: ui.pdf_export_title.get_value().trim().into(), author: ui.pdf_export_author.get_value().trim().into(),
                    subject: ui.pdf_export_subject.get_value().trim().into(), keywords: ui.pdf_export_keywords.get_value().trim().into() },
            },
        }
    };
    ui.pdf_export_dialog.destroy();
    crate::output_ui::start_pdf_export(&ribbon.state, revision, request)?;
    ribbon.sync_output_progress();
    Ok(())
}

// Print ----------------------------------------------------------------------------------------

pub(super) fn print(ribbon: &Ribbon) -> Result<(), String> {
    let (scene, source, revision, current) = {
        let mut s = ribbon.state.borrow_mut();
        let (scene, source) = crate::output_ui::snapshot(&mut s)?;
        (scene, source, s.editor.revision, current_page(&s))
    };
    let total = scene.pages.len();
    let ui = Rc::new(crate::i18n::localized(PrintDialogUi::new(Some(&ribbon.ui.main_frame), false)));
    let chrome = Chrome::new(ui.print_dialog, ui.print_card, &[], &[], (ui.print_ok, "Print…"), Some(ui.print_cancel), &[], &[]);
    let preview = PagePreview::install_for_output(ui.print_preview, scene, source)?;
    ui.print_all.set_value(true);
    ui.print_fit.set_value(true);
    ui.print_collate.set_value(true);
    ui.print_current.set_label(&crate::i18n::text(&(&format!("Current page ({})", current + 1))));
    let pages = { let ui = Rc::clone(&ui); move || chosen(ui.print_all.get_value(), ui.print_current.get_value().then_some(current), &ui.print_pages.get_value(), total) };
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); let preview = Rc::clone(&preview); let pages = pages.clone(); Rc::new(move || {
        let result = pages();
        let copies = ui.print_copies.get_value().round().max(1.0) as usize;
        match &result {
            Ok(list) => {
                ui.print_summary.set_foreground_color(colour(MUTED));
                let sheets = list.len() * copies;
                ui.print_summary.set_label(&crate::i18n::text(&(&format!("{}{}", describe(list), if copies > 1 { format!(" · {sheets} pages in total") } else { String::new() }))));
                preview.set_pages(list.clone());
            }
            Err(e) => { ui.print_summary.set_foreground_color(colour(ERROR)); ui.print_summary.set_label(&crate::i18n::text(&(e))); }
        }
        ui.print_collate.enable(copies > 1);
        ui.print_ok.enable(result.is_ok());
    }) };
    for radio in [ui.print_all, ui.print_current, ui.print_range] { let r = Rc::clone(&refresh); radio.on_selected(move |_| r()); }
    { let r = Rc::clone(&refresh); let ui2 = Rc::clone(&ui); ui.print_pages.on_text_updated(move |_| { if !ui2.print_range.get_value() && !ui2.print_pages.get_value().is_empty() { ui2.print_range.set_value(true); } r(); }); }
    { let r = Rc::clone(&refresh); ui.print_copies.on_value_changed(move |_| r()); }
    refresh();
    let accepted = chrome.show();
    let request = accepted.then(|| (pages(), ui.print_copies.get_value().round().max(1.0) as usize, ui.print_collate.get_value(), ui.print_fit.get_value()));
    ui.print_dialog.destroy();
    let Some((pages, copies, collate, fit)) = request else { return Ok(()); };
    let pages = pages?;
    match crate::output_ui::print_pages(&ribbon.ui.main_frame, &ribbon.state, revision, pages.clone(), copies, collate, fit)? {
        true => ribbon.state.borrow_mut().action_status = Some(format!("Sent {} to the printer", describe(&pages))),
        false => ribbon.state.borrow_mut().action_status = Some("Printing was cancelled".into()),
    }
    Ok(())
}

// Import PDF -----------------------------------------------------------------------------------

/// The first pages of `document` laid out for a preview miniature; a long
/// document is cut short so the preview stays quick.
pub(super) fn preview_layout(document: &Document) -> Arc<VectorDocument> {
    const PREVIEW_BLOCKS: usize = 80;
    let mut short = document.clone();
    let mut left = PREVIEW_BLOCKS;
    for section in &mut short.sections {
        if left == 0 { section.blocks.clear(); continue; }
        section.blocks.truncate(left);
        left -= section.blocks.len();
    }
    short.sections.retain(|s| !s.blocks.is_empty());
    if short.sections.is_empty() { return Arc::new(VectorDocument::default()); }
    match layout::LayoutEngine::default().layout_document(&short) {
        Ok(laid_out) => Arc::new(laid_out),
        Err(_) => Arc::new(VectorDocument::default()),
    }
}



fn read_pdf(path: &Path) -> Result<format_pdf::PdfSource, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let _busy = wxdragon::cursor::BusyCursor::new(None);
    // Scans are read by OCR only on Import: it can take seconds a page.
    format_pdf::analyze_without_ocr(&bytes)
}

fn choose_pdf(parent: &impl WxWidget) -> Option<PathBuf> {
    let dialog = FileDialog::builder(parent).with_message(&crate::i18n::text(&("Import PDF"))).with_wildcard(&crate::i18n::wildcard(&("PDF document (*.pdf)|*.pdf")))
        .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist).build();
    if dialog.show_modal() != wxdragon::id::ID_OK { return None; }
    dialog.get_path().map(PathBuf::from)
}

/// Import PDF: choose a file (unless given), set options with a live preview,
/// then open the result as a new, untitled document.
pub fn import_pdf(parent: &impl WxWidget, state: &Rc<RefCell<CanvasState>>, path: Option<PathBuf>) -> Result<(), String> {
    {
        let s = state.borrow();
        if s.file_job.is_some() || s.clipboard_job.is_some() || s.pending_close { return Err("Finish the current file operation first".into()); }
    }
    let Some(mut path) = path.or_else(|| choose_pdf(parent)) else { return Ok(()); };
    let mut source = read_pdf(&path)?;
    let ui = Rc::new(crate::i18n::localized(PdfImportDialogUi::new(Some(parent), false)));
    let chrome = Chrome::new(ui.pdf_import_dialog, ui.pdf_import_card, &[], &[], (ui.pdf_import_ok, "Import"), Some(ui.pdf_import_cancel), &[], &[(ui.pdf_import_browse, "Choose…")]);
    let preview = PagePreview::install(ui.pdf_import_preview, Arc::new(VectorDocument::default()));
    let source_cell = Rc::new(RefCell::new(source.clone()));
    let path_cell = Rc::new(RefCell::new(path.clone()));
    ui.pdf_import_all.set_value(true);
    ui.pdf_import_reflow.set_value(true);
    for check in [ui.pdf_import_headings, ui.pdf_import_running, ui.pdf_import_hyphens, ui.pdf_import_columns] { check.set_value(true); }
    let describe_file = { let ui = Rc::clone(&ui); move |path: &Path, source: &format_pdf::PdfSource| {
        ui.pdf_import_file.set_label(&crate::i18n::text(&(&path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())));
        let mut info = format!("{} page{}", source.pages.len(), if source.pages.len() == 1 { "" } else { "s" });
        if let Some(title) = &source.title { info.push_str(&format!(" · “{title}”")); }
        if let Some(author) = &source.author { info.push_str(&format!(" · {author}")); }
        ui.pdf_import_info.set_label(&crate::i18n::text(&(&info)));
    } };
    describe_file(&path, &source);
    let options = { let ui = Rc::clone(&ui); let source_cell = Rc::clone(&source_cell); move || -> Result<format_pdf::ImportOptions, String> {
        let total = source_cell.borrow().pages.len().max(1);
        let pages = if ui.pdf_import_all.get_value() { None } else { Some(chosen(false, None, &ui.pdf_import_pages.get_value(), total)?) };
        Ok(format_pdf::ImportOptions {
            pages, reflow: ui.pdf_import_reflow.get_value(), headings: ui.pdf_import_headings.get_value(),
            strip_running: ui.pdf_import_running.get_value(), join_hyphens: ui.pdf_import_hyphens.get_value(),
            page_breaks: ui.pdf_import_breaks.get_value(), keep_columns: ui.pdf_import_columns.get_value(),
        })
    } };
    let refresh: Rc<dyn Fn()> = { let ui = Rc::clone(&ui); let preview = Rc::clone(&preview); let source_cell = Rc::clone(&source_cell); let options = options.clone(); Rc::new(move || {
        let source = source_cell.borrow();
        match options() {
            Ok(opts) => {
                let pieces = format_pdf::pieces(&source, &opts);
                let pages = opts.pages.clone().unwrap_or_else(|| (0..source.pages.len()).collect());
                let headings = pieces.iter().filter(|p| p.kind != format_pdf::Kind::Body).count();
                ui.pdf_import_summary.set_foreground_color(colour(MUTED));
                ui.pdf_import_summary.set_label(&crate::i18n::text(&(&if pages.is_empty() { "No pages".into() } else {
                    format!("{} → {} paragraph{}, {} heading{}", describe(&pages), pieces.len(), if pieces.len() == 1 { "" } else { "s" }, headings, if headings == 1 { "" } else { "s" })
                })));
                // The converted document itself, laid out: its first pages.
                let mut first = opts.clone();
                first.pages = Some(pages.iter().copied().take(3).collect());
                let document = format_pdf::to_document(&source, &first, crate::document_defaults::new_document());
                preview.show_document(preview_layout(&document));
                ui.pdf_import_ok.enable(true);
            }
            Err(e) => { ui.pdf_import_summary.set_foreground_color(colour(ERROR)); ui.pdf_import_summary.set_label(&crate::i18n::text(&(&e))); ui.pdf_import_ok.enable(false); }
        }
    }) };
    for radio in [ui.pdf_import_all, ui.pdf_import_range, ui.pdf_import_reflow, ui.pdf_import_keep] { let r = Rc::clone(&refresh); radio.on_selected(move |_| r()); }
    for check in [ui.pdf_import_headings, ui.pdf_import_running, ui.pdf_import_hyphens, ui.pdf_import_breaks, ui.pdf_import_columns] { let r = Rc::clone(&refresh); check.on_toggled(move |_| r()); }
    { let r = Rc::clone(&refresh); let ui2 = Rc::clone(&ui); ui.pdf_import_pages.on_text_updated(move |_| { if !ui2.pdf_import_range.get_value() && !ui2.pdf_import_pages.get_value().is_empty() { ui2.pdf_import_range.set_value(true); } r(); }); }
    { let r = Rc::clone(&refresh); let ui2 = Rc::clone(&ui); let source_cell = Rc::clone(&source_cell); let path_cell = Rc::clone(&path_cell); let describe_file = describe_file.clone();
      ui.pdf_import_browse.on_click(move |_| {
        let Some(next) = choose_pdf(&ui2.pdf_import_dialog) else { return; };
        match read_pdf(&next) {
            Ok(source) => { describe_file(&next, &source); *source_cell.borrow_mut() = source; *path_cell.borrow_mut() = next; ui2.pdf_import_dialog.layout(); r(); }
            Err(e) => { ui2.pdf_import_summary.set_foreground_color(colour(ERROR)); ui2.pdf_import_summary.set_label(&crate::i18n::text(&(&e))); }
        }
    }); }
    refresh();
    let accepted = chrome.show();
    let opts = accepted.then(&options);
    ui.pdf_import_dialog.destroy();
    let Some(opts) = opts else { return Ok(()); };
    let opts = opts?;
    source = source_cell.borrow().clone();
    path = path_cell.borrow().clone();
    if source.pages.iter().any(|p| p.lines.is_empty() && !p.pictures.is_empty()) {
        let bytes = std::fs::read(&path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        let _busy = wxdragon::cursor::BusyCursor::new(None);
        format_pdf::read_scans(&bytes, &mut source);
    }
    let document = format_pdf::to_document(&source, &opts, crate::document_defaults::new_document());
    crate::install_imported_document(state, &path, document)
}
