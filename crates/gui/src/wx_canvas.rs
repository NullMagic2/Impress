//! Document canvas presented through wxDragon on every platform.
//!
//! Pages are rasterized with tiny-skia into tiles in page space at the current
//! zoom. A frame only copies the visible tiles into place and paints the
//! overlay on top, so:
//! - scrolling copies pixels and rasterizes only newly exposed tiles;
//! - an edit re-rasterizes only the tiles its changed commands touch;
//! - a caret blink or selection change re-rasterizes nothing.
//!
//! Tiles are rasterized in parallel. A frame spends at most `FRAME_BUDGET` on
//! new tiles and shows the rest as blank paper that fills in on the next ticks,
//! so held-down Page Down never waits for a whole screen. While the view is
//! still, the screens above and below are prepared ahead of time.
//!
//! Only the part of the frame that changed is handed to wxWidgets.
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;

use render::{Affine2D, CanvasOverlay, Color, DrawCommand, Paint as ScenePaint,
    PathVerb, RasterImage, Rect, VectorDocument, VectorPage, VectorPath, ViewportState};
use tiny_skia::{FillRule, FilterQuality, IntSize, Mask, Paint, Path, PathBuilder,
    Pixmap, PixmapPaint, Rect as SkRect, Stroke, Transform};
use wxdragon::{Bitmap, DeviceContext};

/// Tile edge in device pixels.
const TILE: u32 = 256;
/// Extra pixels rasterized around each tile. tiny-skia's coverage in the
/// outermost pixel row of a pixmap differs slightly from the same pixel in
/// a larger pixmap, which would otherwise show up as seams between tiles.
const APRON: u32 = 2;
/// Longest a frame spends rasterizing new tiles before it shows what is ready.
const FRAME_BUDGET: Duration = Duration::from_millis(8);
/// Rasterizing time per idle tick for tiles just outside the view.
const PREFETCH_BUDGET: Duration = Duration::from_millis(4);
/// Keep canvas data close to the viewport. Document geometry and text hit
/// information remain available for page jumps and document-wide commands.
const PAGE_CACHE_WINDOW: usize = 20;
const WORKSPACE: Color = Color { r: 232.0 / 255.0, g: 236.0 / 255.0, b: 243.0 / 255.0, a: 1.0 };
const PAGE_SHADOW: Color = Color { r: 192.0 / 255.0, g: 202.0 / 255.0, b: 215.0 / 255.0, a: 1.0 };

struct Tile { pixmap: Pixmap, used: u64 }

struct Base { document: usize, viewport: ViewportState, rects: Vec<Rect>, pixmap: Pixmap }

/// Update of the native bitmap: all of it, or its previous content moved by
/// `shift` (a scroll) with only the listed view rectangles replaced.
#[derive(Clone, Debug, PartialEq)]
enum Present { Full, Patches { shift: (i32, i32), rects: Vec<Rect> } }

#[derive(Default)]
struct PageTiles {
    tiles: HashMap<(u32, u32), Tile>,
    /// Previous pixels retained until the replacement tile is ready.
    stale: HashMap<(u32, u32), Tile>,
    /// Unclipped page-space ink bounds, review-card commands last. Keep the
    /// full extent for deciding whether a cell-edge mask is needed. `None`
    /// marks an unknown extent; that command is drawn into every tile.
    bounds: Option<Arc<[Option<Rect>]>>,
}

/// A glyph outline converted once; keyed by the address of its shared outline,
/// which the entry keeps alive so the address cannot be reused.
struct GlyphPath { _outline: Arc<VectorPath>, path: Option<Path> }

pub struct WxCanvasRenderer {
    tile: u32,
    document: Option<Arc<VectorDocument>>,
    scale: f32,
    pages: Vec<PageTiles>,
    page_window: Range<usize>,
    glyphs: HashMap<usize, GlyphPath>,
    images: HashMap<u64, Pixmap>,
    /// Composited pages without overlay, and the inputs it was built from.
    base: Option<Base>,
    frame: Option<Pixmap>,
    bitmap: Option<Bitmap>,
    /// How the native bitmap must change to show the latest frame.
    present: Present,
    spare: Option<Bitmap>,
    /// Page-pixel areas re-rasterized since the base was last composited.
    damaged: Vec<(usize, Rect)>,
    overlay_bounds: Option<Rect>,
    /// `frame` is the current base with only the overlay drawn on it, inside
    /// `overlay_bounds`: the next overlay restores just that area.
    frame_on_base: bool,
    /// Linux: the frame as a cairo surface that takes only changed rectangles.
    #[cfg(target_os = "linux")]
    cairo: Option<crate::cairo_frame::CairoFrame>,
    clock: u64,
    /// `None` rasterizes every visible tile before a frame is shown (tests).
    budget: Option<Duration>,
    /// Visible tiles are still missing from the last frame.
    incomplete: bool,
    /// Document and view whose neighbouring screens are fully prefetched.
    prefetched: Option<(usize, ViewportState)>,
    pool: Option<Arc<rayon::ThreadPool>>,
    #[cfg(test)] tiles_rendered: usize,
}

/// Read-only inputs of tile rasterization, shared by the worker threads.
struct Raster<'a> { glyphs: &'a HashMap<usize, GlyphPath>, images: &'a HashMap<u64, Pixmap>, scale: f32, tile: u32 }

impl Default for WxCanvasRenderer {
    fn default() -> Self { Self::with_tile_size(TILE) }
}

impl WxCanvasRenderer {
    fn with_tile_size(tile: u32) -> Self {
        Self {
            tile, document: None, scale: 0.0, pages: Vec::new(), page_window: 0..0, glyphs: HashMap::new(),
            images: HashMap::new(), base: None, frame: None, bitmap: None, present: Present::Full, spare: None,
            damaged: Vec::new(), overlay_bounds: None, frame_on_base: false,
            #[cfg(target_os = "linux")] cairo: None,
            clock: 0,
            budget: if cfg!(test) { None } else { Some(FRAME_BUDGET) }, incomplete: false, prefetched: None,
            pool: std::thread::available_parallelism().ok()
                .and_then(|n| rayon::ThreadPoolBuilder::new().num_threads(n.get().clamp(1, 8))
                    .thread_name(|i| format!("canvas-{i}")).build().ok()).map(Arc::new),
            #[cfg(test)] tiles_rendered: 0,
        }
    }

