//! Local spelling providers and the personal dictionary. Provider handles stay
//! on their worker thread; editor positions always use UTF-8 byte offsets.
use model::NodeId;
#[derive(Clone, Debug)]
pub struct Issue {
    pub node: NodeId,
    pub start: usize,
    pub end: usize,
    pub word: String,
    pub language: String,
    pub suggestions: Vec<String>,
}
#[derive(Default)]
pub struct Report {
    pub issues: Vec<Issue>,
    pub limited: bool,
    pub warnings: Vec<String>,
}

#[path = "spelling_local.rs"]
mod local;
pub(crate) use local::{dictionary_root, forget as forget_local_dictionary, validate_pair};

pub(crate) fn system_languages() -> Result<Vec<String>, String> {
    native_languages()
}

/// Installed OS dictionaries and private dictionaries use one language list.
pub fn languages() -> Result<Vec<String>, String> {
    let mut tags = local::languages();
    match native_languages() {
        Ok(native) => tags.extend(native),
        Err(error) if tags.is_empty() => return Err(error),
        Err(_) => {}
    }
    tags.sort();
    tags.dedup();
    Ok(tags)
}

pub fn check(
    language: &str,
    paragraphs: Vec<(NodeId, String)>,
    dictionary: &Dictionary,
) -> Result<Report, String> {
    if let Some(tag) = dictionary_language(language, &local::languages()) {
        return local::check(&tag, paragraphs, dictionary);
    }
    native_check(language, paragraphs, dictionary)
}

