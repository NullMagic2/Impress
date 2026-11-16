//! The few GTK calls wxWidgets does not offer, on Linux only: Impress's own
//! title bar (client-side decorations), true screen positions, every mouse
//! press (to close dropdowns) and the theme's title-bar colours.
//!
//! Once GTK draws the title bar, wxWidgets misjudges where its windows are
//! on screen by the frame's shadow and title-bar offsets (worse after
//! maximizing), so dropdowns opened at `client_to_screen` land in the wrong
//! place. `to_screen` asks GDK instead.
use wxdragon::{geometry::Point, window::WxWidget};

/// Override desktop theme sizing while retaining its button colours and states.
#[cfg(target_os = "linux")]
const USER_STYLE_PRIORITY: u32 = 800;

#[cfg(target_os = "linux")]
mod ffi {
    use std::ffi::{c_char, c_void};
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct Rgba { pub red: f64, pub green: f64, pub blue: f64, pub alpha: f64 }
    #[repr(C)]
    pub struct Allocation { pub x: i32, pub y: i32, pub width: i32, pub height: i32 }
    #[link(name = "gio-2.0")]
    unsafe extern "C" {
        pub fn g_settings_schema_source_get_default() -> *mut c_void;
        pub fn g_settings_schema_source_lookup(source: *mut c_void, schema_id: *const c_char, recursive: i32) -> *mut c_void;
        pub fn g_settings_schema_has_key(schema: *mut c_void, name: *const c_char) -> i32;
        pub fn g_settings_schema_unref(schema: *mut c_void);
        pub fn g_settings_new(schema_id: *const c_char) -> *mut c_void;
        pub fn g_settings_get_string(settings: *mut c_void, key: *const c_char) -> *mut c_char;
        pub fn g_resource_new_from_data(data: *mut c_void, error: *mut *mut c_void) -> *mut c_void;
        pub fn g_resource_enumerate_children(resource: *mut c_void, path: *const c_char, flags: u32, error: *mut *mut c_void) -> *mut *mut c_char;
        pub fn g_resource_lookup_data(resource: *mut c_void, path: *const c_char, flags: u32, error: *mut *mut c_void) -> *mut c_void;
        pub fn g_resource_unref(resource: *mut c_void);
    }
    #[link(name = "glib-2.0")]
    unsafe extern "C" {
        pub fn g_free(memory: *mut c_void);
        pub fn g_strfreev(strings: *mut *mut c_char);
        pub fn g_bytes_new(data: *const c_void, size: usize) -> *mut c_void;
        pub fn g_bytes_get_data(bytes: *mut c_void, size: *mut usize) -> *const c_void;
        pub fn g_bytes_unref(bytes: *mut c_void);
        pub fn g_error_free(error: *mut c_void);
    }
    #[link(name = "pango-1.0")]
    unsafe extern "C" {
        pub fn pango_tab_array_new_with_positions(size: i32, positions_in_pixels: i32, first_alignment: i32, first_position: i32, ...) -> *mut c_void;
        pub fn pango_tab_array_free(tabs: *mut c_void);
    }
    #[link(name = "gobject-2.0")]
    unsafe extern "C" {
        pub fn g_type_check_instance_is_a(instance: *mut c_void, iface_type: usize) -> i32;
        pub fn g_object_unref(object: *mut c_void);
    }
    #[link(name = "cairo")]
    unsafe extern "C" {
        pub fn cairo_image_surface_create(format: i32, width: i32, height: i32) -> *mut c_void;
        pub fn cairo_create(surface: *mut c_void) -> *mut c_void;
        pub fn cairo_set_source_rgb(cr: *mut c_void, red: f64, green: f64, blue: f64);
        pub fn cairo_paint(cr: *mut c_void);
        pub fn cairo_surface_flush(surface: *mut c_void);
        pub fn cairo_image_surface_get_data(surface: *mut c_void) -> *mut u8;
        pub fn cairo_destroy(cr: *mut c_void);
        pub fn cairo_surface_destroy(surface: *mut c_void);
    }
    #[link(name = "gtk-3")]
    unsafe extern "C" {
        pub fn gtk_window_set_titlebar(window: *mut c_void, titlebar: *mut c_void);
        pub fn gtk_widget_show(widget: *mut c_void);
        pub fn gtk_widget_get_type() -> usize;
        pub fn gtk_style_context_add_provider(context:*mut c_void,provider:*mut c_void,priority:u32);
        pub fn gtk_style_context_add_class(context: *mut c_void, class: *const c_char);
        pub fn gtk_css_provider_new() -> *mut c_void;
        pub fn gtk_css_provider_load_from_data(provider: *mut c_void, data: *const c_char, length: isize, error: *mut *mut c_void) -> i32;
        pub fn gtk_style_context_add_provider_for_screen(screen: *mut c_void, provider: *mut c_void, priority: u32);
        pub fn gdk_screen_get_default() -> *mut c_void;
        pub fn gtk_box_new(orientation: i32, spacing: i32) -> *mut c_void;
        pub fn gtk_widget_get_realized(widget: *mut c_void) -> i32;
        pub fn gtk_widget_get_scale_factor(widget: *mut c_void) -> i32;
        pub fn gtk_bin_get_child(bin: *mut c_void) -> *mut c_void;
        pub fn gtk_text_view_set_tabs(view: *mut c_void, tabs: *mut c_void);
        pub fn gtk_text_view_get_type() -> usize;
        pub fn gtk_window_begin_move_drag(window: *mut c_void, button: i32, root_x: i32, root_y: i32, timestamp: u32);
        pub fn gtk_widget_get_window(widget: *mut c_void) -> *mut c_void;
        pub fn gtk_widget_get_has_window(widget: *mut c_void) -> i32;
        pub fn gtk_widget_get_allocation(widget: *mut c_void, allocation: *mut Allocation);
        pub fn gdk_window_get_origin(window: *mut c_void, x: *mut i32, y: *mut i32) -> i32;
        pub fn gtk_widget_get_style_context(widget: *mut c_void) -> *mut c_void;
        pub fn gtk_header_bar_new() -> *mut c_void;
        pub fn g_object_ref_sink(object: *mut c_void) -> *mut c_void;
        pub fn gtk_style_context_lookup_color(context: *mut c_void, name: *const c_char, color: *mut Rgba) -> i32;
        pub fn gtk_style_context_get_color(context: *mut c_void, state: u32, color: *mut Rgba);
        pub fn gtk_render_background(context: *mut c_void, cr: *mut c_void, x: f64, y: f64, width: f64, height: f64);
        pub fn gdk_event_handler_set(func: extern "C" fn(*mut c_void, *mut c_void), data: *mut c_void, notify: *mut c_void);
        pub fn gtk_main_do_event(event: *mut c_void);
    }
}

