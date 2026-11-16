//! Resolve, cache and shape installed and document-local font faces.
use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use std::{collections::{BTreeMap,HashMap},sync::{Arc,Mutex,OnceLock}};
use unicode_script::{Script, UnicodeScript};

/// Process-local font catalogue. The document model stores family names, never
/// `fontdb` IDs, so the file format remains independent from this implementation.
#[derive(Debug)]
pub struct FontCatalog {
    store: Arc<FontStore>,
}

/// Font bytes plus the shaping face parsed from them. Building a rustybuzz
/// face parses every GSUB/GPOS lookup, so it is done once per font rather
/// than once per shaped word.
struct LoadedFont {
    // Declared before `bytes` so it is dropped first: it borrows from them.
    face: rustybuzz::Face<'static>,
    bytes: Arc<Vec<u8>>,
    index: u32,
}
impl LoadedFont {
    fn new(bytes: Arc<Vec<u8>>, index: u32) -> Option<Self> {
        // SAFETY: the slice points into the heap buffer owned by `bytes`, which
        // is never mutated and outlives `face` (field drop order above). The
        // 'static face is only lent out with a caller-bound lifetime.
        let data: &'static [u8] = unsafe { std::slice::from_raw_parts(bytes.as_ptr(), bytes.len()) };
        let face = rustybuzz::Face::from_slice(data, index)?;
        Some(Self { face, bytes, index })
    }
}
impl std::fmt::Debug for LoadedFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedFont").field("bytes", &self.bytes.len()).field("index", &self.index).finish()
    }
}

#[derive(Debug,Default)]
struct ByteCache { entries:HashMap<fontdb::ID,Arc<LoadedFont>>, bytes:usize }
/// Shaping depends only on the font, the text and these options. Documents
/// repeat most words, so each distinct word is shaped once per font and size.
type ShapeKey = (ResolvedFont, String, u32, bool, Option<String>, model::OpenTypeTypography);
struct FontStore {
    db:Database,
    identity:u64,
    data:Mutex<ByteCache>,
    embedded: HashMap<(String,bool,bool),Vec<ResolvedFont>>,
    queries:Mutex<HashMap<(String,bool,bool),Option<ResolvedFont>>>,
    shaped:Mutex<HashMap<ShapeKey,crate::ShapedRun>>,
}
impl std::fmt::Debug for FontStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("FontStore").finish_non_exhaustive() }
}
impl FontStore {fn new(db:Database)->Self{Self{db,identity:NEXT_CATALOG.fetch_add(1,std::sync::atomic::Ordering::Relaxed),embedded:HashMap::new(),data:Mutex::new(ByteCache::default()),queries:Mutex::new(HashMap::new()),shaped:Mutex::new(HashMap::new())}}}
static NEXT_CATALOG:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
static SYSTEM_FONTS:OnceLock<Arc<FontStore>>=OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ResolvedFont {
    pub id: fontdb::ID,
    pub face_index: u32,
    /// fontdb IDs are local to a database; shared outline caches need this namespace.
    pub catalog_id:u64,
}

impl FontCatalog {
    pub fn system() -> Self {
        Self {store:Arc::clone(SYSTEM_FONTS.get_or_init(||{
            let mut db=Database::new(); db.load_system_fonts(); add_family_aliases(&mut db); Arc::new(FontStore::new(db))
        }))}
    }

    /// A private catalog, without system fallback, for isolated font previews
    /// and deterministic shaping tests. The caller owns the font's provenance.
    pub fn from_font_data(data: Vec<u8>) -> Self {
        let mut db=Database::new();db.load_font_data(data);add_family_aliases(&mut db);Self {store:Arc::new(FontStore::new(db))}
    }