    /// Paints the canvas. Returns true while visible tiles are still being
    /// rasterized; the caller should paint again on its next tick.
    /// `pixel_scale` is physical pixels per logical window pixel: the frame
    /// is rendered at the viewport's physical size and drawn through a DC
    /// scaled down by the same factor, so it lands 1:1 on the screen's pixels.
    pub fn render(&mut self, dc: &impl DeviceContext, document: &Arc<VectorDocument>,
        viewport: &ViewportState, overlay: &CanvasOverlay, dirty: bool, pixel_scale: f32) -> bool {
        let width = viewport.viewport_px.width.max(1.0).round() as u32;
        let height = viewport.viewport_px.height.max(1.0).round() as u32;
        if width > 8192 || height > 8192 { return false; }
        #[cfg(target_os = "linux")]
        if self.render_cairo(dc, document, viewport, overlay, dirty, pixel_scale, width, height) { return self.incomplete; }
        let sized = self.bitmap.as_ref().is_some_and(|b|
            b.get_width() == width as i32 && b.get_height() == height as i32);
        if (dirty || !sized) && self.paint(document, viewport, overlay, width, height).is_some() {
            self.present_frame(width, height, sized);
        }
        if let Some(bitmap) = &self.bitmap {
            let reduction = 1.0 / f64::from(pixel_scale.max(1.0));
            dc.set_user_scale(reduction, reduction);
            dc.draw_bitmap(bitmap, 0, 0, false);
            dc.set_user_scale(1.0, 1.0);
        }
        self.incomplete
    }

    /// The Linux path of `render`: the frame goes to a persistent cairo
    /// surface, only its changed rectangles copied, and is painted from there.
    /// False when cairo cannot be used here (the bitmap path runs instead).
    #[cfg(target_os = "linux")]
    #[allow(clippy::too_many_arguments)]
    fn render_cairo(&mut self, dc: &impl DeviceContext, document: &Arc<VectorDocument>, viewport: &ViewportState,
        overlay: &CanvasOverlay, dirty: bool, pixel_scale: f32, width: u32, height: u32) -> bool {
        let fresh = self.cairo.as_ref().is_none_or(|c| c.size() != (width, height));
        if fresh {
            let Some(surface) = crate::cairo_frame::CairoFrame::new(width, height) else { return false; };
            self.cairo = Some(surface);
        }
        if (dirty || fresh) && self.paint(document, viewport, overlay, width, height).is_some() {
            if let (Some(frame), Some(cairo)) = (self.frame.as_ref(), self.cairo.as_mut()) {
                match &self.present {
                    // Only what changed; a scroll or a new surface takes everything.
                    Present::Patches { shift: (0, 0), rects } if !fresh => cairo.update(frame, Some(rects)),
                    _ => cairo.update(frame, None),
                }
            }
        }
        let reduction = 1.0 / f64::from(pixel_scale.max(1.0));
        self.cairo.as_ref().is_some_and(|c| c.paint(dc.dc_ptr() as *mut std::ffi::c_void, reduction))
    }

    /// Rasterizes tiles of the screens just above and below the current view
    /// while the canvas is idle, so Page Up/Down finds them ready. Returns
    /// true while there is more to prefetch.
    pub fn prefetch(&mut self, document: &Arc<VectorDocument>, viewport: &ViewportState) -> bool {
        let key = (Arc::as_ptr(document) as usize, *viewport);
        let current = self.document.as_ref().is_some_and(|d| Arc::ptr_eq(d, document))
            && viewport.points_to_pixels() == self.scale && self.base.is_some();
        if !current || self.incomplete || self.prefetched == Some(key) { return false; }
        let rects = viewport.page_rects_px(document);
        let (w, h) = (viewport.viewport_px.width, viewport.viewport_px.height);
        let wanted = self.missing_tiles(&rects, Rect::new(0.0, -h, w, 3.0 * h), Rect::new(0.0, 0.0, w, h));
        let remaining = self.fill_tiles(document, &rects, &wanted, Some(Instant::now() + PREFETCH_BUDGET)).1;
        if remaining == 0 { self.prefetched = Some(key); }
        remaining > 0
    }

    /// Tiles of pages intersecting `area` (view pixels) that are not cached,
    /// nearest to the centre of `focus` first.
    fn missing_tiles(&self, rects: &[Rect], area: Rect, focus: Rect) -> Vec<(usize, u32, u32)> {
        let tile = self.tile;
        let (cx, cy) = (focus.x + focus.width * 0.5, focus.y + focus.height * 0.5);
        let mut wanted = Vec::new();
        for index in self.page_window.clone() {
            let page = rects[index];
            let (page_w, page_h) = page_pixels(page);
            let local = page_rect_local(page, area);
            if local.is_empty() { continue; }
            let (tx0, ty0) = (local.x as u32 / tile, local.y as u32 / tile);
            let (tx1, ty1) = ((local.max_x().ceil() as u32).div_ceil(tile).min(page_w.div_ceil(tile)),
                (local.max_y().ceil() as u32).div_ceil(tile).min(page_h.div_ceil(tile)));
            for ty in ty0..ty1 { for tx in tx0..tx1 {
                if self.pages[index].tiles.contains_key(&(tx, ty)) { continue; }
                let x = page.x + (tx * tile) as f32 + tile as f32 * 0.5 - cx;
                let y = page.y + (ty * tile) as f32 + tile as f32 * 0.5 - cy;
                wanted.push(((x * x + y * y) as u64, index, tx, ty));
            }}
        }
        wanted.sort_unstable();
        wanted.into_iter().map(|(_, index, tx, ty)| (index, tx, ty)).collect()
    }