/// Replaces the desktop's title bar with an empty one, so the window's own
/// top strip serves as its title bar; GTK keeps the shadow and resize edges.
/// The empty bar is shown but styled to take no room: a hidden one made GTK
/// on Wayland draw its default title bar anyway, with a see-through gap
/// where it had reserved room. Returns false when the window is already on
/// screen (GTK cannot change it).
#[cfg(target_os = "linux")]
pub fn hide_system_title_bar(window: &impl WxWidget) -> bool {
    let handle = window.get_handle();
    if handle.is_null() { return false; }
    unsafe {
        if ffi::gtk_widget_get_realized(handle) != 0 { return false; }
        let screen = ffi::gdk_screen_get_default();
        if !screen.is_null() {
            let provider = ffi::gtk_css_provider_new();
            TITLE_BAR_STYLE.store(provider, std::sync::atomic::Ordering::Relaxed);
            set_title_bar_colour([0xfa, 0xfa, 0xfa]);
            // GTK_STYLE_PROVIDER_PRIORITY_USER: above the theme's title-bar style.
            ffi::gtk_style_context_add_provider_for_screen(screen, provider, USER_STYLE_PRIORITY);
        }
        let empty = ffi::gtk_box_new(0, 0);
        ffi::gtk_style_context_add_class(ffi::gtk_widget_get_style_context(empty), c"impress-titlebar".as_ptr());
        ffi::gtk_widget_show(empty);
        ffi::gtk_window_set_titlebar(handle, empty);
    }
    true
}

