//! Text of scanned pages, read by OCR as Word does when it converts a scan.
//!
//! Tesseract is used when it is installed: it knows about a hundred
//! languages, accents included. Otherwise the built-in ocrs models read the
//! page; they know only unaccented letters, as does a Tesseract with English
//! alone. Their text is therefore kept only when the document reads as
//! English. Otherwise the scans stay pictures and the document notes which
//! Tesseract language to install: accents are never stripped.
use crate::extract::{Line, PageText};
use crate::render::each_page;
use std::collections::BTreeMap;
use std::process::Command;

/// Resolution pages are rendered at for OCR.
const OCR_DPI: f32 = 300.0;
/// A scan covers at least this share of its page.
const SCAN_SHARE: f64 = 0.5;
/// A gap between words wider than this many word heights splits a line
/// (table cells, page columns).
const WORD_GAP_HEIGHTS: f64 = 1.5;
/// Share of common English words in English text (about 40% in running
/// text, under 5% in Portuguese, Spanish or French without accents).
const ENGLISH_SHARE: f64 = 0.15;
/// Estimated sizes within this share of the page's usual size are the same.
const SIZE_SPREAD: f64 = 0.2;

static DETECTION_MODEL: &[u8] = include_bytes!("../assets/ocr/text-detection.rten");
static RECOGNITION_MODEL: &[u8] = include_bytes!("../assets/ocr/text-recognition.rten");

const ENGLISH: &[&str] = &["the", "of", "and", "to", "in", "is", "that", "for", "it", "as", "was", "with", "be", "by", "on", "not",
    "he", "this", "are", "or", "his", "from", "at", "which", "but", "have", "an", "had", "they", "you", "were", "their", "one",
    "all", "we", "can", "her", "has", "there", "been", "if", "more", "when", "will", "would", "who", "so", "no", "its", "our"];

/// Common words of languages with accents, as OCR without accents reads
/// them, with the language's name and Tesseract code.
const ACCENTED: &[(&str, &str, &[&str])] = &[
    ("Portuguese", "por", &["de", "que", "nao", "para", "com", "uma", "os", "no", "se", "na", "por", "mais", "das", "dos", "como",
        "mas", "ao", "ele", "ou", "seu", "sua", "foi", "sao", "esta", "tem", "sobre", "entre", "quando", "muito", "tambem", "pelo", "pela", "ate", "isso"]),
    ("Spanish", "spa", &["de", "la", "que", "el", "en", "los", "del", "se", "las", "por", "un", "para", "con", "una", "su", "al",
        "lo", "como", "pero", "sus", "le", "ya", "este", "si", "porque", "cuando", "muy", "sin", "sobre", "tambien"]),
    ("French", "fra", &["de", "la", "le", "et", "les", "des", "en", "un", "du", "une", "que", "est", "pour", "qui", "dans", "par",
        "plus", "pas", "au", "sur", "ne", "se", "ce", "il", "sont", "avec", "ou", "mais", "comme", "nous", "vous", "leur"]),
    ("German", "deu", &["der", "die", "und", "den", "von", "zu", "das", "mit", "sich", "des", "auf", "fur", "ist", "im", "dem",
        "nicht", "ein", "eine", "als", "auch", "es", "werden", "aus", "er", "hat", "dass", "sie", "nach", "wird", "bei"]),
    ("Italian", "ita", &["di", "il", "la", "che", "per", "un", "del", "non", "una", "sono", "le", "si", "con", "da", "gli",
        "della", "al", "come", "piu", "lo", "ma", "nel", "alla", "anche", "questo", "ha"]),
];

/// A recognised word, in points from the page's top-left corner.
#[derive(Clone, Debug, PartialEq)]
struct Word { text: String, left: f64, top: f64, right: f64, bottom: f64 }

enum Engine { Tesseract { program: String, languages: Vec<String> }, Ocrs }