    /// Rasterizes `wanted` tiles in parallel batches until `deadline` (at
    /// least one batch). Returns the page-pixel rectangles of the new tiles
    /// and how many tiles remain.
    fn fill_tiles(&mut self, document: &VectorDocument, rects: &[Rect], wanted: &[(usize, u32, u32)],
        deadline: Option<Instant>) -> (Vec<(usize, Rect)>, usize) {
        let mut pages: Vec<usize> = wanted.iter().map(|w| w.0).collect();
        pages.sort_unstable();
        pages.dedup();
        for &index in &pages { self.prepare_page(index, &document.pages[index]); }
        let batch = self.pool.as_ref().map_or(1, |p| p.current_num_threads()).max(1);
        let mut fresh = Vec::new();
        let mut done = 0;
        for chunk in wanted.chunks(batch) {
            if done > 0 && deadline.is_some_and(|d| Instant::now() >= d) { break; }
            let raster = Raster { glyphs: &self.glyphs, images: &self.images, scale: self.scale, tile: self.tile };
            let bounds: Vec<_> = chunk.iter().map(|&(index, ..)| self.pages[index].bounds.clone()).collect();
            let job = |(&(index, tx, ty), bounds): (&(usize, u32, u32), &Option<Arc<[Option<Rect>]>>)| {
                let (page_w, page_h) = page_pixels(rects[index]);
                (index, tx, ty, bounds.as_ref().and_then(|b| raster.tile(&document.pages[index], b, tx, ty, page_w, page_h)))
            };
            let results: Vec<_> = match &self.pool {
                Some(pool) => pool.install(|| chunk.par_iter().zip(bounds.par_iter()).map(job).collect()),
                None => chunk.iter().zip(bounds.iter()).map(job).collect(),
            };
            done += chunk.len();
            for (index, tx, ty, pixmap) in results {
                let Some(pixmap) = pixmap else { continue; };
                #[cfg(test)] { self.tiles_rendered += 1; }
                let tile = self.tile as f32;
                fresh.push((index, Rect::new(tx as f32 * tile, ty as f32 * tile, tile, tile)));
                self.pages[index].stale.remove(&(tx, ty));
                self.pages[index].tiles.insert((tx, ty), Tile { pixmap, used: self.clock });
            }
        }
        (fresh, wanted.len() - done)
    }

    /// Everything tile rasterization reads: command bounds, glyph paths and
    /// decoded images of `page`.
    fn prepare_page(&mut self, index: usize, page: &VectorPage) {
        self.page_bounds(index, page);
        for command in page.commands.iter().chain(page.review_cards.iter().flat_map(|c| c.commands.iter())) {
            if let DrawCommand::Image { image, .. } = command.unclipped().0 {
                if !self.images.contains_key(&image.key) {
                    if self.images.len() >= 64 { self.images.clear(); }
                    if let Some(pixmap) = image_pixmap(image) { self.images.insert(image.key, pixmap); }
                }
            }
        }
    }

    /// Hands the changed part of the frame to the native bitmap. Converting
    /// pixels to a native bitmap is the costliest step of a scroll frame, so
    /// a scroll moves the existing bitmap and converts only exposed strips.
    fn present_frame(&mut self, width: u32, height: u32, sized: bool) {
        let Some(frame) = &self.frame else { return; };
        let (shift, rects) = match (&self.present, sized, self.bitmap.is_some()) {
            (Present::Patches { shift, rects }, true, true) => (*shift, rects.clone()),
            _ => { self.bitmap = Bitmap::from_rgba(frame.data(), width, height); return; }
        };
        let full = Rect::new(0.0, 0.0, width as f32, height as f32);
        let patches: Vec<(Bitmap, i32, i32)> = rects.iter().map(|r| r.intersection(full)).filter(|r| !r.is_empty())
            .filter_map(|rect| {
                let (x, y, w, h) = (rect.x as usize, rect.y as usize, rect.width as usize, rect.height as usize);
                let mut pixels = Vec::with_capacity(w * h * 4);
                for row in y..y + h {
                    let start = (row * width as usize + x) * 4;
                    pixels.extend_from_slice(&frame.data()[start..start + w * 4]);
                }
                Some((Bitmap::from_rgba(&pixels, w as u32, h as u32)?, x as i32, y as i32))
            }).collect();
        if shift != (0, 0) {
            let Some(previous) = self.bitmap.take() else { return; };
            let mut next = match self.spare.take() {
                Some(b) if b.get_width() == width as i32 && b.get_height() == height as i32 => b,
                _ => match Bitmap::new(width as i32, height as i32) { Some(b) => b, None => {
                    self.bitmap = Bitmap::from_rgba(frame.data(), width, height); return; } },
            };
            {
                let mut dc = wxdragon::dc::MemoryDC::new();
                dc.select_object(&mut next);
                dc.draw_bitmap(&previous, shift.0, shift.1, false);
                for (patch, x, y) in &patches { dc.draw_bitmap(patch, *x, *y, false); }
            }
            self.bitmap = Some(next);
            self.spare = Some(previous);
        } else if !patches.is_empty() {
            let Some(bitmap) = self.bitmap.as_mut() else { return; };
            let mut dc = wxdragon::dc::MemoryDC::new();
            dc.select_object(bitmap);
            for (patch, x, y) in &patches { dc.draw_bitmap(patch, *x, *y, false); }
        }
    }