/// Convert Windows UTF-16 offsets without slicing a surrogate pair or UTF-8 codepoint.
#[cfg(windows)]
fn byte_at_utf16(text: &str, target: usize) -> Option<usize> {
    let mut units = 0;
    for (offset, c) in text.char_indices() {
        if units == target {
            return Some(offset);
        }
        units += c.len_utf16();
        if units > target {
            return None;
        }
    }
    (units == target).then_some(text.len())
}
#[cfg(target_os = "linux")]
#[path = "spelling_linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
use linux::{check as native_check, languages as native_languages};
#[cfg(not(any(windows, target_os = "linux")))]
fn native_check(_: &str, _: Vec<(NodeId, String)>, _: &Dictionary) -> Result<Report, String> {
    Err("Spellchecking is supported on Linux and Windows.".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
fn native_languages() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

/// Windows factories and enumerators belong to the apartment that made them.
#[cfg(windows)]
fn native_languages() -> Result<Vec<String>, String> {
    std::thread::spawn(|| unsafe {
        use windows::{
            Win32::{
                Globalization::{ISpellCheckerFactory, SpellCheckerFactory},
                System::Com::*,
            },
            core::PWSTR,
        };
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _apartment = Apartment;
        let factory: ISpellCheckerFactory =
            CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| e.to_string())?;
        let strings = factory.SupportedLanguages().map_err(|e| e.to_string())?;
        let mut tags = Vec::new();
        for _ in 0..256 {
            let mut value = [PWSTR::null()];
            let mut fetched = 0;
            let result = strings.Next(&mut value, Some(&mut fetched));
            let tag = if !value[0].is_null() {
                let tag = value[0].to_string();
                CoTaskMemFree(Some(value[0].0.cast()));
                Some(tag)
            } else {
                None
            };
            result.ok().map_err(|e| e.to_string())?;
            if fetched == 0 {
                break;
            }
            if let Some(Ok(tag)) = tag {
                tags.push(tag);
            }
        }
        tags.sort();
        tags.dedup();
        Ok(tags)
    })
    .join()
    .map_err(|_| "Could not list Windows spelling languages".to_string())?
}

#[path = "spelling_languages.rs"]
mod language_catalog;
pub use language_catalog::{LANGUAGES, dictionary_language, language_label};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub language: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            language: "en-US".into(),
        }
    }
}
impl Settings {
    pub fn load() -> Self {
        let value = (|| {
            let path = crate::data_dir::data_dir()
                .ok()?
                .join("spelling-settings.json");
            let file = std::fs::File::open(path).ok()?;
            use std::io::Read;
            serde_json::from_reader::<_, Self>(file.take(4096)).ok()
        })()
        .unwrap_or_else(|| {
            let mut settings = Self::default();
            if let Ok(tags) = languages() {
                if !tags
                    .iter()
                    .any(|tag| tag.eq_ignore_ascii_case(&settings.language))
                {
                    if let Some(tag) = tags.first() {
                        settings.language = tag.clone();
                    }
                }
            }
            settings
        });
        if valid_language(&value.language) {
            value
        } else {
            Self::default()
        }
    }
    pub fn save(&self) -> Result<(), String> {
        let directory = crate::data_dir::data_dir()?;
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        format::atomic_write(&directory.join("spelling-settings.json"), |file| {
            use std::io::Write;
            file.write_all(&bytes).map_err(|e| e.to_string())
        })
    }
}
pub fn valid_language(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 85
        && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
#[cfg(windows)]
fn native_check(
    language: &str,
    paragraphs: Vec<(NodeId, String)>,
    dictionary: &Dictionary,
) -> Result<Report, String> {
    use windows::{
        Win32::{
            Globalization::{ISpellCheckerFactory, SpellCheckerFactory},
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize,
            },
        },
        core::{PCWSTR, PWSTR},
    };
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        let _apartment = Apartment;
        // Declared after the apartment guard: interfaces drop before CoUninitialize.
        let factory: ISpellCheckerFactory =
            CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| e.to_string())?;
        let tag = wide(language);
        if !factory
            .IsSupported(PCWSTR(tag.as_ptr()))
            .map_err(|e| e.to_string())?
            .as_bool()
        {
            return Err(format!(
                "Windows has no installed spelling provider for {language}. Install that language's typing features in Windows Settings, or choose another language tag."
            ));
        }
        let checker = factory
            .CreateSpellChecker(PCWSTR(tag.as_ptr()))
            .map_err(|e| e.to_string())?;
        let mut report = Report::default();
        let mut bytes = 0usize;
        for (node, text) in paragraphs {
            if text.contains('\0') || text.len() > 65536 {
                report.limited = true;
                continue;
            }
            bytes = bytes.saturating_add(text.len());
            if bytes > 2 * 1024 * 1024 {
                report.limited = true;
                break;
            }
            let input = wide(&text);
            let errors = checker
                .Check(PCWSTR(input.as_ptr()))
                .map_err(|e| e.to_string())?;
            loop {
                let mut error = None;
                let hr = errors.Next(&mut error);
                hr.ok().map_err(|e| e.to_string())?;
                let Some(error) = error else {
                    break;
                };
                let first = error.StartIndex().map_err(|e| e.to_string())? as usize;
                let last =
                    first.saturating_add(error.Length().map_err(|e| e.to_string())? as usize);
                let (Some(start), Some(end)) =
                    (byte_at_utf16(&text, first), byte_at_utf16(&text, last))
                else {
                    continue;
                };
                if end <= start || text[start..end].contains('\u{fffc}') {
                    continue;
                }
                let word = text[start..end].to_string();
                if dictionary.contains(language, &word) {
                    continue;
                }
                let word_wide = wide(&word);
                let mut suggestions = Vec::new();
                let strings = checker
                    .Suggest(PCWSTR(word_wide.as_ptr()))
                    .map_err(|e| e.to_string())?;
                for _ in 0..8 {
                    let mut value = [PWSTR::null()];
                    let mut fetched = 0;
                    let hr = strings.Next(&mut value, Some(&mut fetched));
                    // Even on failure, release any pointer the provider supplied.
                    let suggestion = if !value[0].is_null() {
                        let s = value[0].to_string();
                        CoTaskMemFree(Some(value[0].0.cast()));
                        Some(s)
                    } else {
                        None
                    };
                    hr.ok().map_err(|e| e.to_string())?;
                    if fetched == 0 {
                        break;
                    }
                    if let Some(Ok(s)) = suggestion {
                        if !s.is_empty() && s.len() < 1024 {
                            suggestions.push(s);
                        }
                    }
                }
                report.issues.push(Issue {
                    node: node.clone(),
                    start,
                    end,
                    word,
                    suggestions,
                    language: language.to_string(),
                });
                if report.issues.len() >= 256 {
                    report.limited = true;
                    return Ok(report);
                }
            }
        }
        Ok(report)
    }
}

