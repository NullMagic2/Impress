//! Private Hunspell-format dictionaries, shared by Linux and Windows.
use super::{Dictionary, Issue, Report};
use model::NodeId;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use unicode_segmentation::UnicodeSegmentation;

const MAX_DICTIONARY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PARAGRAPH_BYTES: usize = 65_536;
const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_ISSUES: usize = 256;
const MAX_WORD_BYTES: usize = 1024;
const MAX_SUGGESTIONS: usize = 8;
const MAX_CACHED_DICTIONARIES: usize = 4;
type Cache = BTreeMap<PathBuf, Arc<Mutex<impress_hunspell::Dictionary>>>;
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

pub(crate) fn dictionary_root() -> Result<PathBuf, String> {
    crate::data_dir::user_data_dir()
        .map(|path| path.join("dictionaries"))
        .ok_or_else(|| "The user data folder is unavailable.".into())
}

pub(super) fn languages() -> Vec<String> {
    let Ok(root) = dictionary_root() else {
        return Vec::new();
    };
    super::LANGUAGES
        .iter()
        .filter(|language| {
            let folder = root.join(language.tag);
            folder.join("dictionary.aff").is_file() && folder.join("dictionary.dic").is_file()
        })
        .map(|language| language.tag.to_string())
        .collect()
}

fn parse_pair(aff: &Path, dic: &Path) -> Result<impress_hunspell::Dictionary, String> {
    for path in [aff, dic] {
        if std::fs::metadata(path)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_DICTIONARY_BYTES
        {
            return Err("The dictionary exceeds the size limit.".into());
        }
    }
    impress_hunspell::Dictionary::new(aff, dic)
}

pub(crate) fn validate_pair(aff: &Path, dic: &Path) -> Result<(), String> {
    parse_pair(aff, dic).map(|_| ())
}

fn load(tag: &str) -> Result<Arc<Mutex<impress_hunspell::Dictionary>>, String> {
    let path = dictionary_root()?.join(tag);
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    // Keep loading and cache eviction ordered when a dictionary is removed
    // and reinstalled while an older spelling worker is still finishing.
    let mut entries = cache.lock().map_err(|_| "Dictionary cache unavailable")?;
    if let Some(dictionary) = entries.get(&path) {
        return Ok(Arc::clone(dictionary));
    }
    let dictionary = Arc::new(Mutex::new(parse_pair(
        &path.join("dictionary.aff"),
        &path.join("dictionary.dic"),
    )?));
    if entries.len() >= MAX_CACHED_DICTIONARIES {
        entries.clear();
    }
    entries.insert(path, Arc::clone(&dictionary));
    Ok(dictionary)
}

pub(crate) fn forget(tag: &str) {
    if let (Ok(root), Some(cache)) = (dictionary_root(), CACHE.get()) {
        if let Ok(mut entries) = cache.lock() {
            entries.remove(&root.join(tag));
        }
    }
}

pub(super) fn check(
    language: &str,
    paragraphs: Vec<(NodeId, String)>,
    personal: &Dictionary,
) -> Result<Report, String> {
    let provider = load(language)?;
    let mut report = Report::default();
    let mut bytes = 0;
    let mut cache = HashMap::<String, Option<Vec<String>>>::new();
    for (node, text) in paragraphs {
        if text.contains('\0') || text.len() > MAX_PARAGRAPH_BYTES {
            report.limited = true;
            continue;
        }
        bytes += text.len();
        if bytes > MAX_DOCUMENT_BYTES {
            report.limited = true;
            break;
        }
        for (start, word) in text.unicode_word_indices() {
            if !word.chars().any(char::is_alphabetic)
                || word.chars().any(char::is_numeric)
                || personal.contains(language, word)
            {
                continue;
            }
            if word.len() > MAX_WORD_BYTES {
                report.limited = true;
                continue;
            }
            let normalized = word.replace('’', "'");
            let suggestions = if let Some(suggestions) = cache.get(&normalized) {
                suggestions.clone()
            } else {
                let mut provider = provider
                    .lock()
                    .map_err(|_| "The dictionary is unavailable.")?;
                let suggestions = if provider.check(&normalized)? {
                    None
                } else {
                    let mut suggestions = provider.suggest(&normalized, MAX_SUGGESTIONS)?;
                    suggestions.retain(|word| {
                        !word.is_empty()
                            && word.len() <= MAX_WORD_BYTES
                            && !word.contains(['\n', '\r', '\0'])
                    });
                    Some(suggestions)
                };
                cache.insert(normalized, suggestions.clone());
                suggestions
            };
            if let Some(suggestions) = suggestions {
                report.issues.push(Issue {
                    node: node.clone(),
                    start,
                    end: start + word.len(),
                    word: word.to_string(),
                    language: language.to_string(),
                    suggestions: suggestions.clone(),
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