/// The style of the empty title bar, restyled when the theme changes.
#[cfg(target_os = "linux")]
static TITLE_BAR_STYLE: std::sync::atomic::AtomicPtr<std::ffi::c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Paints the pixel of height GTK keeps for the empty title bar in the top
/// strip's colour, so no line shows above it.
pub fn set_title_bar_colour(colour: [u8; 3]) {
    #[cfg(target_os = "linux")]
    unsafe {
        let provider = TITLE_BAR_STYLE.load(std::sync::atomic::Ordering::Relaxed);
        if provider.is_null() { return; }
        // Without transitions: the theme fades a button's hover state over
        // 200 ms, and moving or resizing the window under the pointer made
        // the whole Ribbon repaint every frame for that long.
        let css = format!(".impress-titlebar, .titlebar.impress-titlebar, headerbar.impress-titlebar {{ min-height: 0; padding: 0; margin: 0; border: none; box-shadow: none; background: rgb({}, {}, {}); }} button {{ transition: none; }}",
            colour[0], colour[1], colour[2]);
        let Ok(css) = std::ffi::CString::new(css) else { return; };
        ffi::gtk_css_provider_load_from_data(provider, css.as_ptr(), -1, std::ptr::null_mut());
    }
    #[cfg(not(target_os = "linux"))]
    let _ = colour;
}

/// Starts moving the window with the mouse, as a press on a title bar does.
/// Physical pixels per logical pixel of `window`'s monitor, straight from
/// GTK. Unlike a device context's content scale it costs no style lookup:
/// creating a wxClientDC on a control queries the theme's colours, and the
/// Ribbon's hundreds of buttons asking on every resize made maximizing stall.
#[cfg(target_os = "linux")]
pub fn scale_factor(window: &impl WxWidget) -> Option<i32> {
    let handle = window.get_handle();
    if handle.is_null() { return None; }
    Some(unsafe { ffi::gtk_widget_get_scale_factor(handle) })
}
#[cfg(not(target_os = "linux"))]
pub fn scale_factor(_window: &impl WxWidget) -> Option<i32> { None }

/// Sets a multi-line text control's tab stops to every `width` pixels. GTK's
/// default is 8 spaces, too wide for the tab-indented Typst source.
#[cfg(target_os = "linux")]
pub fn set_tab_width(control: &impl WxWidget, width: i32) {
    let handle = control.get_handle();
    if handle.is_null() || width <= 0 { return; }
    unsafe {
        // A multi-line wxTextCtrl is a GtkTextView inside a GtkScrolledWindow.
        let view_type = ffi::gtk_text_view_get_type();
        let view = if ffi::g_type_check_instance_is_a(handle, view_type) != 0 { handle } else { ffi::gtk_bin_get_child(handle) };
        if view.is_null() || ffi::g_type_check_instance_is_a(view, view_type) == 0 { return; }
        // One left-aligned stop, in pixels; Pango repeats it across the line.
        let tabs = ffi::pango_tab_array_new_with_positions(1, 1, 0, width);
        ffi::gtk_text_view_set_tabs(view, tabs);
        ffi::pango_tab_array_free(tabs);
    }
}
#[cfg(not(target_os = "linux"))]
pub fn set_tab_width(_control: &impl WxWidget, _width: i32) {}

#[cfg(target_os = "linux")]
pub fn begin_move(window: &impl WxWidget) {
    let at = wxdragon::utils::get_mouse_position();
    unsafe { ffi::gtk_window_begin_move_drag(window.get_handle(), 1, at.x, at.y, 0); }
}

/// A point of `window` (client coordinates) on the screen.
pub fn to_screen(window: &impl WxWidget, point: Point) -> Point {
    #[cfg(target_os = "linux")]
    unsafe {
        let widget = window.get_handle();
        let gdk = if widget.is_null() { std::ptr::null_mut() } else { ffi::gtk_widget_get_window(widget) };
        if !gdk.is_null() {
            let (mut x, mut y) = (0, 0);
            ffi::gdk_window_get_origin(gdk, &mut x, &mut y);
            if ffi::gtk_widget_get_has_window(widget) == 0 {
                // A widget drawn in its parent's window: add its place there.
                let mut a = ffi::Allocation { x: 0, y: 0, width: 0, height: 0 };
                ffi::gtk_widget_get_allocation(widget, &mut a);
                x += a.x; y += a.y;
            }
            return Point::new(x + point.x, y + point.y);
        }
    }
    window.client_to_screen(point)
}