/// Reads the text of scanned pages into their lines, removing the page
/// picture it replaces. Returns notes for the document.
pub(crate) fn recognise(bytes: &[u8], pages: &mut [PageText]) -> Vec<String> {
    let scanned: Vec<usize> = pages.iter().enumerate().filter(|(_, page)| page.lines.is_empty()
        && page.pictures.iter().any(|p| p.area.width * p.area.height >= page.width * page.height * SCAN_SHARE)).map(|(i, _)| i).collect();
    if scanned.is_empty() { return Vec::new(); }
    let engine = tesseract().map_or(Engine::Ocrs, |(program, languages)| Engine::Tesseract { program, languages });
    let read: Vec<(usize, Vec<Vec<Word>>)> = each_page(bytes, &scanned, OCR_DPI, |index, image| match &engine {
        Engine::Tesseract { program, languages } => run_tesseract(program, languages, index, image),
        Engine::Ocrs => run_ocrs(image),
    });
    let knows_accents = matches!(&engine, Engine::Tesseract { languages, .. } if languages.iter().any(|l| l != "eng"));
    if !knows_accents {
        let words: Vec<String> = read.iter().flat_map(|(_, lines)| lines.iter().flatten()).map(|w| w.text.to_lowercase()).collect();
        if !reads_as_english(&words) {
            let count = scanned.len();
            let (pages, its) = if count == 1 { ("1 scanned page was kept as a picture".to_string(), "its") } else { (format!("{count} scanned pages were kept as pictures"), "their") };
            let package = guess_language(&words).map_or_else(|| "the document's language".to_string(), |(name, code)| format!("{name} (tesseract-ocr-{code})"));
            return vec![format!("{pages}: to read {its} text without losing accents, install Tesseract with {package}.")];
        }
    }
    for (index, lines) in read {
        if lines.is_empty() { continue; }
        let page = &mut pages[index];
        page.lines = lines.iter().flat_map(|words| split_line(words)).collect();
        even_sizes(&mut page.lines);
        let area = page.width * page.height;
        page.pictures.retain(|p| p.area.width * p.area.height < area * SCAN_SHARE);
    }
    Vec::new()
}

/// Tesseract's program and its installed languages, if it is installed.
fn tesseract() -> Option<(String, Vec<String>)> {
    let mut candidates = vec!["tesseract".to_string()];
    if cfg!(windows) { candidates.push(r"C:\Program Files\Tesseract-OCR\tesseract.exe".into()); }
    candidates.into_iter().find_map(|program| {
        let output = quiet(Command::new(&program).arg("--list-langs")).output().ok()?;
        if !output.status.success() { return None; }
        let languages: Vec<String> = String::from_utf8_lossy(&output.stdout).lines().skip(1).map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && !matches!(l.as_str(), "osd" | "equ" | "snum")).collect();
        (!languages.is_empty()).then_some((program, languages))
    })
}

/// No console window on Windows; one thread each, as pages run in parallel.
fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x0800_0000); }
    command.env("OMP_THREAD_LIMIT", "1")
}

fn run_tesseract(program: &str, languages: &[String], index: usize, image: &image::RgbImage) -> Vec<Vec<Word>> {
    let path = std::env::temp_dir().join(format!("impress-ocr-{}-{index}.png", std::process::id()));
    let grey = image::DynamicImage::ImageRgb8(image.clone()).into_luma8();
    if grey.save(&path).is_err() { return Vec::new(); }
    // English first: Tesseract tries the languages in order.
    let mut ordered: Vec<&String> = languages.iter().filter(|l| *l != "eng").take(3).collect();
    if languages.iter().any(|l| l == "eng") { ordered.insert(0, &languages[languages.iter().position(|l| l == "eng").unwrap_or(0)]); }
    let list = ordered.iter().map(|l| l.as_str()).collect::<Vec<_>>().join("+");
    let output = quiet(Command::new(program).arg(&path).arg("stdout").args(["-l", &list, "--psm", "3", "tsv"])).output();
    let _ = std::fs::remove_file(&path);
    output.ok().filter(|o| o.status.success()).map_or_else(Vec::new, |o| parse_tsv(&String::from_utf8_lossy(&o.stdout)))
}

/// Words of Tesseract's TSV output, grouped by line.
fn parse_tsv(tsv: &str) -> Vec<Vec<Word>> {
    let scale = 72.0 / f64::from(OCR_DPI);
    let mut lines: BTreeMap<(u32, u32, u32, u32), Vec<Word>> = BTreeMap::new();
    for row in tsv.lines().skip(1) {
        let cells: Vec<&str> = row.split('\t').collect();
        if cells.len() < 12 || cells[0] != "5" { continue; }
        let text = cells[11].trim();
        if text.is_empty() { continue; }
        let number = |i: usize| cells[i].parse::<f64>().unwrap_or(0.0);
        let key = (cells[1].parse().unwrap_or(0), cells[2].parse().unwrap_or(0), cells[3].parse().unwrap_or(0), cells[4].parse().unwrap_or(0));
        let (left, top) = (number(6) * scale, number(7) * scale);
        lines.entry(key).or_default().push(Word { text: text.to_string(), left, top, right: left + number(8) * scale, bottom: top + number(9) * scale });
    }
    lines.into_values().collect()
}

thread_local! {
    static OCRS: Option<ocrs::OcrEngine> = ocrs_engine();
}