    /// Produces the frame for `document` and records in `present` which part
    /// of it changed since the previous frame.
    fn paint(&mut self, document: &Arc<VectorDocument>, viewport: &ViewportState,
        overlay: &CanvasOverlay, width: u32, height: u32) -> Option<&Pixmap> {
        self.clock += 1;
        let scale = viewport.points_to_pixels();
        if scale != self.scale {
            self.scale = scale;
            for page in &mut self.pages[self.page_window.clone()] { page.tiles.clear(); page.stale.clear(); }
            self.base = None;
        }
        let document_changed = self.sync_document(document);
        let rects = viewport.page_rects_px(document);
        let view = Rect::new(0.0, 0.0, width as f32, height as f32);
        self.retain_page_window(document, &rects, view, document_changed);

        // Pages without overlay. Rebuilt when the view moves or tiles change.
        let key = Arc::as_ptr(document) as usize;
        let sized = |b: &Base| b.pixmap.width() == width && b.pixmap.height() == height;
        let same_view = self.base.as_ref().is_some_and(|b| sized(b) && b.viewport == *viewport && b.rects == rects);
        let reusable = same_view && self.base.as_ref().is_some_and(|b| b.document == key);
        // A pure scroll moves every page by the same whole-pixel offset.
        let shift = self.base.as_ref().filter(|b| sized(b) && !same_view && self.damaged.is_empty()
            && b.document == key && b.rects.len() == rects.len() && !rects.is_empty())
            .and_then(|b| {
                let (dx, dy) = (rects[0].x - b.rects[0].x, rects[0].y - b.rects[0].y);
                let uniform = b.rects.iter().zip(&rects).all(|(o, n)|
                    n.x - o.x == dx && n.y - o.y == dy && n.width == o.width && n.height == o.height);
                (uniform && dx.abs() < width as f32 && dy.abs() < height as f32).then_some((dx as i32, dy as i32))
            });
        let mut changed: Option<Vec<Rect>> = match shift {
            Some((dx, dy)) => Some(exposed(view, dx as f32, dy as f32)),
            None if same_view => Some(Vec::new()),
            None => None,
        };
        if !reusable || !self.damaged.is_empty() || self.incomplete {
            if same_view {
                changed = Some(self.damaged.iter().filter_map(|&(index, rect)| rects.get(index).map(|page|
                    pixel_rect(Rect::new(page.x + rect.x, page.y + rect.y, rect.width, rect.height).intersection(view))))
                    .filter(|r| !r.is_empty()).collect());
            }
            let mut pixmap = match self.base.take() {
                Some(base) if sized(&base) => base.pixmap,
                _ => Pixmap::new(width, height)?,
            };
            let fresh = self.compose(&mut pixmap, document, &rects, view);
            // Tiles that replaced blank paper differ on screen as well.
            if let Some(changed) = changed.as_mut() {
                changed.extend(fresh.iter().filter_map(|&(index, rect)| rects.get(index).map(|page|
                    pixel_rect(Rect::new(page.x + rect.x, page.y + rect.y, rect.width, rect.height).intersection(view))))
                    .filter(|r| !r.is_empty()));
            }
            self.evict(&rects, view);
            self.base = Some(Base { document: key, viewport: *viewport, rects, pixmap });
            self.frame_on_base = false;
        }
        self.damaged.clear();

        // Overlay on a copy of the base. When only the overlay changed (the
        // caret blinking, a selection), the frame is restored from the base
        // where the previous overlay was, not copied whole: on a HiDPI screen
        // a whole frame is tens of megabytes.
        let (mut frame, reused) = match self.frame.take() {
            Some(frame) if frame.width() == width && frame.height() == height => (frame, self.frame_on_base),
            _ => (Pixmap::new(width, height)?, false),
        };
        let base = self.base.as_ref()?;
        match (reused, self.overlay_bounds) {
            (true, Some(previous)) => restore_rect(&mut frame, &base.pixmap, previous),
            (true, None) => {}
            (false, _) => frame.data_mut().copy_from_slice(base.pixmap.data()),
        }
        self.frame_on_base = true;
        let rects = base.rects.clone();
        let areas = overlay.selections.iter().map(|a| (a, Color::rgba(0.31, 0.57, 0.96, 0.42)))
            .chain(overlay.adornments.iter().map(|a| (a, Color::rgb8(43, 111, 232))))
            .chain(overlay.review_marks.iter().map(|m| (&m.area, m.color)))
            .chain(overlay.caret.iter().map(|a| (a, Color::rgb8(30, 36, 44))));
        let mut overlay_bounds = None;
        for (area, color) in areas {
            let Some(page) = rects.get(area.page_index) else { continue; };
            let rect = Rect::new(page.x + area.rect.x * scale, page.y + area.rect.y * scale,
                area.rect.width * scale, area.rect.height * scale);
            if !rects_intersect(rect, view) { continue; }
            fill_rect(&mut frame, rect, color);
            overlay_bounds = union_rect(overlay_bounds, Some(pixel_rect(rect.intersection(view))));
        }
        for shape in &overlay.chrome {
            let bounds = shape.bounds();
            if !rects_intersect(bounds, view) { continue; }
            draw_chrome(&mut frame, shape);
            overlay_bounds = union_rect(overlay_bounds, Some(pixel_rect(bounds.intersection(view))));
        }
        self.frame = Some(frame);
        // The previous overlay (moved along with a scroll) must be cleared and
        // the new one painted.
        let previous = shifted(self.overlay_bounds, shift);
        self.overlay_bounds = overlay_bounds;
        self.present = match changed {
            None => Present::Full,
            Some(mut rects) => {
                rects.extend(previous.into_iter().chain(overlay_bounds).map(|r| pixel_rect(r.intersection(view))).filter(|r| !r.is_empty()));
                Present::Patches { shift: shift.unwrap_or((0, 0)), rects }
            }
        };
        self.frame.as_ref()
    }

    /// Paints the workspace, page shadows and visible page tiles. Tiles not
    /// ready within the frame budget keep their previous pixels, when available.
    /// Newly exposed areas start as paper. Returns the tiles
    /// rasterized for this frame.
    fn compose(&mut self, base: &mut Pixmap, document: &VectorDocument, rects: &[Rect], view: Rect) -> Vec<(usize, Rect)> {
        let wanted = self.missing_tiles(rects, view, view);
        let (fresh, remaining) = self.fill_tiles(document, rects, &wanted, self.budget.map(|b| Instant::now() + b));
        self.incomplete = remaining > 0;
        base.fill(sk_color(WORKSPACE));
        let tile = self.tile;
        let shadow_start = self.page_window.start.saturating_sub(1);
        let shadow_end = (self.page_window.end + 1).min(rects.len());
        for index in shadow_start..shadow_end {
            let page = rects[index];
            let shadow = Rect::new(page.x - 4.0, page.y - 2.0, page.width + 8.0, page.height + 10.0);
            if !rects_intersect(shadow, view) { continue; }
            fill_rect(base, shadow.intersection(view), PAGE_SHADOW);
            if !self.page_window.contains(&index) { continue; }
            let (page_w, page_h) = page_pixels(page);
            let visible = page_rect_local(page, view);
            if visible.is_empty() { continue; }
            fill_rect(base, Rect::new(page.x + visible.x, page.y + visible.y, visible.width, visible.height), Color::WHITE);
            let (tx0, ty0) = (visible.x as u32 / tile, visible.y as u32 / tile);
            let (tx1, ty1) = ((visible.max_x().ceil() as u32).div_ceil(tile), (visible.max_y().ceil() as u32).div_ceil(tile));
            for ty in ty0..ty1.min(page_h.div_ceil(tile)) {
                for tx in tx0..tx1.min(page_w.div_ceil(tile)) {
                    let PageTiles { tiles, stale, .. } = &mut self.pages[index];
                    let Some(entry) = tiles.get_mut(&(tx, ty)).or_else(|| stale.get_mut(&(tx, ty))) else { continue; };
                    entry.used = self.clock;
                    blit(base, &entry.pixmap, page.x as i32 + (tx * tile) as i32, page.y as i32 + (ty * tile) as i32);
                }
            }
        }
        fresh
    }

