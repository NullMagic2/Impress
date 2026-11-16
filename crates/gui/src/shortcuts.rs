//! Command keys are distinct from text. Keep Latin layout letters (AZERTY,
//! QWERTZ, Dvorak, etc.), and use the US hardware-key equivalent for other
//! symbols, as wxMSW does. Never translate character/IME input this way.
use wxdragon::event::window_events::KeyboardEvent;

pub(crate) fn letter(key: &KeyboardEvent) -> Option<char> {
    if !key.cmd_down() || alt_graph() { return None; }
    #[cfg(target_os = "linux")]
    if let Some(native) = linux::current_letter() { return native; }
    // Windows supplies virtual-key codes, including for non-Latin layouts.
    key.get_key_code().and_then(ascii_letter)
        .or_else(|| key.get_unicode_key().and_then(ascii_letter))
}

fn ascii_letter(code: i32) -> Option<char> {
    match code {
        65..=90 => char::from_u32(code as u32),
        97..=122 => char::from_u32(code as u32).map(|c| c.to_ascii_uppercase()),
        1..=26 => char::from_u32((b'A' + code as u8 - 1) as u32),
        _ => None,
    }
}

/// Ctrl+Alt is also used by AltGr. Do not turn its text into commands.
pub(crate) fn alt_graph() -> bool {
    #[cfg(target_os = "linux")]
    { linux::alt_graph() }
    #[cfg(windows)]
    { unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0xa5) < 0 } }
    #[cfg(not(any(target_os = "linux", windows)))]
    { false }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{c_char, c_void};

    #[repr(C)]
    struct Names { rules: *const c_char, model: *const c_char, layout: *const c_char,
        variant: *const c_char, options: *const c_char }

    #[link(name = "gtk-3")]
    unsafe extern "C" {
        fn gtk_get_current_event() -> *mut c_void;
    }
    #[link(name = "gdk-3")]
    unsafe extern "C" {
        fn gdk_event_free(event: *mut c_void);
        fn gdk_event_get_keycode(event: *const c_void, code: *mut u16) -> i32;
        fn gdk_event_get_keyval(event: *const c_void, value: *mut u32) -> i32;
        fn gdk_event_get_state(event: *const c_void, state: *mut u32) -> i32;
        fn gdk_keyval_to_unicode(value: u32) -> u32;
    }
    #[link(name = "xkbcommon")]
    unsafe extern "C" {
        fn xkb_context_new(flags: u32) -> *mut c_void;
        fn xkb_context_unref(context: *mut c_void);
        fn xkb_keymap_new_from_names(context: *mut c_void, names: *const Names, flags: u32) -> *mut c_void;
        fn xkb_keymap_unref(keymap: *mut c_void);
        fn xkb_keymap_key_get_syms_by_level(keymap: *mut c_void, key: u32, layout: u32,
            level: u32, syms: *mut *const u32) -> i32;
        #[cfg(test)] fn xkb_keymap_key_by_name(keymap: *mut c_void, name: *const c_char) -> u32;
        #[cfg(test)] fn xkb_keysym_to_utf32(sym: u32) -> u32;
    }

    // GDK uses XKB hardware keycodes on both X11 and Wayland. Use the same
    // reference keymap as wxGTK's native KEY_DOWN translation, without a
    // per-language symbol table or reliance on XKB_DEFAULT_* environment vars.
    struct Keymap(*mut c_void);
    impl Keymap {
        fn new(layout: &std::ffi::CStr, variant: &std::ffi::CStr) -> Option<Self> {
            unsafe {
                let context = xkb_context_new(1 << 1); // NO_ENVIRONMENT_NAMES
                if context.is_null() { return None; }
                let names = Names { rules: c"evdev".as_ptr(), model: c"pc105".as_ptr(),
                    layout: layout.as_ptr(), variant: variant.as_ptr(), options: c"".as_ptr() };
                let map = xkb_keymap_new_from_names(context, &names, 0);
                xkb_context_unref(context);
                (!map.is_null()).then_some(Self(map))
            }
        }
        fn symbol(&self, key: u32, level: u32) -> Option<u32> {
            unsafe {
                let mut syms = std::ptr::null();
                (xkb_keymap_key_get_syms_by_level(self.0, key, 0, level, &mut syms) == 1)
                    .then(|| *syms)
            }
        }
    }
    impl Drop for Keymap { fn drop(&mut self) { unsafe { xkb_keymap_unref(self.0); } } }
    thread_local! { static REFERENCE: Option<Keymap> = Keymap::new(c"us", c""); }

    fn resolve(unicode: u32, hardware: u32) -> Option<char> {
        // Only literal Latin letters have priority over the hardware fallback;
        // control codes and non-Latin Unicode values must not be truncated.
        let logical = char::from_u32(unicode).filter(char::is_ascii_alphabetic)
            .map(|c| c.to_ascii_uppercase());
        logical.or_else(|| REFERENCE.with(|map| {
            let sym = map.as_ref()?.symbol(hardware, 0)?;
            char::from_u32(sym).filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase())
        }))
    }

    struct Event(*mut c_void);
    impl Event {
        fn current() -> Option<Self> {
            let event = unsafe { gtk_get_current_event() };
            (!event.is_null()).then_some(Self(event))
        }
    }
    impl Drop for Event { fn drop(&mut self) { unsafe { gdk_event_free(self.0); } } }

    /// Outer None means there is no native keyboard event (e.g. a synthetic
    /// wx event). Some(None) is a real non-letter key and must stay that way.
    pub(super) fn current_letter() -> Option<Option<char>> {
        let event = Event::current()?;
        let (mut hardware, mut value) = (0, 0);
        unsafe {
            if gdk_event_get_keycode(event.0, &mut hardware) == 0
                || gdk_event_get_keyval(event.0, &mut value) == 0 { return None; }
            // If keyboard definitions are missing, retain the toolkit's
            // virtual-key fallback rather than suppressing a valid shortcut.
            if !REFERENCE.with(|map| map.is_some()) { return None; }
            Some(resolve(gdk_keyval_to_unicode(value), u32::from(hardware)))
        }
    }
    pub(super) fn alt_graph() -> bool {
        let Some(event) = Event::current() else { return false; };
        let mut state = 0;
        // GDK_MOD5_MASK is the XKB level-three selector, including remapped
        // AltGr keys; it isn't tied to a particular physical right-Alt key.
        unsafe { gdk_event_get_state(event.0, &mut state) != 0 && state & (1 << 7) != 0 }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::ffi::CString;

        #[test]
        fn non_latin_layouts_resolve_all_command_letters_by_hardware() {
            // Exercise real installed XKB maps, not a copy of the resolver's
            // mapping. Include final sigma/uppercase sigma and scripts whose
            // Unicode character has no visual relation to the Latin command.
            for layout in ["gr", "ru", "ua", "ara", "il", "th", "am", "ge", "in"] {
                let map = Keymap::new(&CString::new(layout).unwrap(), c"").unwrap();
                let mut non_latin = 0;
                for (name, expected) in [("AD01", 'Q'), ("AD02", 'W'), ("AD03", 'E'),
                    ("AD04", 'R'), ("AD05", 'T'), ("AD06", 'Y'), ("AD07", 'U'),
                    ("AD08", 'I'), ("AD09", 'O'), ("AD10", 'P'), ("AC01", 'A'),
                    ("AC02", 'S'), ("AC03", 'D'), ("AC04", 'F'), ("AC05", 'G'),
                    ("AC06", 'H'), ("AC07", 'J'), ("AC08", 'K'), ("AC09", 'L'),
                    ("AB01", 'Z'), ("AB02", 'X'), ("AB03", 'C'), ("AB04", 'V'),
                    ("AB05", 'B'), ("AB06", 'N'), ("AB07", 'M')] {
                    let hardware = unsafe { xkb_keymap_key_by_name(map.0, CString::new(name).unwrap().as_ptr()) };
                    for level in 0..=1 {
                        let Some(symbol) = map.symbol(hardware, level) else { continue; };
                        let unicode = unsafe { xkb_keysym_to_utf32(symbol) };
                        if char::from_u32(unicode).is_some_and(|c| !c.is_ascii_alphabetic()) {
                            assert_eq!(resolve(unicode, hardware), Some(expected), "{layout}/{name}/{level}");
                            non_latin += 1;
                        }
                    }
                }
                assert!(non_latin > 0, "{layout} must exercise non-Latin input");
            }
        }

        #[test]
        fn latin_layouts_keep_their_letters_instead_of_qwerty_positions() {
            for (layout, variant, name, expected) in [("fr", "", "AD01", 'A'),
                ("de", "", "AD06", 'Z'), ("us", "dvorak", "AC03", 'E'),
                ("tr", "", "AD08", 'I')] {
                let map = Keymap::new(&CString::new(layout).unwrap(), &CString::new(variant).unwrap()).unwrap();
                let hardware = unsafe { xkb_keymap_key_by_name(map.0, CString::new(name).unwrap().as_ptr()) };
                let unicode = unsafe { xkb_keysym_to_utf32(map.symbol(hardware, 0).unwrap()) };
                assert_eq!(resolve(unicode, hardware), Some(expected), "{layout}/{variant}");
            }
        }

        #[test]
        fn non_letter_keys_and_unavailable_hardware_do_not_become_commands() {
            for name in [c"LEFT", c"FK07", c"RTRN", c"LFSH", c"AE01"] {
                let hardware = REFERENCE.with(|map| unsafe { xkb_keymap_key_by_name(map.as_ref().unwrap().0, name.as_ptr()) });
                assert_eq!(resolve(0, hardware), None);
            }
            assert_eq!(resolve('ψ' as u32, 0), None);
            assert_eq!(resolve('c' as u32, 0), Some('C'));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn virtual_key_and_control_code_shortcuts_remain_compatible() {
        for letter in 'A'..='Z' {
            assert_eq!(ascii_letter(letter as i32), Some(letter));
            assert_eq!(ascii_letter(letter.to_ascii_lowercase() as i32), Some(letter));
            assert_eq!(ascii_letter(letter as i32 - 'A' as i32 + 1), Some(letter));
        }
        for value in [0, 27, 314, 'ψ' as i32, 'Ж' as i32] { assert_eq!(ascii_letter(value), None); }
    }
}