fn ocrs_engine() -> Option<ocrs::OcrEngine> {
    let detection_model = rten::Model::load_static_slice(DETECTION_MODEL).ok()?;
    let recognition_model = rten::Model::load_static_slice(RECOGNITION_MODEL).ok()?;
    ocrs::OcrEngine::new(ocrs::OcrEngineParams { detection_model: Some(detection_model), recognition_model: Some(recognition_model), ..Default::default() }).ok()
}

fn run_ocrs(image: &image::RgbImage) -> Vec<Vec<Word>> {
    use ocrs::TextItem;
    let scale = 72.0 / f64::from(OCR_DPI);
    OCRS.with(|engine| -> Option<Vec<Vec<Word>>> {
        let engine = engine.as_ref()?;
        let input = engine.prepare_input(ocrs::ImageSource::from_bytes(image.as_raw(), image.dimensions()).ok()?).ok()?;
        let boxes = engine.detect_words(&input).ok()?;
        let line_boxes = engine.find_text_lines(&input, &boxes);
        let lines = engine.recognize_text(&input, &line_boxes).ok()?;
        Some(lines.into_iter().flatten().map(|line| line.words().map(|word| {
            let rect = word.bounding_rect();
            Word { text: word.to_string(), left: f64::from(rect.left()) * scale, top: f64::from(rect.top()) * scale,
                right: f64::from(rect.right()) * scale, bottom: f64::from(rect.bottom()) * scale }
        }).filter(|w| !w.text.trim().is_empty()).collect::<Vec<_>>()).filter(|l| !l.is_empty()).collect())
    }).unwrap_or_default()
}

/// A recognised line as importer lines, split at wide gaps.
fn split_line(words: &[Word]) -> Vec<Line> {
    let mut sorted = words.to_vec();
    sorted.sort_by(|a, b| a.left.total_cmp(&b.left));
    let mut heights: Vec<f64> = sorted.iter().map(|w| w.bottom - w.top).collect();
    heights.sort_by(|a, b| a.total_cmp(b));
    let height = heights.get(heights.len() / 2).copied().unwrap_or(10.0).max(1.0);
    let mut out = Vec::new();
    let mut run: Vec<&Word> = Vec::new();
    let flush = |run: &mut Vec<&Word>, out: &mut Vec<Line>| {
        if run.is_empty() { return; }
        let text = run.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        let (top, bottom) = run.iter().fold((f64::MAX, f64::MIN), |(t, b), w| (t.min(w.top), b.max(w.bottom)));
        // A line box runs from ascenders to descenders, about 1.15 em.
        let size = ((bottom - top) / 1.15).max(4.0);
        let baseline = bottom - (bottom - top) * 0.2;
        out.push(Line::plain(&text, run[0].left, run[run.len() - 1].right, baseline, size));
        run.clear();
    };
    for word in &sorted {
        if run.last().is_some_and(|last| word.left - last.right > height * WORD_GAP_HEIGHTS) { flush(&mut run, &mut out); }
        run.push(word);
    }
    flush(&mut run, &mut out);
    out
}

/// Sizes estimated from line boxes vary a little from line to line; those
/// near the page's usual size take it, so a paragraph is one run. Clearly
/// larger lines (headings) keep theirs.
fn even_sizes(lines: &mut [Line]) {
    let mut sizes: Vec<f64> = lines.iter().map(|l| l.size).collect();
    sizes.sort_by(|a, b| a.total_cmp(b));
    let Some(&usual) = sizes.get(sizes.len() / 2) else { return; };
    let usual = (usual * 2.0).round() / 2.0;
    for line in lines.iter_mut().filter(|l| (l.size - usual).abs() <= usual * SIZE_SPREAD) {
        line.size = usual;
        for span in &mut line.spans { span.size = usual; }
    }
}

fn tokens(words: &[String]) -> Vec<&str> {
    words.iter().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric())).filter(|w| !w.is_empty() && w.chars().all(char::is_alphabetic)).collect()
}

fn reads_as_english(words: &[String]) -> bool {
    let tokens = tokens(words);
    if tokens.is_empty() { return true; }
    let common = tokens.iter().filter(|t| ENGLISH.contains(t)).count();
    common as f64 / tokens.len() as f64 >= ENGLISH_SHARE
}

/// The accented language the words most likely are in.
fn guess_language(words: &[String]) -> Option<(&'static str, &'static str)> {
    let tokens = tokens(words);
    if tokens.is_empty() { return None; }
    ACCENTED.iter().map(|(name, code, common)| (tokens.iter().filter(|t| common.contains(t)).count(), *name, *code))
        .filter(|(n, ..)| *n as f64 / tokens.len() as f64 >= 0.1).max_by_key(|(n, ..)| *n).map(|(_, name, code)| (name, code))
}

#[cfg(test)] #[path = "../tests/unit/ocr.rs"] mod tests;
