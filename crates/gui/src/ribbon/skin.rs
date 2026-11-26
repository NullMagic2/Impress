//! Ribbon button faces made with wxDragon bitmaps and text drawing.
//! wxButton keeps keyboard, mouse, tooltip, and accessibility behavior.
use super::{art, bitmap_button, face, ButtonSpec};
use crate::display_scale::window_scale;
use std::{cell::{Cell, RefCell}, collections::HashMap, rc::{Rc, Weak}};
use wxdragon::{
    bitmap::Bitmap, bitmap_bundle::BitmapBundle, color::Colour,
    dc::{BackgroundMode, DeviceContext, MemoryDC},
    event::window_events::WindowEvents,
    widgets::button::{Button, ButtonBitmapPosition}, window::WxWidget,
};

thread_local! {
    static BUTTONS: RefCell<HashMap<usize, Weak<ButtonFace>>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RenderKey { width: i32, height: i32, dpi: i32, selected: bool, language: u8, background: [u8; 3], colour: Option<art::ColourIndicator> }

struct ButtonFace {
    button: Button,
    label: Cell<&'static str>,
    icon: &'static str,
    kind: &'static str,
    segment: face::Segment,
    art_width_dip: i32,
    art_height_dip: i32,
    width_dip: i32,
    height_dip: i32,
    selected: Cell<bool>,
    colour: Cell<Option<art::ColourIndicator>>,
    busy: Cell<bool>,
    key: Cell<Option<RenderKey>>,
    bundles: RefCell<Vec<BitmapBundle>>,
}

fn lookup(button: &Button) -> Option<Rc<ButtonFace>> {
    let ptr = button.handle_ptr() as usize;
    BUTTONS.with(|buttons| buttons.borrow().get(&ptr).and_then(Weak::upgrade))
}

thread_local! {
    /// Measured caption sizes by label, font size and DPI scale. Every tab
    /// switch used to measure each visible caption through a new memory DC.
    static LABEL_SIZES: RefCell<HashMap<(&'static str, i32, i32), wxdragon::geometry::Size>> = RefCell::new(HashMap::new());
}

/// Measure with the same physical-pixel font used to paint the button face.
pub(super) fn label_size(spec: &ButtonSpec) -> wxdragon::geometry::Size {
    let points = spec.button.get_font().map_or(0, |font| font.get_point_size());
    let key = (spec.label, points, spec.button.from_dip_int(96));
    if let Some(size) = LABEL_SIZES.with(|sizes| sizes.borrow().get(&key).copied()) { return size; }
    let size = measure_label(spec);
    LABEL_SIZES.with(|sizes| sizes.borrow_mut().insert(key, size));
    size
}

fn measure_label(spec: &ButtonSpec) -> wxdragon::geometry::Size {
    let Some(mut bitmap) = Bitmap::new(1, 1) else { return spec.button.get_text_extent(&crate::i18n::text(&(spec.label))); };
    let mut dc = MemoryDC::new();
    dc.select_object(&mut bitmap);
    if let Some(mut font) = spec.button.get_font() {
        let scale = spec.button.from_dip_int(96) as f32 / 96.0;
        font.set_point_size((font.get_point_size() as f32 * scale).round().max(1.0) as i32);
        dc.set_font(&font);
    }
    let (width, height) = dc.get_text_extent(&crate::i18n::text(&(spec.label)));
    wxdragon::geometry::Size::new(width, height)
}

pub fn install(spec: &ButtonSpec) -> bool {
    if !spec.button.is_valid() { return false; }
    if let Some(face) = lookup(&spec.button) { face.update(); return true; }
    let face = Rc::new(ButtonFace {
        button: spec.button,
        label: Cell::new(spec.label),
        icon: spec.icon,
        kind: spec.kind,
        segment: face::Segment::from_name(spec.segment),
        art_width_dip: spec.art_width_dip,
        art_height_dip: spec.art_height_dip,
        width_dip: spec.width_dip,
        height_dip: spec.height_dip,
        selected: Cell::new(false),
        colour: Cell::new(None),
        busy: Cell::new(false),
        key: Cell::new(None),
        bundles: RefCell::new(Vec::new()),
    });
    let ptr = spec.button.handle_ptr() as usize;
    BUTTONS.with(|buttons| { buttons.borrow_mut().insert(ptr, Rc::downgrade(&face)); });
    // The XRC uses wxBU_NOTEXT for the Ribbon, retaining the native caption
    // for accessibility while the complete face bitmap draws the visible one.
    spec.button.set_accessibility_description(&crate::i18n::text(&(spec.tip)));
    let resize = Rc::clone(&face);
    spec.button.on_size(move |event| { resize.update(); event.skip(true); });
    let destroyed = Rc::clone(&face);
    spec.button.on_destroy(move |event| {
        BUTTONS.with(|buttons| { buttons.borrow_mut().remove(&ptr); });
        destroyed.bundles.borrow_mut().clear();
        event.skip(true);
    });
    face.update();
    true
}

pub(super) fn caption(button: &Button, label: &'static str) -> bool {
    let Some(face) = lookup(button) else { return false };
    if face.label.replace(label) != label {
        face.key.set(None);
        face.update();
    }
    true
}

pub fn selected(button: &Button, value: bool) {
    if let Some(face) = lookup(button) {
        if face.selected.replace(value) != value { face.key.set(None); face.update(); }
    }
}

/// Update all native button faces, including hover, pressed and disabled states.
pub(super) fn colour(button:&Button, value:art::ColourIndicator){
    if let Some(face)=lookup(button){
        if face.colour.replace(Some(value))!=Some(value){face.key.set(None);face.update();}
    }
}

impl ButtonFace {
    /// Device-independent pixels to the face bitmap's physical pixels.
    fn px(&self, dip: i32) -> i32 {
        (self.button.from_dip_int(dip) as f32 * window_scale(&self.button)).round() as i32
    }

    /// The colour around the face: the title bar's (the theme's) for the
    /// title-bar buttons.
    fn background(&self) -> [u8; 3] {
        // The panes' close cross sits on its pane, whatever the pane's colour.
        if self.icon == "x" || self.kind.starts_with("status-") {
            if let Some(parent) = self.button.get_parent() {
                let c = parent.get_background_color();
                return [c.r, c.g, c.b];
            }
        }
        if matches!(self.kind, "quick" | "logo") { super::title_bar() }
        else if matches!(self.kind, "tab" | "dialog" | "dialog-default" | "dialog-menu") { [232, 236, 242] }
        else if self.kind == "mode" { [238, 241, 245] } else { [255, 255, 255] }
    }

    fn update(&self) {
        if !self.button.is_valid() || self.busy.replace(true) { return; }
        let scale = window_scale(&self.button);
        let actual = self.button.get_client_size();
        let width = ((actual.width as f32 * scale).round() as i32).max(self.px(self.width_dip)).clamp(1, 4096);
        let height = ((actual.height as f32 * scale).round() as i32).max(self.px(self.height_dip)).clamp(1, 2048);
        let dpi = self.px(96).max(72);
        let key = RenderKey { width, height, dpi, selected: self.selected.get(), language: crate::i18n::language() as u8, background: self.background(), colour: self.colour.get() };
        if self.key.get() != Some(key) {
            if let Some([normal, hover, pressed, disabled]) = self.images(key) {
                let bundles = [normal, hover, pressed, disabled].map(|image| face_bundle(&image, key.dpi));
                bitmap_button::set_bundle(&self.button, &bundles[0], ButtonBitmapPosition::Left);
                self.button.set_bitmap_bundle_hover(&bundles[1]);
                self.button.set_bitmap_bundle_pressed(&bundles[2]);
                self.button.set_bitmap_bundle_disabled(&bundles[3]);
                self.button.set_bitmap_bundle_focus(&bundles[0]);
                *self.bundles.borrow_mut() = bundles.into();
                self.key.set(Some(key));
            }
        }
        self.busy.set(false);
    }

    fn images(&self, key: RenderKey) -> Option<[Bitmap; 4]> {
        let states = [
            face::State { selected: key.selected, ..Default::default() },
            face::State { selected: key.selected, hot: true, ..Default::default() },
            face::State { selected: key.selected, pressed: true, ..Default::default() },
            face::State { selected: key.selected, disabled: true, ..Default::default() },
        ];
        Some([
            self.image(key, states[0])?, self.image(key, states[1])?,
            self.image(key, states[2])?, self.image(key, states[3])?,
        ])
    }

    fn image(&self, key: RenderKey, state: face::State) -> Option<Bitmap> {
        let width = key.width as u32;
        let height = key.height as u32;
        let scale = key.dpi as f32 / 96.0;
        let background = key.background;
        let palette = if self.kind == "gallery-more" {
            // The More strip is the gallery frame's right edge, tinted on hover.
            let fill = if state.pressed { [214, 227, 250] } else if state.hot { [232, 239, 253] } else { [255, 255, 255] };
            face::Palette { outside: [246, 248, 251], top: fill, bottom: fill, border: [223, 228, 236],
                text: if state.disabled { [170, 178, 190] } else { [68, 81, 102] } }
        } else if matches!(self.kind, "gallery" | "gallery-more") {
            face::Palette { outside: [246, 248, 251], top: [255, 255, 255], bottom: [255, 255, 255],
                border: if state.selected { [58, 104, 194] } else { [223, 228, 236] }, text: [36, 48, 66] }
        } else if self.kind.starts_with("dialog") { face::dialog_palette(state, self.kind == "dialog-default") }
        else if self.kind.starts_with("pane") { face::pane_palette(state, self.kind == "pane-default") }
        else if matches!(self.kind, "quick" | "logo") {
            // On the theme-coloured title bar: shades of the bar itself.
            let ink = super::title_ink();
            let fill = if state.pressed { super::mix(background, ink, 0.24) } else if state.hot { super::mix(background, ink, 0.14) } else { background };
            face::Palette { outside: background, top: fill, bottom: fill, border: fill, text: ink }
        }
        else { face::palette(state, background) };
        let mut pixels = if self.kind == "tab" { face::render_tab(width, height, scale, state) }
            else { face::render(width, height, scale, self.segment, palette) };
        let art_width = self.px(self.art_width_dip).max(0) as u32;
        let art_height = self.px(self.art_height_dip).max(0) as u32;
        let icon = if matches!(self.kind, "tab" | "pane-default") || art_width == 0 || art_height == 0 {
            None
        } else if let Some(colour)=key.colour { art::colour_raster(self.icon,art_width,art_height,state.disabled,colour) }
        else if self.icon == "interface-language" { art::raster(super::interface_language::flag_name(), art_width, art_height, state.disabled) }
        else { art::raster(self.icon, art_width, art_height, state.disabled) };
        // On a dark title bar the quick-access icons become glyphs in its text
        // colour: an icon's darker ink shows, its light parts (the "?" of
        // Help, a page's white) let the bar through.
        let icon = match icon {
            Some(raster) if self.kind == "quick" && self.icon != "interface-language" && super::title_is_dark() => {
                let ink = super::title_ink();
                let fade = if state.disabled { 0.45 } else { 1.0 };
                let glyph: Vec<u8> = raster.chunks_exact(4).flat_map(|p| {
                    let light = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
                    let coverage = ((235.0 - light) / 110.0).clamp(0.0, 1.0);
                    [ink[0], ink[1], ink[2], (p[3] as f32 * coverage * fade).round() as u8]
                }).collect();
                Some(std::rc::Rc::<[u8]>::from(glyph))
            }
            other => other,
        };
        if let Some(icon) = icon.as_deref() {
            let (x, y) = self.icon_position(key, art_width as i32, art_height as i32);
            face::icon(&mut pixels, width, height, icon, art_width, art_height, x, y);
        }
        if matches!(self.kind, "dialog-menu" | "typography-menu" | "status-menu" | "colour-menu") {
            face::dropdown_caret(&mut pixels, width, height, scale, palette.text);
        } else if matches!(self.kind, "file-menu" | "large-menu") {
            face::large_dropdown_caret(&mut pixels, width, height, scale, palette.text);
        } else if self.kind == "gallery-more" {
            face::gallery_more_mark(&mut pixels, width, height, scale, palette.text);
        }
        let mut rgba = vec![0; pixels.len()];
        for (target, source) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(pixels.as_chunks::<4>().0) {
            *target = [source[2], source[1], source[0], source[3]];
        }
        let mut bitmap = Bitmap::from_rgba(&rgba, width, height)?;
        {
            let mut dc = MemoryDC::new();
            dc.select_object(&mut bitmap);
            dc.set_background_mode(BackgroundMode::Transparent);
            if let Some(mut font) = self.button.get_font() {
                // MemoryDC paints physical pixels, while the inherited XRC font
                // is specified in logical points. Match the face bitmap's DPI.
                let points = font.get_point_size();
                if points > 0 {
                    font.set_point_size(((points as f32 * scale).round() as i32).max(1));
                }
                dc.set_font(&font);
            }
            dc.set_text_foreground(Colour::rgb(palette.text[0], palette.text[1], palette.text[2]));
            if !self.label.get().is_empty() {
                let (text_width, text_height) = dc.get_text_extent(&crate::i18n::text(&(self.label.get())));
                let (x, y) = self.text_position(key, text_width, text_height, art_width as i32, art_height as i32);
                dc.draw_text(&crate::i18n::text(&(self.label.get())), x, y);
            }
        }
        // GDI text drawing into an RGBA wxBitmap changes its colour channels
        // but can leave the glyph pixels' alpha at zero. Recreate the bitmap
        // with opaque alpha so the caption survives the native button paint.
        let mut painted = bitmap.get_rgba_data()?;
        for pixel in painted.as_chunks_mut::<4>().0 { pixel[3] = 255; }
        Bitmap::from_rgba(&painted, width, height)
    }

    fn icon_position(&self, key: RenderKey, width: i32, height: i32) -> (i32, i32) {
        let gap = self.px(4);
        if self.kind == "colour-menu" { return (self.px(4), (key.height-height)/2); }
        if self.label.get().is_empty() { return ((key.width-width)/2, (key.height-height)/2); }
        if matches!(self.kind, "large" | "large-menu" | "wide" | "gallery" | "file-main" | "file-menu") {
            ((key.width-width)/2, ((key.height-height-self.px(18)-gap)/2).max(gap))
        } else { (gap, (key.height-height)/2) }
    }

    fn text_position(&self, key: RenderKey, text_width: i32, text_height: i32, icon_width: i32, icon_height: i32) -> (i32, i32) {
        let gap = self.px(4);
        if self.kind == "gallery" { return ((key.width-text_width)/2, key.height-text_height-gap); }
        if matches!(self.kind, "large" | "large-menu" | "wide" | "file-main" | "file-menu") {
            let y = self.icon_position(key, icon_width, icon_height).1 + icon_height + gap;
            return ((key.width-text_width)/2, y.min(key.height-text_height));
        }
        if self.kind == "tab" || self.kind.starts_with("dialog") || self.kind.starts_with("pane") || icon_width == 0 {
            return ((key.width-text_width)/2, (key.height-text_height)/2);
        }
        (self.icon_position(key, icon_width, icon_height).0 + icon_width + gap, (key.height-text_height)/2)
    }
}

/// wxBitmapBundle takes its logical size from the smallest bitmap. A single
/// physical-pixel bitmap on a high-DPI display is scaled a second time by
/// wxWidgets, and its enlarged best size forces neighbouring ribbon groups to
/// overlap. Supply a real 1x face before the display-resolution face.
fn face_bundle(bitmap: &Bitmap, dpi: i32) -> BitmapBundle {
    if dpi <= 96 { return BitmapBundle::from_bitmap(bitmap); }
    let logical_width = ((bitmap.get_width() as f32 * 96.0 / dpi as f32).round() as u32).max(1);
    let logical_height = ((bitmap.get_height() as f32 * 96.0 / dpi as f32).round() as u32).max(1);
    let Some(pixels) = bitmap.get_rgba_data() else { return BitmapBundle::from_bitmap(bitmap); };
    let Some(source) = image::RgbaImage::from_raw(bitmap.get_width() as u32, bitmap.get_height() as u32, pixels) else {
        return BitmapBundle::from_bitmap(bitmap);
    };
    let one_x = if dpi % 96 == 0 && source.width() == logical_width * (dpi / 96) as u32
        && source.height() == logical_height * (dpi / 96) as u32 {
        // Whole-number scale (200 %, 300 %): averaging each block is exact and
        // far cheaper than a Lanczos filter, which ran four times per button.
        box_reduce(&source, (dpi / 96) as u32)
    } else {
        image::imageops::resize(&source, logical_width, logical_height, image::imageops::FilterType::Lanczos3)
    };
    let Some(base) = Bitmap::from_rgba(one_x.as_raw(), logical_width, logical_height) else {
        return BitmapBundle::from_bitmap(bitmap);
    };
    BitmapBundle::from_bitmaps(&[base, bitmap.clone()])
}

/// Shrink `source` by a whole `factor`, averaging each factor x factor block.
fn box_reduce(source: &image::RgbaImage, factor: u32) -> image::RgbaImage {
    let (width, height) = (source.width() / factor, source.height() / factor);
    let area = factor * factor;
    image::RgbaImage::from_fn(width, height, |x, y| {
        let mut sum = [0u32; 4];
        for dy in 0..factor {
            for dx in 0..factor {
                let pixel = source.get_pixel(x * factor + dx, y * factor + dy);
                for channel in 0..4 { sum[channel] += u32::from(pixel[channel]); }
            }
        }
        image::Rgba(sum.map(|total| ((total + area / 2) / area) as u8))
    })
}


/// Repaint existing faces and remeasure labels after a language change.
pub(super) fn language_changed() {
    LABEL_SIZES.with(|sizes|sizes.borrow_mut().clear());
    BUTTONS.with(|buttons| {
        for face in buttons.borrow().values().filter_map(Weak::upgrade) {
            face.key.set(None);
            face.update();
        }
    });
}