    /// A private catalogue cached by immutable payload identity. Embedded faces
    /// never enter the process-wide system database or another document.
    pub fn for_resources(resources: &BTreeMap<model::ResourceId,model::Resource>) -> Self {
        type Owner = (Arc<Vec<u8>>, Vec<model::EmbeddedFontFace>);
        type Entry = (Vec<Owner>, Arc<FontStore>);
        thread_local! { static CATALOGS: std::cell::RefCell<Vec<Entry>> = const { std::cell::RefCell::new(Vec::new()) }; }
        let owners:Vec<Owner> = resources.values().filter(|r|r.media_type==crate::EMBEDDED_FONT_MEDIA_TYPE)
            .map(|r|(r.data.clone(),r.font_faces.clone())).collect();
        if owners.is_empty() {return Self::system();}
        CATALOGS.with(|cache| {
            let mut cache=cache.borrow_mut();
            if let Some((_,store))=cache.iter().find(|(old,_)|old.len()==owners.len()&&old.iter().zip(&owners).all(|((a,fa),(b,fb))|Arc::ptr_eq(a,b)&&fa==fb)) {
                return Self{store:store.clone()};
            }
            let mut store=FontStore::new(Self::system().store.db.clone());
            for (bytes,bindings) in &owners {
                let ids=store.db.load_font_source(fontdb::Source::Binary(bytes.clone()));
                for binding in bindings {
                    if let Some(id)=ids.iter().copied().find(|id|store.db.face(*id).is_some_and(|f|f.index==binding.face_index)) {
                        let preferred=store.db.face(id).is_some_and(|face|face.families.iter().any(|(name,_)|name.eq_ignore_ascii_case(&binding.family)));
                        let fonts=store.embedded.entry((binding.family.to_lowercase(),binding.bold,binding.italic)).or_default();
                        let resolved=ResolvedFont{id,face_index:binding.face_index,catalog_id:store.identity};
                        if preferred {fonts.insert(0,resolved);}else{fonts.push(resolved);}
                    }
                }
            }
            let store=Arc::new(store);
            if cache.len()>=4 {cache.remove(0);}
            cache.push((owners,store.clone()));Self{store}
        })
    }

    pub fn database(&self) -> &Database { &self.store.db }

    /// Installed family names offered by the editor. Use the same database as
    /// shaping so every listed face can also be rendered. Preserve localized
    /// family names while removing case-only duplicates and vertical aliases.
    pub fn family_names(&self) -> Vec<String> {
        let mut names = BTreeMap::new();
        for face in self.store.db.faces() {
            for (name, _) in &face.families {
                let name = name.trim();
                if !name.is_empty() && !name.starts_with('@') {
                    names.entry(name.to_lowercase()).or_insert_with(|| name.to_owned());
                }
            }
        }
        names.into_values().collect()
    }


    pub fn resolve(&self, family: &str, bold: bool, italic: bool) -> Option<ResolvedFont> {
        self.resolve_family(Family::Name(family), bold, italic)
    }

    /// Resolve a requested family and fall back to the platform serif family.
    /// This is a runtime rendering choice only; it never rewrites the document's
    /// stored font-family intent.
    pub fn resolve_or_fallback(&self, family: &str, bold: bool, italic: bool) -> Option<ResolvedFont> {
        self.resolve(family, bold, italic)
            .or_else(|| self.resolve_family(Family::Serif, bold, italic))
    }

    /// Resolve a font that can actually cover the supplied text. The requested
    /// family always wins when it has the glyphs; otherwise script-appropriate
    /// system families are tried before a generic serif/sans fallback.
    pub fn resolve_for_text(&self, family: &str, bold: bool, italic: bool, text: &str) -> Option<ResolvedFont> {
        if let Some(fonts)=self.store.embedded.get(&(family.to_lowercase(),bold,italic)) {
            if let Some(font)=fonts.iter().find(|font|self.font_covers_text(**font,text)) {return Some(*font);}
        }
        let requested = self.resolve(family, bold, italic);
        if requested.is_some_and(|font| self.font_covers_text(font, text)) { return requested; }

        let mut candidates: Vec<&str> = script_fallback_families(text).to_vec();
        // Broad Windows/Linux/macOS families that commonly cover Greek,
        // Cyrillic, symbols and many combining marks.
        candidates.extend(["Segoe UI", "Noto Serif", "Noto Sans", "Arial", "Times New Roman"]);
        for candidate in candidates {
            if candidate.eq_ignore_ascii_case(family) { continue; }
            if let Some(font) = self.resolve(candidate, bold, italic) {
                if self.font_covers_text(font, text) { return Some(font); }
            }
        }

        for generic in [Family::Serif, Family::SansSerif] {
            if let Some(font) = self.resolve_family(generic, bold, italic) {
                if self.font_covers_text(font, text) { return Some(font); }
            }
        }

        // Last-resort behavior matches 0.6: render something rather than make
        // a document unlayoutable because one glyph is unavailable.
        requested.or_else(|| self.resolve_family(Family::Serif, bold, italic))
    }