/// The remaining provider offsets can be reused only for non-overlapping issues
/// in the same unchanged paragraph. Byte deltas preserve Greek/emoji offsets.
pub fn adjust_after_replacement(issues: &mut Vec<Issue>, changed: &Issue, new_bytes: usize) {
    issues.retain(|i| i.node != changed.node || i.end <= changed.start || i.start >= changed.end);
    let old_bytes = changed.end - changed.start;
    for i in issues {
        if i.node == changed.node && i.start >= changed.end {
            if new_bytes >= old_bytes {
                i.start += new_bytes - old_bytes;
                i.end += new_bytes - old_bytes;
            } else {
                i.start -= old_bytes - new_bytes;
                i.end -= old_bytes - new_bytes;
            }
        }
    }
}
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Dictionary {
    #[serde(default)]
    words: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
}
impl Dictionary {
    fn path() -> Result<std::path::PathBuf, String> {
        if let Some(path) = std::env::var_os("IMPRESS_SPELLING_DICTIONARY") {
            return Ok(path.into());
        }
        Ok(crate::data_dir::data_dir()?.join("spelling-dictionary.json"))
    }
    fn read(path: &std::path::Path) -> Result<Self, String> {
        use std::io::Read;
        match std::fs::File::open(path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 1024 * 1024 {
                    return Err("Spelling dictionary exceeds 1 MiB".into());
                }
                let value: Self = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
                value.validate()?;
                Ok(value)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Could not read {}: {e}", path.display())),
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.words.len() > 256
            || self.words.iter().any(|(l, words)| {
                l.len() > 85
                    || words.len() > 10000
                    || words
                        .iter()
                        .any(|s| s.is_empty() || s.len() > 1024 || s.contains(['\n', '\r', '\0']))
            })
        {
            return Err("Invalid/oversized spelling dictionary".into());
        }
        Ok(())
    }
    pub fn load() -> Result<Self, String> {
        Self::read(&Self::path()?)
    }
    pub fn contains(&self, language: &str, word: &str) -> bool {
        self.words
            .get(&language.to_ascii_lowercase())
            .is_some_and(|words| words.contains(word))
    }
    pub fn add(&mut self, language: &str, word: &str) -> Result<(), String> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let lock_path = path.with_extension("json.lock");
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| format!("Could not open dictionary lock: {e}"))?;
        lock.try_lock()
            .map_err(|e| format!("Dictionary is in use or cannot be locked: {e}"))?;
        let mut latest = Self::read(&path)?;
        latest
            .words
            .entry(language.to_ascii_lowercase())
            .or_default()
            .insert(word.to_string());
        latest.validate()?;
        let bytes = serde_json::to_vec_pretty(&latest).map_err(|e| e.to_string())?;
        if bytes.len() > 1024 * 1024 {
            return Err("Spelling dictionary exceeds 1 MiB".into());
        }
        format::atomic_write(&path, |f| {
            use std::io::Write;
            f.write_all(&bytes).map_err(|e| e.to_string())
        })?;
        *self = latest;
        drop(lock);
        Ok(())
    }
}
#[cfg(test)]
#[path = "../tests/unit/spelling/session_tests.rs"]
mod session_tests;