/// The last mouse press anywhere in the application: screen position and time.
static LAST_PRESS: std::sync::Mutex<Option<(i32, i32, std::time::Instant)>> = std::sync::Mutex::new(None);
/// Shift was held for the last mouse press. wxDragon's mouse events cannot
/// report modifiers (its ShiftDown is for key events only).
static PRESS_SHIFT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The modifier state (GdkModifierType) of the last wheel event.
static SCROLL_STATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(target_os = "linux")]
extern "C" fn on_gdk_event(event: *mut std::ffi::c_void, _data: *mut std::ffi::c_void) {
    // GdkEventButton (GTK 3): type at 0, modifier state (guint) at 48, x_root
    // and y_root (f64) at 64 and 72.
    const GDK_BUTTON_PRESS: i32 = 4;
    const GDK_SHIFT_MASK: u32 = 1;
    // GdkEventScroll: modifier state (guint) at 40.
    const GDK_SCROLL: i32 = 31;
    unsafe {
        if !event.is_null() && *(event as *const i32) == GDK_SCROLL {
            SCROLL_STATE.store(*((event as *const u8).add(40) as *const u32), std::sync::atomic::Ordering::Relaxed);
        }
        if !event.is_null() && *(event as *const i32) == GDK_BUTTON_PRESS {
            let base = event as *const u8;
            let x = *(base.add(64) as *const f64);
            let y = *(base.add(72) as *const f64);
            if let Ok(mut last) = LAST_PRESS.lock() { *last = Some((x.round() as i32, y.round() as i32, std::time::Instant::now())); }
            let state = *(base.add(48) as *const u32);
            PRESS_SHIFT.store(state & GDK_SHIFT_MASK != 0, std::sync::atomic::Ordering::Relaxed);
        }
        ffi::gtk_main_do_event(event);
    }
}

/// Starts noting every mouse press, so dropdowns can close on a press outside.
pub fn watch_presses() {
    #[cfg(target_os = "linux")]
    unsafe { ffi::gdk_event_handler_set(on_gdk_event, std::ptr::null_mut(), std::ptr::null_mut()); }
}

/// Shift and Ctrl held for the wheel event being handled.
pub fn scroll_modifiers() -> (bool, bool) {
    let state = SCROLL_STATE.load(std::sync::atomic::Ordering::Relaxed);
    (state & 1 != 0, state & 4 != 0)
}

/// Whether Shift was held for the mouse press being handled.
pub fn press_with_shift() -> bool { PRESS_SHIFT.load(std::sync::atomic::Ordering::Relaxed) }

/// The last mouse press: where on the screen, and when.
pub fn last_press() -> Option<(Point, std::time::Instant)> {
    LAST_PRESS.lock().ok().and_then(|last| last.map(|(x, y, when)| (Point::new(x, y), when)))
}

/// The current GTK theme's title-bar colours, background and text, read from
/// a header bar's style as GNOME's own title bars use them. None where there
/// is no GTK theme (Windows).
pub fn theme_title_colours() -> Option<([u8; 3], [u8; 3])> {
    #[cfg(target_os = "linux")]
    unsafe {
        use std::sync::atomic::{AtomicPtr, Ordering};
        static HEADER: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
        let mut header = HEADER.load(Ordering::Relaxed);
        if header.is_null() {
            header = ffi::g_object_ref_sink(ffi::gtk_header_bar_new());
            HEADER.store(header, Ordering::Relaxed);
        }
        let context = ffi::gtk_widget_get_style_context(header);
        if context.is_null() { return None; }
        let bytes = |c: ffi::Rgba| [c.red, c.green, c.blue].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        let named = |name: &std::ffi::CStr| {
            let mut c = ffi::Rgba::default();
            (ffi::gtk_style_context_lookup_color(context, name.as_ptr(), &mut c) != 0 && c.alpha > 0.5).then_some(c)
        };
        // The header bar as the theme draws it (many themes paint it with an
        // image, not a plain colour), over the window colour, sampled at its middle.
        let window = named(c"theme_bg_color").map(bytes).unwrap_or([236, 236, 236]);
        let background = {
            const SIDE: i32 = 16;
            let surface = ffi::cairo_image_surface_create(0, SIDE, SIDE);
            let cr = ffi::cairo_create(surface);
            ffi::cairo_set_source_rgb(cr, window[0] as f64 / 255.0, window[1] as f64 / 255.0, window[2] as f64 / 255.0);
            ffi::cairo_paint(cr);
            ffi::gtk_render_background(context, cr, 0.0, 0.0, SIDE as f64, SIDE as f64);
            ffi::cairo_surface_flush(surface);
            let data = ffi::cairo_image_surface_get_data(surface);
            // ARGB32, opaque here: bytes are blue, green, red, alpha.
            let middle = ((SIDE / 2 * SIDE + SIDE / 2) * 4) as usize;
            let pixel = if data.is_null() { None } else { Some([*data.add(middle + 2), *data.add(middle + 1), *data.add(middle)]) };
            ffi::cairo_destroy(cr);
            ffi::cairo_surface_destroy(surface);
            pixel.unwrap_or(window)
        };
        // The text colour that reads best on it, of the theme's own.
        let mut header_text = ffi::Rgba::default();
        ffi::gtk_style_context_get_color(context, 0, &mut header_text);
        let candidates = [Some(header_text).filter(|c| c.alpha > 0.5), named(c"theme_fg_color"), named(c"theme_text_color"), named(c"theme_bg_color")];
        let luminance = |c: [u8; 3]| 0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32;
        let text = candidates.into_iter().flatten().map(bytes)
            .max_by(|a, b| (luminance(*a) - luminance(background)).abs().total_cmp(&(luminance(*b) - luminance(background)).abs()))?;
        return Some((background, text));
    }
    #[allow(unreachable_code)]
    None
}

