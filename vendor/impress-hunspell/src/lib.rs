//! Owned Hunspell C handles. A process-wide lock protects upstream global data.
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::Path,
    ptr::NonNull,
    sync::Mutex,
};

static ENGINE: Mutex<()> = Mutex::new(());

unsafe extern "C" {
    fn Hunspell_create(aff: *const c_char, dic: *const c_char) -> *mut c_void;
    fn Hunspell_destroy(handle: *mut c_void);
    fn Hunspell_get_dic_encoding(handle: *mut c_void) -> *const c_char;
    fn Hunspell_spell(handle: *mut c_void, word: *const c_char) -> c_int;
    fn Hunspell_suggest(
        handle: *mut c_void,
        values: *mut *mut *mut c_char,
        word: *const c_char,
    ) -> c_int;
    fn Hunspell_free_list(handle: *mut c_void, values: *mut *mut *mut c_char, count: c_int);
}

/// A dictionary handle that is never used concurrently and never shared raw.
pub struct Dictionary {
    handle: NonNull<c_void>,
    encoding: &'static encoding_rs::Encoding,
}
// All upstream calls, including construction and destruction, hold ENGINE.
// Moving the owned handle between worker threads does not retain thread state.
unsafe impl Send for Dictionary {}

impl Dictionary {
    pub fn new(aff: &Path, dic: &Path) -> Result<Self, String> {
        let path = |path: &Path| -> Result<CString, String> {
            if !path.is_file() {
                return Err("Dictionary files are missing.".into());
            }
            CString::new(
                path.to_str()
                    .ok_or("The dictionary path is not valid Unicode.")?,
            )
            .map_err(|_| "The dictionary path contains an invalid character.".into())
        };
        let aff = path(aff)?;
        let dic = path(dic)?;
        let lock = ENGINE
            .lock()
            .map_err(|_| "The dictionary engine is unavailable.")?;
        let handle = NonNull::new(unsafe { Hunspell_create(aff.as_ptr(), dic.as_ptr()) })
            .ok_or("Could not open the dictionary.")?;
        let name = unsafe { Hunspell_get_dic_encoding(handle.as_ptr()) };
        let encoding = if name.is_null() {
            None
        } else {
            encoding_rs::Encoding::for_label(unsafe { CStr::from_ptr(name) }.to_bytes())
        };
        let Some(encoding) = encoding else {
            unsafe {
                Hunspell_destroy(handle.as_ptr());
            }
            return Err("The dictionary uses an unsupported text encoding.".into());
        };
        drop(lock);
        Ok(Self { handle, encoding })
    }

    fn word(&self, word: &str) -> Option<CString> {
        let (bytes, _, errors) = self.encoding.encode(word);
        if errors {
            return None;
        }
        CString::new(bytes.as_ref()).ok()
    }

    pub fn check(&mut self, word: &str) -> Result<bool, String> {
        let Some(word) = self.word(word) else {
            return Ok(false);
        };
        let _lock = ENGINE
            .lock()
            .map_err(|_| "The dictionary engine is unavailable.")?;
        Ok(unsafe { Hunspell_spell(self.handle.as_ptr(), word.as_ptr()) } != 0)
    }

    pub fn suggest(&mut self, word: &str, limit: usize) -> Result<Vec<String>, String> {
        let Some(word) = self.word(word) else {
            return Ok(Vec::new());
        };
        let _lock = ENGINE
            .lock()
            .map_err(|_| "The dictionary engine is unavailable.")?;
        let mut values = std::ptr::null_mut();
        let count = unsafe { Hunspell_suggest(self.handle.as_ptr(), &mut values, word.as_ptr()) };
        let mut suggestions = Vec::new();
        if count > 0 && !values.is_null() {
            for index in 0..(count as usize).min(limit) {
                let value = unsafe { *values.add(index) };
                if value.is_null() {
                    continue;
                }
                let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
                let (text, _, errors) = self.encoding.decode(bytes);
                if !errors {
                    suggestions.push(text.into_owned());
                }
            }
        }
        if !values.is_null() {
            unsafe {
                Hunspell_free_list(self.handle.as_ptr(), &mut values, count.max(0));
            }
        }
        Ok(suggestions)
    }
}
impl Drop for Dictionary {
    fn drop(&mut self) {
        let _lock = ENGINE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        unsafe {
            Hunspell_destroy(self.handle.as_ptr());
        }
    }
}