    fn retain_page_window(&mut self, document: &VectorDocument, rects: &[Rect], view: Rect, document_changed: bool) {
        let next = page_cache_window(rects, view);
        if !document_changed && next == self.page_window { return; }

        for index in self.page_window.clone().filter(|index| !next.contains(index)) {
            self.pages[index] = PageTiles::default();
        }
        self.page_window = next;

        if !self.images.is_empty() {
            let used_images: HashSet<_> = document.pages[self.page_window.clone()].iter()
                .flat_map(|page| page.commands.iter().chain(page.review_cards.iter().flat_map(|card| &card.commands)))
                .filter_map(|command| match command.unclipped().0 {
                    DrawCommand::Image { image, .. } => Some(image.key),
                    _ => None,
                })
                .collect();
            self.images.retain(|key, _| used_images.contains(key));
        }
    }

    /// Within the window, keep a screen-sized budget for recently used tiles.
    fn evict(&mut self, rects: &[Rect], view: Rect) {
        let tile = self.tile as f32;
        let visible: usize = rects[self.page_window.clone()].iter().map(|page| page_rect_local(*page, view)).filter(|r| !r.is_empty())
            .map(|r| ((r.width / tile).ceil() as usize + 1) * ((r.height / tile).ceil() as usize + 1)).sum();
        let budget = visible * 3 + 32;
        let pages = &mut self.pages[self.page_window.clone()];
        let total: usize = pages.iter().map(|p| p.tiles.len() + p.stale.len()).sum();
        if total > budget {
            let mut ages: Vec<u64> = pages.iter().flat_map(|p| p.tiles.values().chain(p.stale.values()).map(|t| t.used)).collect();
            ages.sort_unstable_by(|a, b| b.cmp(a));
            let cutoff = ages[budget - 1];
            for page in pages { page.tiles.retain(|_, t| t.used >= cutoff); page.stale.retain(|_, t| t.used >= cutoff); }
        }
        if self.glyphs.len() > 20_000 { self.glyphs.clear(); }
    }

    /// Invalidates only the tiles that a new document revision changes.
    fn sync_document(&mut self, document: &Arc<VectorDocument>) -> bool {
        let Some(old) = self.document.replace(Arc::clone(document)) else {
            self.pages.clear();
            self.pages.resize_with(document.pages.len(), PageTiles::default);
            return true;
        };
        if Arc::ptr_eq(&old, document) { return false; }
        self.pages.truncate(document.pages.len());
        self.pages.resize_with(document.pages.len(), PageTiles::default);
        self.page_window.start = self.page_window.start.min(document.pages.len());
        self.page_window.end = self.page_window.end.min(document.pages.len());
        for index in self.page_window.clone() {
            if self.pages[index].tiles.is_empty() && self.pages[index].stale.is_empty() {
                self.pages[index].bounds = None;
                continue;
            }
            let before = &old.pages[index];
            match self.page_damage(index, before, &document.pages[index]) {
                Some(None) => {}
                Some(Some(damage)) => {
                    self.pages[index].bounds = None;
                    let damage = Rect::new(damage.x * self.scale - 2.0, damage.y * self.scale - 2.0,
                        damage.width * self.scale + 4.0, damage.height * self.scale + 4.0);
                    let tile = self.tile as f32;
                    let mut area = None;
                    let invalid: Vec<_> = self.pages[index].tiles.keys().copied().filter(|&(tx, ty)| {
                        rects_intersect(Rect::new(tx as f32 * tile, ty as f32 * tile, tile, tile), damage)
                    }).collect();
                    for key @ (tx, ty) in invalid {
                        let rect = Rect::new(tx as f32 * tile, ty as f32 * tile, tile, tile);
                        area = union_rect(area, Some(rect));
                        if let Some(previous) = self.pages[index].tiles.remove(&key) {
                            self.pages[index].stale.insert(key, previous);
                        }
                    }
                    if let Some(area) = area { self.damaged.push((index, area)); }
                }
                None => {
                    if before.size == document.pages[index].size {
                        let page = &mut self.pages[index];
                        page.stale.extend(page.tiles.drain());
                        page.bounds = None;
                    } else { self.pages[index] = PageTiles::default(); }
                    self.damaged.push((index, Rect::new(0.0, 0.0, 1.0e9, 1.0e9)));
                }
            }
        }
        true
    }