/// A string setting of GNOME's desktop interface, if this desktop has it
/// (reading a key that does not exist would abort).
#[cfg(target_os = "linux")]
fn interface_setting(key: &std::ffi::CStr) -> Option<String> {
    unsafe {
        let source = ffi::g_settings_schema_source_get_default();
        if source.is_null() { return None; }
        let schema = ffi::g_settings_schema_source_lookup(source, c"org.gnome.desktop.interface".as_ptr(), 1);
        if schema.is_null() { return None; }
        let present = ffi::g_settings_schema_has_key(schema, key.as_ptr()) != 0;
        ffi::g_settings_schema_unref(schema);
        if !present { return None; }
        let settings = ffi::g_settings_new(c"org.gnome.desktop.interface".as_ptr());
        if settings.is_null() { return None; }
        let raw = ffi::g_settings_get_string(settings, key.as_ptr());
        let value = (!raw.is_null()).then(|| std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned());
        if !raw.is_null() { ffi::g_free(raw.cast()); }
        ffi::g_object_unref(settings);
        value
    }
}

/// The desktop's accent colour: GNOME's Appearance setting (or a Yaru theme
/// variant such as Yaru-blue), as the theme draws it. Ubuntu's
/// Yaru has its own shades of the GNOME accents (orange is Ubuntu orange).
pub fn accent_colour() -> Option<[u8; 3]> {
    #[cfg(target_os = "linux")]
    {
        let theme = interface_setting(c"gtk-theme").unwrap_or_default().to_lowercase();
        let yaru = theme.starts_with("yaru");
        let name = interface_setting(c"accent-color").filter(|n| !n.is_empty())
            .or_else(|| theme.strip_prefix("yaru-").map(|v| v.trim_end_matches("-dark").to_string()))
            .unwrap_or_else(|| if yaru { "orange".into() } else { String::new() });
        let hex: u32 = match (name.as_str(), yaru) {
            ("orange", true) => 0xE95420, ("bark", _) => 0x787859, ("sage", _) => 0x657B69,
            ("olive", _) => 0x4B8501, ("viridian", _) => 0x03875B, ("prussiangreen", _) => 0x308280,
            ("magenta", _) => 0xB34CB3,
            ("blue", true) => 0x0073E5, ("teal", true) => 0x308280, ("green", true) => 0x4B8501,
            ("red", true) => 0xDA3450, ("pink", true) => 0xB34CB3, ("purple", true) => 0x7764D8,
            ("slate", true) => 0x657B69, ("yellow", true) => 0xC88800,
            ("blue", false) => 0x3584E4, ("teal", false) => 0x2190A4, ("green", false) => 0x3A944A,
            ("yellow", false) => 0xC88800, ("orange", false) => 0xED5B00, ("red", false) => 0xE62D42,
            ("pink", false) => 0xD56199, ("purple", false) => 0x9141AC, ("slate", false) => 0x6F8396,
            _ => return None,
        };
        return Some([(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]);
    }
    #[allow(unreachable_code)]
    None
}

/// Whether the desktop asks for dark windows (GNOME's Appearance style);
/// None where there is no such setting.
pub fn prefers_dark() -> Option<bool> {
    #[cfg(target_os = "linux")]
    { return interface_setting(c"color-scheme").map(|scheme| scheme == "prefer-dark"); }
    #[allow(unreachable_code)]
    None
}

/// The title bar's colours as GNOME's apps draw them: background and text,
/// active and in the background. They are read from the stylesheets built
/// into the installed libadwaita (the library those apps draw with) for the
/// current light or dark style and accent, so they follow the system rather
/// than a copy kept in Impress. None where libadwaita is not installed.
pub struct TitleBarColours { pub active: [u8; 3], pub backdrop: [u8; 3], pub text: [u8; 3] }

#[cfg(target_os = "linux")]
fn libadwaita_path() -> Option<std::path::PathBuf> {
    let candidates = ["/usr/lib/x86_64-linux-gnu", "/usr/lib/aarch64-linux-gnu", "/usr/lib64", "/usr/lib", "/usr/local/lib"];
    candidates.iter().map(|dir| std::path::Path::new(dir).join("libadwaita-1.so.0")).find(|p| p.is_file())
}

/// The stylesheets built into a library: path inside it to their text.
#[cfg(target_os = "linux")]
fn library_stylesheets(library: &std::path::Path) -> Vec<(String, String)> {
    let Ok(data) = std::fs::read(library) else { return Vec::new(); };
    // A GResource bundle begins with the GVariant database magic.
    let Some(start) = data.windows(8).position(|w| w == b"GVariant") else { return Vec::new(); };
    let bytes = &data[start..];
    let mut sheets = Vec::new();
    unsafe {
        let gbytes = ffi::g_bytes_new(bytes.as_ptr().cast(), bytes.len());
        let mut error = std::ptr::null_mut();
        let resource = ffi::g_resource_new_from_data(gbytes, &mut error);
        ffi::g_bytes_unref(gbytes);
        if resource.is_null() { if !error.is_null() { ffi::g_error_free(error); } return sheets; }
        let folder = c"/org/gnome/Adwaita/styles/";
        let names = ffi::g_resource_enumerate_children(resource, folder.as_ptr(), 0, std::ptr::null_mut());
        if !names.is_null() {
            let mut index = 0;
            while !(*names.add(index)).is_null() {
                let name = std::ffi::CStr::from_ptr(*names.add(index)).to_string_lossy().into_owned();
                index += 1;
                if !name.ends_with(".css") { continue; }
                let Ok(path) = std::ffi::CString::new(format!("/org/gnome/Adwaita/styles/{name}")) else { continue; };
                let found = ffi::g_resource_lookup_data(resource, path.as_ptr(), 0, std::ptr::null_mut());
                if found.is_null() { continue; }
                let mut size = 0usize;
                let text = ffi::g_bytes_get_data(found, &mut size);
                if !text.is_null() {
                    sheets.push((name, String::from_utf8_lossy(std::slice::from_raw_parts(text.cast::<u8>(), size)).into_owned()));
                }
                ffi::g_bytes_unref(found);
            }
            ffi::g_strfreev(names);
        }
        ffi::g_resource_unref(resource);
    }
    sheets
}

/// The last plain colour a stylesheet gives a CSS variable (`--name: #hex;`).
fn css_colour(css: &str, name: &str) -> Option<[u8; 3]> {
    let key = format!("{name}:");
    css.match_indices(&key).filter_map(|(at, _)| {
        let value = css[at + key.len()..].split(';').next()?.trim();
        let hex = value.strip_prefix('#')?;
        let hex = if hex.len() == 3 { hex.chars().flat_map(|c| [c, c]).collect::<String>() } else { hex.to_string() };
        (hex.len() == 6).then(|| u32::from_str_radix(&hex, 16).ok()).flatten()
            .map(|n| [(n >> 16) as u8, (n >> 8) as u8, n as u8])
    }).last()
}

/// Ubuntu's Yaru stylesheets are named after accents of their own.
fn yaru_accent(gnome: &str) -> &str {
    match gnome { "orange" | "" => "default", "teal" => "prussiangreen", "green" => "olive", "pink" => "magenta",
        "slate" => "sage", other => other }
}

pub fn title_bar_colours() -> Option<TitleBarColours> {
    #[cfg(target_os = "linux")]
    {
        use std::sync::Mutex;
        type Cached = (std::path::PathBuf, std::time::SystemTime, bool, String, Option<(u8, [u8; 3], [u8; 3], [u8; 3])>);
        static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
        let library = libadwaita_path()?;
        let modified = std::fs::metadata(&library).and_then(|m| m.modified()).ok()?;
        let dark = prefers_dark().unwrap_or(false);
        let accent = interface_setting(c"accent-color").unwrap_or_default();
        let mut cache = CACHE.lock().ok()?;
        let fresh = cache.as_ref().is_some_and(|(p, m, d, a, _)| *p == library && *m == modified && *d == dark && *a == accent);
        if !fresh {
            let scheme = if dark { "dark" } else { "light" };
            let sheets = library_stylesheets(&library);
            // The stylesheet for this style and accent, else any for this style.
            let wanted = format!("-{scheme}-yaru-{}.css", yaru_accent(&accent));
            let sheet = sheets.iter().find(|(n, _)| n.starts_with("default") && n.ends_with(&wanted))
                .or_else(|| sheets.iter().find(|(n, _)| n.starts_with("default") && n.contains(scheme)))
                .or_else(|| sheets.iter().find(|(n, _)| n.contains(scheme)))
                .or_else(|| sheets.iter().find(|(n, _)| n == "base.css" || n == "defaults.css"));
            let colours = sheet.and_then(|(_, css)| {
                let active = css_colour(css, "--headerbar-bg-color")?;
                let text = css_colour(css, "--headerbar-fg-color").or_else(|| css_colour(css, "--window-fg-color"))?;
                let backdrop = css_colour(css, "--headerbar-backdrop-color").unwrap_or(active);
                Some((0, active, backdrop, text))
            });
            *cache = Some((library, modified, dark, accent, colours));
        }
        let (_, active, backdrop, text) = cache.as_ref()?.4?;
        return Some(TitleBarColours { active, backdrop, text });
    }
    #[allow(unreachable_code)]
    None
}


/// Font inputs use the Ribbon's white/blue field palette rather than inheriting
/// a contrasting desktop entry fill and orange native focus ring.
pub fn style_font_input(control:&impl WxWidget) {
    #[cfg(target_os="linux")]
    unsafe {
        let widget=control.get_handle();
        if widget.is_null() || ffi::g_type_check_instance_is_a(widget,ffi::gtk_widget_get_type())==0{return;}
        let context=ffi::gtk_widget_get_style_context(widget);if context.is_null(){return;}
        ffi::gtk_style_context_add_class(context,c"impress-font-input".as_ptr());
        let provider=ffi::gtk_css_provider_new();
        let css=c"entry.impress-font-input, entry.impress-font-input:focus {background: #ffffff; color: #23324a; border: none; border-radius: 0; box-shadow: none; outline-style: none; padding: 0; min-height: 0;} entry.impress-font-input selection {background: #cfe1ff; color: #23324a;}";
        ffi::gtk_css_provider_load_from_data(provider,css.as_ptr(),-1,std::ptr::null_mut());
        ffi::gtk_style_context_add_provider(context,provider,USER_STYLE_PRIORITY);ffi::g_object_unref(provider);
    }
    #[cfg(not(target_os="linux"))]
    let _=control;
}


/// Keep native theme padding from clipping artwork inside compact bitmap buttons.
pub fn style_compact_bitmap_button(control: &impl WxWidget) {
    #[cfg(target_os = "linux")]
    unsafe {
        let widget = control.get_handle();
        if widget.is_null() || ffi::g_type_check_instance_is_a(widget, ffi::gtk_widget_get_type()) == 0 { return; }
        let context = ffi::gtk_widget_get_style_context(widget);
        if context.is_null() { return; }
        ffi::gtk_style_context_add_class(context, c"impress-compact-bitmap".as_ptr());
        let provider = ffi::gtk_css_provider_new();
        if provider.is_null() { return; }
        let css = c"button.impress-compact-bitmap {padding: 2px; min-width: 0; min-height: 0;}";
        ffi::gtk_css_provider_load_from_data(provider, css.as_ptr(), -1, std::ptr::null_mut());
        ffi::gtk_style_context_add_provider(context, provider, USER_STYLE_PRIORITY);
        ffi::g_object_unref(provider);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = control;
}
