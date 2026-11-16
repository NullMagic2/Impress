//! Enchant 2 adapter. Enchant uses installed Hunspell/Aspell dictionaries and
//! accepts UTF-8, so words retain their original document byte positions.
use super::{Dictionary, Issue, Report};
use libloading::Library;
use model::NodeId;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use unicode_segmentation::UnicodeSegmentation;

const MAX_PARAGRAPH_BYTES: usize = 65_536;
const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_ISSUES: usize = 256;
const MAX_SUGGESTIONS: usize = 8;
const MAX_WORD_BYTES: usize = 1024;

type Describe =
    unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, *const c_char, *mut c_void);

/// The shared library lives for the process. Each broker owns its requested
/// dictionaries, freed before the broker on every return path.
struct Broker {
    handle: *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
    request: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    free_dictionary: unsafe extern "C" fn(*mut c_void, *mut c_void),
    list: unsafe extern "C" fn(*mut c_void, Describe, *mut c_void),
    check: unsafe extern "C" fn(*mut c_void, *const c_char, isize) -> c_int,
    suggest:
        unsafe extern "C" fn(*mut c_void, *const c_char, isize, *mut usize) -> *mut *mut c_char,
    free_suggestions: unsafe extern "C" fn(*mut c_void, *mut *mut c_char),
    _library: &'static Library,
}
impl Broker {
    fn new() -> Result<Self, String> {
        // Only load the system soname; documents cannot supply library paths.
        unsafe {
            // Enchant registers process-global GLib types. Keep the library
            // loaded between checks so their vtables cannot become dangling.
            static LIBRARY: std::sync::OnceLock<Result<Library, String>> =
                std::sync::OnceLock::new();
            let library = LIBRARY.get_or_init(|| Library::new("libenchant-2.so.2").map_err(|_| "Install libenchant-2-2 and a spelling dictionary (for example hunspell-en-us or hunspell-pt-br), then restart Impress.".to_string()))
                .as_ref().map_err(Clone::clone)?;
            let init: unsafe extern "C" fn() -> *mut c_void = *library
                .get(b"enchant_broker_init\0")
                .map_err(|e| e.to_string())?;
            let free = *library
                .get(b"enchant_broker_free\0")
                .map_err(|e| e.to_string())?;
            let request = *library
                .get(b"enchant_broker_request_dict\0")
                .map_err(|e| e.to_string())?;
            let free_dictionary = *library
                .get(b"enchant_broker_free_dict\0")
                .map_err(|e| e.to_string())?;
            let list = *library
                .get(b"enchant_broker_list_dicts\0")
                .map_err(|e| e.to_string())?;
            let check = *library
                .get(b"enchant_dict_check\0")
                .map_err(|e| e.to_string())?;
            let suggest = *library
                .get(b"enchant_dict_suggest\0")
                .map_err(|e| e.to_string())?;
            let free_suggestions = *library
                .get(b"enchant_dict_free_string_list\0")
                .map_err(|e| e.to_string())?;
            let handle = init();
            if handle.is_null() {
                return Err("Could not start the installed Enchant spelling service".into());
            }
            Ok(Self {
                handle,
                free,
                request,
                free_dictionary,
                list,
                check,
                suggest,
                free_suggestions,
                _library: library,
            })
        }
    }
    fn languages(&self) -> Vec<String> {
        unsafe extern "C" fn collect(
            tag: *const c_char,
            _: *const c_char,
            _: *const c_char,
            _: *const c_char,
            data: *mut c_void,
        ) {
            if tag.is_null() {
                return;
            }
            unsafe {
                let tags = &mut *data.cast::<Vec<String>>();
                let tag = CStr::from_ptr(tag).to_string_lossy().replace('_', "-");
                if tags.len() < 256 && super::valid_language(&tag) {
                    tags.push(tag);
                }
            }
        }
        let mut tags = Vec::<String>::new();
        unsafe {
            (self.list)(self.handle, collect, (&mut tags as *mut Vec<String>).cast());
        }
        tags.sort();
        tags.dedup();
        tags
    }
}
impl Drop for Broker {
    fn drop(&mut self) {
        unsafe {
            (self.free)(self.handle);
        }
    }
}
struct ProviderDictionary<'a> {
    broker: &'a Broker,
    handle: *mut c_void,
}
impl Drop for ProviderDictionary<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.broker.free_dictionary)(self.broker.handle, self.handle);
        }
    }
}
pub fn languages() -> Result<Vec<String>, String> {
    let mut tags = Broker::new()?.languages();
    // Aspell also enumerates implementation profiles such as en-variant-0
    // and en-GB-w-accents. Show language/script/region choices, not dozens
    // of duplicate internal profiles in the user-facing language menu.
    tags.retain(|tag| {
        let parts = tag.split('-').collect::<Vec<_>>();
        let language = |part: &str| {
            (2..=3).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphabetic())
        };
        let region = |part: &str| {
            (part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_alphabetic()))
                || (part.len() == 3 && part.bytes().all(|byte| byte.is_ascii_digit()))
        };
        let script =
            |part: &str| part.len() == 4 && part.bytes().all(|byte| byte.is_ascii_alphabetic());
        match parts.as_slice() {
            [base] => language(base),
            [base, qualifier] => language(base) && (region(qualifier) || script(qualifier)),
            [base, script_tag, region_tag] => {
                language(base) && script(script_tag) && region(region_tag)
            }
            _ => false,
        }
    });
    let qualified = tags
        .iter()
        .filter_map(|tag| {
            tag.split_once('-')
                .map(|(base, _)| base.to_ascii_lowercase())
        })
        .collect::<std::collections::BTreeSet<_>>();
    tags.retain(|tag| tag.contains('-') || !qualified.contains(&tag.to_ascii_lowercase()));
    Ok(tags)
}