    /// `Some(None)`: unchanged. `Some(Some(rect))`: only `rect` (page points)
    /// changed. `None`: repaint the whole page.
    fn page_damage(&mut self, index: usize, a: &VectorPage, b: &VectorPage) -> Option<Option<Rect>> {
        if a.size != b.size || a.review_cards != b.review_cards { return None; }
        let (a, b) = (&a.commands, &b.commands);
        let prefix = a.iter().zip(b).take_while(|(x, y)| same_command(x, y)).count();
        if prefix == a.len() && prefix == b.len() { return Some(None); }
        let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev())
            .take_while(|(x, y)| same_command(x, y)).count();
        let old_bounds = self.pages[index].bounds.clone();
        let mut damage = None;
        for (i, command) in a.iter().enumerate().take(a.len() - suffix).skip(prefix) {
            let bounds = match old_bounds.as_ref() { Some(b) => b[i], None => self.command_bounds(command) };
            damage = union_rect(damage, Some(bounds?));
        }
        for command in &b[prefix..b.len() - suffix] {
            damage = union_rect(damage, Some(self.command_bounds(command)?));
        }
        Some(Some(damage.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))))
    }

    /// Command bounds of `page`, computed once per page revision.
    fn page_bounds(&mut self, index: usize, page: &VectorPage) -> Arc<[Option<Rect>]> {
        if let Some(bounds) = &self.pages[index].bounds { return Arc::clone(bounds); }
        let bounds: Arc<[Option<Rect>]> = page.commands.iter()
            .chain(page.review_cards.iter().flat_map(|card| card.commands.iter()))
            .map(|command| self.command_bounds(command)).collect();
        self.pages[index].bounds = Some(Arc::clone(&bounds));
        bounds
    }

    fn command_bounds(&mut self, command: &DrawCommand) -> Option<Rect> {
        let (inner, _) = command.unclipped();
        let bounds = match inner {
            DrawCommand::Clipped { .. } => unreachable!("unclipped command"),
            DrawCommand::FillRect { rect, .. } | DrawCommand::Image { rect, .. } => *rect,
            DrawCommand::TextRun(run) => {
                let mut bounds = None;
                for glyph in &run.glyphs {
                    let Some(path) = self.glyph_path(&glyph.outline) else { continue; };
                    let b = path.bounds();
                    bounds = union_rect(bounds, Some(transformed_rect(
                        Rect::new(b.x(), b.y(), b.width(), b.height()), glyph.transform)));
                }
                bounds.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
            }
            DrawCommand::Path(object) => {
                let b = sk_path(&object.path)?.bounds();
                let pad = object.stroke.as_ref().map_or(0.0, |(_, style)| style.width_pt.max(0.01));
                let rect = transformed_rect(Rect::new(b.x(), b.y(), b.width(), b.height()), object.transform);
                Rect::new(rect.x - pad, rect.y - pad, rect.width + 2.0 * pad, rect.height + 2.0 * pad)
            }
        };
        Some(bounds)
    }

    fn glyph_path(&mut self, outline: &Arc<VectorPath>) -> Option<&Path> {
        self.glyphs.entry(Arc::as_ptr(outline) as usize)
            .or_insert_with(|| GlyphPath { _outline: Arc::clone(outline), path: sk_path(outline) })
            .path.as_ref()
    }

}

impl Raster<'_> {
    /// One tile of `page` with its apron. `bounds` are the page's command bounds.
    fn tile(&self, page: &VectorPage, bounds: &[Option<Rect>], tx: u32, ty: u32,
        page_w: u32, page_h: u32) -> Option<Pixmap> {
        let (x0, y0) = (tx * self.tile, ty * self.tile);
        let (w, h) = (self.tile.min(page_w - x0), self.tile.min(page_h - y0));
        let mut pixmap = Pixmap::new(w + 2 * APRON, h + 2 * APRON)?;
        pixmap.fill(tiny_skia::Color::WHITE);
        let scale = self.scale;
        let (left, top) = (x0 as f32 - APRON as f32, y0 as f32 - APRON as f32);
        let view = Affine2D::scale(scale, scale).then(Affine2D::translation(-left, -top));
        let area = Rect::new(0.0, 0.0, pixmap.width() as f32, pixmap.height() as f32);
        // Commands are culled in page space, with a pixel of antialiasing margin.
        let cull = Rect::new((left - 1.0) / scale, (top - 1.0) / scale,
            (area.width + 2.0) / scale, (area.height + 2.0) / scale);
        let mut masks = HashMap::<[u32; 4], Mask>::new();
        let commands = page.commands.iter().chain(page.review_cards.iter().flat_map(|c| c.commands.iter()));
        for (command, bounds) in commands.zip(bounds.iter()) {
            if bounds.is_some_and(|b| !rects_intersect(b, cull)) { continue; }
            self.draw_command(&mut pixmap, &mut masks, command, *bounds, view, area);
        }
        Some(pixmap)
    }

    fn draw_command(&self, target: &mut Pixmap, masks: &mut HashMap<[u32; 4], Mask>,
        command: &DrawCommand, bounds: Option<Rect>, view: Affine2D, area: Rect) {
        let (command, clip) = command.unclipped();
        let clip = clip.map(|rect| transformed_rect(rect, view).intersection(area));
        if clip.is_some_and(Rect::is_empty) { return; }
        // Most text sits inside its clip; a mask is needed only at the edges.
        let needs_mask = clip.is_some_and(|clip| bounds.map(|b| transformed_rect(b, view))
            .is_none_or(|b| !rect_contains(clip, b)));
        if needs_mask {
            let rect = clip.expect("clip present");
            let key = [rect.x.to_bits(), rect.y.to_bits(), rect.width.to_bits(), rect.height.to_bits()];
            if !masks.contains_key(&key) {
                let Some(mask) = clip_mask(target.width(), target.height(), rect) else { return; };
                masks.insert(key, mask);
            }
        }
        let mask = clip.filter(|_| needs_mask).and_then(|rect|
            masks.get(&[rect.x.to_bits(), rect.y.to_bits(), rect.width.to_bits(), rect.height.to_bits()]));
        match command {
            DrawCommand::Clipped { .. } => unreachable!("unclipped command"),
            DrawCommand::FillRect { rect, paint } => {
                if let Some(rect) = sk_rect(*rect) {
                    target.fill_rect(rect, &solid_paint(scene_color(paint)), transform(view), mask);
                }
            }
            DrawCommand::Path(object) => {
                if let Some(path) = sk_path(&object.path) {
                    let mapped = transform(object.transform.then(view));
                    if let Some(paint) = &object.fill {
                        target.fill_path(&path, &solid_paint(scene_color(paint)), FillRule::Winding, mapped, mask);
                    }
                    if let Some((paint, style)) = &object.stroke {
                        let stroke = Stroke { width: style.width_pt.max(0.01), ..Stroke::default() };
                        target.stroke_path(&path, &solid_paint(scene_color(paint)), &stroke, mapped, mask);
                    }
                }
            }
            DrawCommand::TextRun(run) => {
                let paint = solid_paint(scene_color(&run.fill));
                for glyph in &run.glyphs {
                    let cached = self.glyphs.get(&(Arc::as_ptr(&glyph.outline) as usize));
                    let owned = cached.is_none().then(|| sk_path(&glyph.outline)).flatten();
                    if let Some(path) = cached.and_then(|g| g.path.as_ref()).or(owned.as_ref()) {
                        target.fill_path(path, &paint, FillRule::Winding, transform(glyph.transform.then(view)), mask);
                    }
                }
            }
            DrawCommand::Image { rect, image } => {
                if let Some(pixmap) = self.images.get(&image.key) {
                    let placement = Affine2D::scale(rect.width / image.width as f32, rect.height / image.height as f32)
                        .then(Affine2D::translation(rect.x, rect.y)).then(view);
                    target.draw_pixmap(0, 0, pixmap.as_ref(),
                        &PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() },
                        transform(placement), mask);
                }
            }
        }
    }
}