    pub fn font_covers_text(&self, font: ResolvedFont, text: &str) -> bool {
        self.with_face(font, |face| text.chars().all(|ch| {
            ch.is_whitespace() || ch.is_control() || face.glyph_index(ch).is_some()
        })).unwrap_or(false)
    }

    /// Runs a closure with the raw font bytes. This avoids leaking fontdb's
    /// storage strategy into the rest of Impress and avoids unnecessary copies.
    pub fn with_font_data<T>(&self, font: ResolvedFont, f: impl FnOnce(&[u8], u32) -> T) -> Option<T> {
        let loaded = self.load(font)?;
        Some(f(loaded.bytes.as_slice(), loaded.index))
    }

    /// Runs a closure with the cached, fully parsed face. rustybuzz faces
    /// dereference to `ttf_parser::Face` for outlines and metrics.
    pub fn with_face<T>(&self, font: ResolvedFont, f: impl for<'a> FnOnce(&'a rustybuzz::Face<'a>) -> T) -> Option<T> {
        let loaded = self.load(font)?;
        Some(f(&loaded.face))
    }

    /// Shapes one run with the cached face of a resolved font.
    pub fn shape(&self, font: ResolvedFont, text: &str, options: &crate::ShapeOptions)
        -> Option<Result<crate::ShapedRun, crate::ShapeError>> {
        let key: ShapeKey = (font, text.to_owned(), options.size_pt.to_bits(),
            options.direction == crate::TextDirection::RightToLeft, options.language.clone(), options.typography.clone());
        if let Some(run) = self.store.shaped.lock().unwrap_or_else(|e|e.into_inner()).get(&key) {
            return Some(Ok(run.clone()));
        }
        let result = self.with_face(font, |face| crate::shape_with_face(face, text, options))?;
        if let Ok(run) = &result {
            let mut cache = self.store.shaped.lock().unwrap_or_else(|e|e.into_inner());
            if cache.len() >= 100_000 { cache.clear(); }
            cache.insert(key, run.clone());
        }
        Some(result)
    }

    fn load(&self, font: ResolvedFont) -> Option<Arc<LoadedFont>> {
        if font.catalog_id!=self.store.identity{return None;}
        if let Some(loaded) = self.store.data.lock().unwrap_or_else(|e|e.into_inner()).entries.get(&font.id) {
            return Some(Arc::clone(loaded));
        }
        // Own a bounded copy once; avoid remapping and reparsing the font for
        // each word and every repaint. Parse outside the lock.
        let (bytes,index)=self.store.db.with_face_data(font.id,|data,index|(Arc::new(data.to_vec()),index))?;
        let size = bytes.len();
        let loaded = Arc::new(LoadedFont::new(bytes, index)?);
        if size<=64*1024*1024 {
            let mut cache=self.store.data.lock().unwrap_or_else(|e|e.into_inner());
            if let Some(existing) = cache.entries.get(&font.id) { return Some(Arc::clone(existing)); }
            if cache.bytes+size>64*1024*1024 || cache.entries.len()>=64 {cache.entries.clear();cache.bytes=0;}
            cache.bytes+=size;cache.entries.insert(font.id,Arc::clone(&loaded));
        }
        Some(loaded)
    }