pub fn check(
    language: &str,
    paragraphs: Vec<(NodeId, String)>,
    dictionary: &Dictionary,
) -> Result<Report, String> {
    if !super::valid_language(language) {
        return Err("Choose a valid spelling language".into());
    }
    let broker = Broker::new()?;
    let tags = broker.languages();
    let selected = super::dictionary_language(language, &tags).ok_or_else(|| {
        format!(
            "Dictionary not installed for {}.",
            super::language_label(language)
        )
    })?;
    let tag = CString::new(selected.replace('-', "_")).map_err(|e| e.to_string())?;
    let handle = unsafe { (broker.request)(broker.handle, tag.as_ptr()) };
    if handle.is_null() {
        return Err(format!(
            "Could not open the installed {selected} dictionary"
        ));
    }
    let provider = ProviderDictionary {
        broker: &broker,
        handle,
    };
    let mut report = Report::default();
    let mut bytes = 0usize;
    // Repeated words occur frequently; only ask the provider once per spelling.
    let mut cache = std::collections::HashMap::<String, Option<Vec<String>>>::new();
    for (node, text) in paragraphs {
        if text.contains('\0') || text.len() > MAX_PARAGRAPH_BYTES {
            report.limited = true;
            continue;
        }
        bytes = bytes.saturating_add(text.len());
        if bytes > MAX_DOCUMENT_BYTES {
            report.limited = true;
            break;
        }
        for (start, word) in text.unicode_word_indices() {
            if !word.chars().any(char::is_alphabetic)
                || word.chars().any(char::is_numeric)
                || dictionary.contains(language, word)
            {
                continue;
            }
            if word.len() > MAX_WORD_BYTES {
                report.limited = true;
                continue;
            }
            let normalized = word.replace('’', "'");
            let suggestions = if let Some(cached) = cache.get(&normalized) {
                cached.clone()
            } else {
                let input = CString::new(normalized.as_str()).map_err(|e| e.to_string())?;
                let result = unsafe {
                    (broker.check)(provider.handle, input.as_ptr(), normalized.len() as isize)
                };
                if result < 0 {
                    return Err(format!(
                        "The {selected} spelling dictionary could not check this text"
                    ));
                }
                let result = if result == 0 {
                    None
                } else {
                    let mut count = 0;
                    let values = unsafe {
                        (broker.suggest)(
                            provider.handle,
                            input.as_ptr(),
                            normalized.len() as isize,
                            &mut count,
                        )
                    };
                    let mut suggestions = Vec::new();
                    if !values.is_null() {
                        for index in 0..count.min(MAX_SUGGESTIONS) {
                            let value = unsafe { *values.add(index) };
                            if value.is_null() {
                                continue;
                            }
                            if let Ok(value) = unsafe { CStr::from_ptr(value) }.to_str() {
                                if !value.is_empty() && value.len() <= MAX_WORD_BYTES {
                                    suggestions.push(value.to_string());
                                }
                            }
                        }
                        unsafe {
                            (broker.free_suggestions)(provider.handle, values);
                        }
                    }
                    Some(suggestions)
                };
                cache.insert(normalized, result.clone());
                result
            };
            if let Some(suggestions) = suggestions {
                report.issues.push(Issue {
                    node: node.clone(),
                    start,
                    end: start + word.len(),
                    word: word.to_string(),
                    suggestions,
                    language: language.to_string(),
                });
                if report.issues.len() >= MAX_ISSUES {
                    report.limited = true;
                    return Ok(report);
                }
            }
        }
    }
    Ok(report)
}