/// Equality that short-circuits on shared runs: unchanged pages from the
/// incremental layout share their command storage.
/// Copies `rect` (whole view pixels) of `base` into `frame`, both the same size.
fn restore_rect(frame: &mut Pixmap, base: &Pixmap, rect: Rect) {
    let (width, height) = (frame.width() as usize, frame.height() as usize);
    let x0 = (rect.x.floor().max(0.0) as usize).min(width);
    let y0 = (rect.y.floor().max(0.0) as usize).min(height);
    let x1 = (rect.max_x().ceil().max(0.0) as usize).min(width);
    let y1 = (rect.max_y().ceil().max(0.0) as usize).min(height);
    if x0 >= x1 || y0 >= y1 { return; }
    let source = base.data();
    let target = frame.data_mut();
    for row in y0..y1 {
        let (start, end) = ((row * width + x0) * 4, (row * width + x1) * 4);
        target[start..end].copy_from_slice(&source[start..end]);
    }
}

fn same_command(a: &DrawCommand, b: &DrawCommand) -> bool {
    match (a, b) {
        (DrawCommand::TextRun(x), DrawCommand::TextRun(y)) => Arc::ptr_eq(x, y) || x == y,
        (DrawCommand::Clipped { rect: r1, command: c1 }, DrawCommand::Clipped { rect: r2, command: c2 }) =>
            r1 == r2 && (Arc::ptr_eq(c1, c2) || same_command(c1, c2)),
        _ => a == b,
    }
}

/// View strips uncovered when the previous frame moves by `(dx, dy)`.
fn exposed(view: Rect, dx: f32, dy: f32) -> Vec<Rect> {
    let mut strips = Vec::new();
    if dy > 0.0 { strips.push(Rect::new(0.0, 0.0, view.width, dy)); }
    if dy < 0.0 { strips.push(Rect::new(0.0, view.height + dy, view.width, -dy)); }
    if dx > 0.0 { strips.push(Rect::new(0.0, 0.0, dx, view.height)); }
    if dx < 0.0 { strips.push(Rect::new(view.width + dx, 0.0, -dx, view.height)); }
    strips
}

fn shifted(rect: Option<Rect>, shift: Option<(i32, i32)>) -> Option<Rect> {
    let (dx, dy) = shift.unwrap_or((0, 0));
    rect.map(|r| Rect::new(r.x + dx as f32, r.y + dy as f32, r.width, r.height))
}

/// Pixel size of a page raster.
fn page_pixels(page: Rect) -> (u32, u32) {
    (page.width.ceil().max(1.0) as u32, page.height.ceil().max(1.0) as u32)
}

/// Centre the retained pages on the visible range, borrowing unused slots
/// from the other side when the view is near a document boundary.
fn page_cache_window(pages: &[Rect], view: Rect) -> Range<usize> {
    if pages.len() <= PAGE_CACHE_WINDOW { return 0..pages.len(); }
    let first = pages.partition_point(|page| page.y + page.height.ceil().max(1.0) <= view.y)
        .min(pages.len() - 1);
    let end = pages.partition_point(|page| page.y < view.max_y()).max(first + 1);
    // At very low zoom every visible page still has to be paintable.
    let visible = end - first;
    let count = PAGE_CACHE_WINDOW.max(visible);
    let before = (count - visible) / 2;
    let start = first.saturating_sub(before).min(pages.len() - count);
    start..start + count
}

/// The part of `view` covered by `page`, in page pixels.
fn page_rect_local(page: Rect, view: Rect) -> Rect {
    let (w, h) = page_pixels(page);
    let visible = Rect::new(page.x, page.y, w as f32, h as f32).intersection(view);
    Rect::new(visible.x - page.x, visible.y - page.y, visible.width, visible.height)
}

/// Copies the inside of an opaque tile (without its apron) into the frame at
/// an integer offset.
fn blit(target: &mut Pixmap, tile: &Pixmap, x: i32, y: i32) {
    let (tw, th) = (target.width() as i32, target.height() as i32);
    let apron = APRON as i32;
    let stride = tile.width() as i32;
    let (sw, sh) = (stride - 2 * apron, tile.height() as i32 - 2 * apron);
    let (x0, y0) = (x.max(0), y.max(0));
    let (x1, y1) = ((x + sw).min(tw), (y + sh).min(th));
    if x0 >= x1 || y0 >= y1 { return; }
    let bytes = (x1 - x0) as usize * 4;
    let source = tile.data();
    let target_data = target.data_mut();
    for row in y0..y1 {
        let s = (((row - y + apron) * stride + (x0 - x + apron)) * 4) as usize;
        let t = ((row * tw + x0) * 4) as usize;
        target_data[t..t + bytes].copy_from_slice(&source[s..s + bytes]);
    }
}

/// Expands a rectangle to whole pixels plus one pixel of antialiasing.
fn pixel_rect(rect: Rect) -> Rect {
    if rect.is_empty() { return Rect::new(0.0, 0.0, 0.0, 0.0); }
    let x = (rect.x.floor() - 1.0).max(0.0); let y = (rect.y.floor() - 1.0).max(0.0);
    Rect::new(x, y, rect.max_x().ceil() + 1.0 - x, rect.max_y().ceil() + 1.0 - y)
}

fn union_rect(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a.filter(|r| !r.is_empty()), b.filter(|r| !r.is_empty())) {
        (Some(a), Some(b)) => {
            let x = a.x.min(b.x); let y = a.y.min(b.y);
            Some(Rect::new(x, y, a.max_x().max(b.max_x()) - x, a.max_y().max(b.max_y()) - y))
        }
        (a, b) => a.or(b),
    }
}

fn rect_contains(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x && inner.y >= outer.y
        && inner.max_x() <= outer.max_x() && inner.max_y() <= outer.max_y()
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    !a.intersection(b).is_empty()
}

