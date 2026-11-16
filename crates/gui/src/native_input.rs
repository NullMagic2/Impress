//! IMM32 bridge for the native document canvas. Preedit is displayed by the
//! system composition window; only committed strings enter the document.
//! The HWND callback queues values, never borrows the editor or holds an IMC
//! while dispatching a Rust/wxWidgets callback.
use std::{cell::{Cell, RefCell}, collections::VecDeque, rc::Rc};
#[derive(Debug)]
pub enum InputEvent { Begin, Preedit(String), Commit(String), Cancel, DoubleClick(i32, i32), CaptureLost, Wheel(crate::wheel::Input) }
#[derive(Default)]
pub struct InputState {
    events: RefCell<VecDeque<InputEvent>>,
    active: Cell<bool>,
    shift: Cell<bool>,
    installed: Cell<bool>,
    caret: Cell<(i32, i32, i32)>,
}
impl InputState {
    pub fn shift_down(&self) -> bool { self.shift.get() }
    pub fn active(&self) -> bool { self.active.get() }
    pub fn installed(&self) -> bool { self.installed.get() }
    pub fn drain(&self) -> Vec<InputEvent> { self.events.borrow_mut().drain(..).collect() }
    fn push(&self, event: InputEvent) {
        let mut queue = self.events.borrow_mut();
        // Consecutive preedit replacements have no semantic effect. Coalesce
        // them, but never coalesce or drop a committed result.
        if matches!(event, InputEvent::Preedit(_)) && matches!(queue.back(), Some(InputEvent::Preedit(_))) { queue.pop_back(); }
        queue.push_back(event);
    }
}
#[cfg(windows)]
mod native {
    use super::*;
    use std::ptr::null_mut;
    use windows_sys::Win32::{Foundation::{HWND, WPARAM, LPARAM, LRESULT, RECT, POINT}, UI::{
        Input::Ime::*, Shell::{SetWindowSubclass, RemoveWindowSubclass, DefSubclassProc}, WindowsAndMessaging::*,
    }};
    const ID: usize = 0x49504D45;
    pub fn install(hwnd: HWND) -> Rc<InputState> {
        let state = Rc::new(InputState::default());
        if hwnd.is_null() { return state; }
        let boxed = Box::into_raw(Box::new(Rc::clone(&state)));
        if unsafe { SetWindowSubclass(hwnd, Some(procedure), ID, boxed as usize) } == 0 {
            unsafe { drop(Box::from_raw(boxed)); }
        } else {
            state.installed.set(true);
            // wxPanel is not a native EDIT control. Associate the thread's
            // default context without taking ownership of its HIMC.
            unsafe { ImmAssociateContextEx(hwnd, null_mut(), IACE_DEFAULT); }
        }
        state
    }
    struct Context { hwnd: HWND, imc: HIMC }
    impl Context { fn get(hwnd: HWND) -> Option<Self> { let imc = unsafe { ImmGetContext(hwnd) }; if imc.is_null() { None } else { Some(Self { hwnd, imc }) } } }
    impl Drop for Context { fn drop(&mut self) { unsafe { ImmReleaseContext(self.hwnd, self.imc); } } }
    fn string(imc: HIMC, flag: u32) -> Option<String> {
        let bytes = unsafe { ImmGetCompositionStringW(imc, flag, null_mut(), 0) };
        if !(0..=1024*1024).contains(&bytes) || bytes % 2 != 0 { return None; }
        let mut text = vec![0u16; bytes as usize / 2];
        let got = unsafe { ImmGetCompositionStringW(imc, flag, text.as_mut_ptr().cast(), bytes as u32) };
        if got < 0 || got > bytes || got % 2 != 0 { return None; }
        text.truncate(got as usize / 2);
        Some(String::from_utf16_lossy(&text))
    }
    pub fn position(hwnd: HWND, state: &InputState, x: i32, y: i32, height: i32) {
        let next = (x.max(0), y.max(0), height.max(1));
        if state.caret.replace(next) == next { return; }
        set_position(hwnd, state);
    }
    fn set_position(hwnd: HWND, state: &InputState) {
        let Some(context) = Context::get(hwnd) else { return; };
        let (x, y, h) = state.caret.get();
        let area = RECT { left: x, top: y, right: x + 2, bottom: y + h };
        let composition = COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: POINT { x, y }, rcArea: area };
        let candidates = CANDIDATEFORM { dwIndex: 0, dwStyle: CFS_EXCLUDE, ptCurrentPos: POINT { x, y: y+h }, rcArea: area };
        unsafe { ImmSetCompositionWindow(context.imc, &composition); ImmSetCandidateWindow(context.imc, &candidates); }
    }
    unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, data: usize) -> LRESULT { unsafe {
        if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(procedure), ID);
            if data != 0 { let owned = Box::from_raw(data as *mut Rc<InputState>); owned.installed.set(false); owned.active.set(false); }
            return DefSubclassProc(hwnd, msg, wp, lp);
        }
        if data == 0 { return DefSubclassProc(hwnd, msg, wp, lp); }
        let state = Rc::clone(&*(data as *const Rc<InputState>));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match msg {
                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                    let horizontal = msg == WM_MOUSEHWHEEL;
                    let mut units: u32 = 3;
                    let setting = if horizontal { SPI_GETWHEELSCROLLCHARS } else { SPI_GETWHEELSCROLLLINES };
                    SystemParametersInfoW(setting, 0, (&mut units as *mut u32).cast(), 0);
                    let rotation = ((wp >> 16) as u16 as i16) as i32;
                    state.push(InputEvent::Wheel(crate::wheel::Input {
                        rotation, horizontal, shift: wp & 4 != 0, control: wp & 8 != 0,
                        units, page_scroll: units == u32::MAX,
                    }));
                    Some(0) // Consumed here; never scroll a second time through wx.
                }
                WM_MOUSEMOVE | WM_LBUTTONDOWN => { state.shift.set(wp & 4 != 0); None },
                WM_LBUTTONDBLCLK => { state.push(InputEvent::DoubleClick((lp as u16 as i16) as i32, ((lp >> 16) as u16 as i16) as i32)); Some(0) },
                WM_CAPTURECHANGED => { state.push(InputEvent::CaptureLost); None },
                WM_IME_STARTCOMPOSITION => {
                    state.active.set(true); state.push(InputEvent::Begin); set_position(hwnd, &state);
                    None // let the native preedit window open
                }
                WM_IME_COMPOSITION => {
                    let Some(context) = Context::get(hwnd) else { return None; };
                    let flags = lp as u32;
                    if flags & GCS_RESULTSTR != 0 {
                        if let Some(text) = string(context.imc, GCS_RESULTSTR) { state.push(InputEvent::Commit(text)); }
                    }
                    if flags & GCS_COMPSTR != 0 {
                        state.active.set(true);
                        if let Some(text) = string(context.imc, GCS_COMPSTR) { state.push(InputEvent::Preedit(text)); }
                    }
                    if flags == 0 { state.push(InputEvent::Cancel); }
                    drop(context);
                    if flags & GCS_RESULTSTR != 0 {
                        // Default handling of the result generates WM_IME_CHAR /
                        // WM_CHAR. Consume that part so it cannot be inserted twice.
                        let composition_flags = flags & !(GCS_RESULTSTR | GCS_RESULTCLAUSE | GCS_RESULTREADSTR | GCS_RESULTREADCLAUSE);
                        if composition_flags != 0 { Some(DefSubclassProc(hwnd, msg, wp, composition_flags as isize)) } else { Some(0) }
                    } else { None }
                }
                WM_IME_ENDCOMPOSITION => { state.active.set(false); state.push(InputEvent::Cancel); None }
                WM_KILLFOCUS => { if state.active.replace(false) { state.push(InputEvent::Cancel); } None }
                _ => None,
            }
        }));
        match result { Ok(Some(value)) => value, _ => DefSubclassProc(hwnd, msg, wp, lp) }
    }}
}
#[cfg(windows)] pub use native::{install, position};
#[cfg(not(windows))]
pub fn install(_hwnd: *mut std::ffi::c_void) -> Rc<InputState> { Rc::new(InputState::default()) }
#[cfg(not(windows))]
pub fn position(_hwnd: *mut std::ffi::c_void, _state: &InputState, _x: i32, _y: i32, _h: i32) {}

#[cfg(windows)]
pub fn set_pointer(pointer: crate::objects_ui::Pointer) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    use crate::objects_ui::Pointer::*;
    let id = match pointer { Arrow => IDC_ARROW, ResizeNwSe => IDC_SIZENWSE, ResizeNeSw => IDC_SIZENESW,
        ResizeColumn => IDC_SIZEWE, ResizeRow => IDC_SIZENS, Move => IDC_SIZEALL, Hand => IDC_HAND,
        // Windows has no stock rotate cursor.
        Rotate => IDC_CROSS };
    unsafe { let cursor = LoadCursorW(std::ptr::null_mut(), id); if !cursor.is_null() { SetCursor(cursor); } }
}
#[cfg(not(windows))] pub fn set_pointer(_pointer: crate::objects_ui::Pointer) {}