    fn resolve_family(&self, family: Family<'_>, bold: bool, italic: bool) -> Option<ResolvedFont> {
        if let Family::Name(name)=family {
            if let Some(font)=self.store.embedded.get(&(name.to_lowercase(),bold,italic)).and_then(|fonts|fonts.first()) {return Some(*font);}
        }
        let key=(self.store.db.family_name(&family).to_owned(),bold,italic);
        if let Some(result)=self.store.queries.lock().unwrap_or_else(|e|e.into_inner()).get(&key).copied(){return result;}
        // A typographic family may put its real bold face in a condensed
        // width (Neo Aegean does). Prefer an exact weight/style face before
        // the CSS stretch fallback chooses the upright regular instead.
        let exact=if let Family::Name(name)=family {
            let weight=if bold{Weight::BOLD}else{Weight::NORMAL};
            let style=if italic{Style::Italic}else{Style::Normal};
            self.store.db.faces().filter(|face|face.weight==weight&&face.style==style&&face.families.iter().any(|(family,_)|family.eq_ignore_ascii_case(name)))
                .min_by_key(|face|face.stretch.to_number().abs_diff(Stretch::Normal.to_number()))
                .map(|face|ResolvedFont{id:face.id,face_index:face.index,catalog_id:self.store.identity})
        }else{None};
        let families = [family];
        // Missing families are cached too: fallback chains probe several
        // uninstalled families for every word, and a miss scans the database.
        let result = exact.or_else(||self.store.db.query(&Query {
            families: &families,
            weight: if bold { Weight::BOLD } else { Weight::NORMAL },
            stretch: Stretch::Normal,
            style: if italic { Style::Italic } else { Style::Normal },
        }).and_then(|id| self.store.db.face(id).map(|face| ResolvedFont { id, face_index: face.index,catalog_id:self.store.identity })));
        let mut cache=self.store.queries.lock().unwrap_or_else(|e|e.into_inner());
        if cache.len()>=512 {cache.clear();}cache.insert(key,result);result
    }
}

/// Typographic (16), legacy style-linked (1), and WWS (21) names can all
/// identify a family. Keep the original face's weight/style/stretch, so a
/// compact/light alias selects its own faces without rewriting font files.
fn add_family_aliases(db:&mut Database) {
    let faces=db.faces().cloned().collect::<Vec<_>>();
    for mut face in faces {
        let aliases=db.with_face_data(face.id,|data,index|{
            crate::font_family_names(data, index)
        }).unwrap_or_default();
        let mut changed=false;
        for name in aliases {
            if !name.trim().is_empty() && !face.families.iter().any(|(old,_)|old.eq_ignore_ascii_case(&name)) {
                face.families.push((name,fontdb::Language::Unknown));changed=true;
            }
        }
        if changed {db.remove_face(face.id);face.id=fontdb::ID::dummy();db.push_face_info(face);}
    }
}

fn script_fallback_families(text: &str) -> &'static [&'static str] {
    let script = text.chars()
        .find(|ch| ch.is_alphabetic())
        .map(|ch| ch.script())
        .unwrap_or(Script::Common);
    match script {
        Script::Arabic => &["Noto Naskh Arabic", "Arabic Typesetting", "Traditional Arabic", "Segoe UI"],
        Script::Hebrew => &["Noto Sans Hebrew", "Arial Hebrew", "Segoe UI", "Arial"],
        Script::Devanagari => &["Noto Sans Devanagari", "Nirmala UI", "Mangal"],
        Script::Bengali => &["Noto Sans Bengali", "Nirmala UI", "Vrinda"],
        Script::Gujarati => &["Noto Sans Gujarati", "Nirmala UI", "Shruti"],
        Script::Gurmukhi => &["Noto Sans Gurmukhi", "Nirmala UI", "Raavi"],
        Script::Tamil => &["Noto Sans Tamil", "Nirmala UI", "Latha"],
        Script::Telugu => &["Noto Sans Telugu", "Nirmala UI", "Gautami"],
        Script::Kannada => &["Noto Sans Kannada", "Nirmala UI", "Tunga"],
        Script::Malayalam => &["Noto Sans Malayalam", "Nirmala UI", "Kartika"],
        Script::Thai => &["Noto Sans Thai", "Leelawadee UI", "Tahoma"],
        Script::Han => &["Noto Sans CJK SC", "Microsoft YaHei", "Yu Gothic", "PingFang SC"],
        Script::Hiragana | Script::Katakana => &["Noto Sans CJK JP", "Yu Gothic", "Meiryo"],
        Script::Hangul => &["Noto Sans CJK KR", "Malgun Gothic", "Apple SD Gothic Neo"],
        Script::Greek => &["Noto Serif", "Times New Roman", "Segoe UI"],
        Script::Cyrillic => &["Noto Serif", "Times New Roman", "Segoe UI"],
        _ => &[],
    }
}

#[cfg(test)] #[path = "../tests/unit/fonts.rs"] mod tests;