fn scene_color(paint: &ScenePaint) -> Color {
    match paint { ScenePaint::Solid(color) => *color }
}
fn sk_color(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(byte(color.r), byte(color.g), byte(color.b), byte(color.a))
}
fn solid_paint(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(byte(color.r), byte(color.g), byte(color.b), byte(color.a));
    paint
}
/// Anti-aliased overlay controls.
fn draw_chrome(frame: &mut Pixmap, shape: &render::ChromeShape) {
    use render::ChromeShape;
    let paint = |color: Color| { let mut p = solid_paint(color); p.anti_alias = true; p };
    let mut dash = None;
    let (path, fill, stroke) = match shape {
        ChromeShape::RoundRect { rect, radius, fill, stroke } => {
            let (x, y, w, h) = (rect.x, rect.y, rect.width.max(0.0), rect.height.max(0.0));
            let r = radius.min(w * 0.5).min(h * 0.5).max(0.0);
            let c = r * 0.4477;
            let (x1, y1) = (x + w, y + h);
            let mut p = PathBuilder::new();
            p.move_to(x + r, y);
            p.line_to(x1 - r, y); p.cubic_to(x1 - c, y, x1, y + c, x1, y + r);
            p.line_to(x1, y1 - r); p.cubic_to(x1, y1 - c, x1 - c, y1, x1 - r, y1);
            p.line_to(x + r, y1); p.cubic_to(x + c, y1, x, y1 - c, x, y1 - r);
            p.line_to(x, y + r); p.cubic_to(x, y + c, x + c, y, x + r, y);
            p.close();
            (p.finish(), *fill, *stroke)
        }
        ChromeShape::Circle { center, radius, fill, stroke } =>
            (PathBuilder::from_circle(center.x, center.y, radius.max(0.1)), *fill, *stroke),
        ChromeShape::Line { from, to, width, color } => {
            let mut p = PathBuilder::new();
            p.move_to(from.x, from.y); p.line_to(to.x, to.y);
            (p.finish(), None, Some((*color, *width)))
        }
        ChromeShape::DashedLine { from, to, width, color, dash: length } => {
            let mut p = PathBuilder::new();
            p.move_to(from.x, from.y); p.line_to(to.x, to.y);
            dash = tiny_skia::StrokeDash::new(vec![*length, *length], 0.0);
            (p.finish(), None, Some((*color, *width)))
        }
    };
    let Some(path) = path else { return; };
    if let Some(color) = fill { frame.fill_path(&path, &paint(color), FillRule::Winding, Transform::identity(), None); }
    if let Some((color, width)) = stroke {
        let cap = if dash.is_some() { tiny_skia::LineCap::Butt } else { tiny_skia::LineCap::Round };
        let stroke = Stroke { width, line_cap: cap, line_join: tiny_skia::LineJoin::Round, dash, ..Stroke::default() };
        frame.stroke_path(&path, &paint(color), &stroke, Transform::identity(), None);
    }
}

fn fill_rect(frame: &mut Pixmap, rect: Rect, color: Color) {
    if let Some(rect) = sk_rect(rect) {
        if rect.width() < 2.0 || rect.height() < 2.0 {
            // The rectangle hairline fast path asserts for some fractional
            // caret widths. The general path handles these subpixel edges.
            frame.fill_path(&PathBuilder::from_rect(rect), &solid_paint(color),
                FillRule::Winding, Transform::identity(), None);
        } else {
            frame.fill_rect(rect, &solid_paint(color), Transform::identity(), None);
        }
    }
}
fn sk_rect(rect: Rect) -> Option<SkRect> {
    SkRect::from_xywh(rect.x, rect.y, rect.width, rect.height)
}
fn sk_path(source: &VectorPath) -> Option<Path> {
    let mut path = PathBuilder::new();
    for verb in &source.verbs {
        match *verb {
            PathVerb::MoveTo(p) => path.move_to(p.x, p.y),
            PathVerb::LineTo(p) => path.line_to(p.x, p.y),
            PathVerb::QuadTo { ctrl, to } => path.quad_to(ctrl.x, ctrl.y, to.x, to.y),
            PathVerb::CubicTo { ctrl1, ctrl2, to } =>
                path.cubic_to(ctrl1.x, ctrl1.y, ctrl2.x, ctrl2.y, to.x, to.y),
            PathVerb::Close => path.close(),
        }
    }
    path.finish()
}
fn transform(value: Affine2D) -> Transform {
    Transform::from_row(value.sx, value.ky, value.kx, value.sy, value.tx, value.ty)
}
fn transformed_rect(rect: Rect, transform: Affine2D) -> Rect {
    let corners = [(rect.x, rect.y), (rect.max_x(), rect.y), (rect.x, rect.max_y()), (rect.max_x(), rect.max_y())]
        .map(|(x, y)| transform.map_point(render::Point::new(x, y)));
    let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in corners { x0 = x0.min(p.x); y0 = y0.min(p.y); x1 = x1.max(p.x); y1 = y1.max(p.y); }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
fn clip_mask(width: u32, height: u32, clip: Rect) -> Option<Mask> {
    let mut mask = Mask::new(width, height)?;
    mask.fill_path(&PathBuilder::from_rect(sk_rect(clip)?), FillRule::Winding,
        true, Transform::identity());
    Some(mask)
}
fn image_pixmap(image: &RasterImage) -> Option<Pixmap> {
    let level = image.levels.first()?;
    let len = level.width.checked_mul(level.height)?.checked_mul(4)? as usize;
    let mut rgba = image.pixels.get(level.offset..level.offset.checked_add(len)?)?.to_vec();
    // RasterImage stores linear-light premultiplication encoded as sRGB.
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = pixel[3] as f32 / 255.0;
        for channel in &mut pixel[..3] {
            let linear = render::srgb_to_linear(*channel as f32 / 255.0);
            *channel = byte(linear_to_srgb(if alpha > 0.0 { linear / alpha } else { 0.0 }) * alpha);
        }
    }
    Pixmap::from_vec(rgba, IntSize::from_wh(level.width, level.height)?)
}
fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 { 12.92 * value }
    else { 1.055 * value.powf(1.0 / 2.4) - 0.055 }
}
fn byte(value: f32) -> u8 { (value.clamp(0.0, 1.0) * 255.0).round() as u8 }

#[cfg(test)] #[path = "../tests/unit/wx_canvas.rs"] mod tests;
