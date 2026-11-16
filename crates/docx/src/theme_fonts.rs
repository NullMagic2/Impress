//! Resolve OOXML theme font selectors to concrete regional model fonts.
use super::*;
use model::{FontHint, FontRegion, FontSlot, FontSlots, ThemeFontReference, ThemeFontSet};
use std::{cell::Cell, rc::Rc};

const FALLBACK_FONT: &str = "Times New Roman";
const THEME_WARNING: &str =
    "Theme-based font selection is not resolved; verify the imported typeface.";
#[derive(Clone, Debug, Default)]
struct FontCollection {
    latin: Option<String>,
    east_asia: Option<String>,
    complex_script: Option<String>,
    scripts: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct ThemeFonts {
    major: Option<FontCollection>,
    minor: Option<FontCollection>,
    latin_language: Option<String>,
    east_asia_language: Option<String>,
    complex_script_language: Option<String>,
    unresolved: Cell<bool>,
}
fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
fn collection(node: Option<&XmlNode>) -> Option<FontCollection> {
    let node = node?;
    Some(FontCollection {
        latin: nonempty(node.child("a:latin").and_then(|n| n.attr("typeface"))),
        east_asia: nonempty(node.child("a:ea").and_then(|n| n.attr("typeface"))),
        complex_script: nonempty(node.child("a:cs").and_then(|n| n.attr("typeface"))),
        scripts: node
            .children_named("a:font")
            .filter_map(|n| Some((n.attr("script")?.to_owned(), nonempty(n.attr("typeface"))?)))
            .collect(),
    })
}
impl ThemeFonts {
    pub(super) fn load<R: Read + std::io::Seek>(
        zip: &mut ZipArchive<R>,
        relations: &BTreeMap<String, Relationship>,
    ) -> FormatResult<Rc<Self>> {
        let theme_path = relations
            .values()
            .find(|r| !r.external && r.kind.ends_with("/theme"))
            .map(|r| relationship_path(&r.target))
            .unwrap_or_else(|| "word/theme/theme1.xml".into());
        let settings_path = relations
            .values()
            .find(|r| !r.external && r.kind.ends_with("/settings"))
            .map(|r| relationship_path(&r.target))
            .unwrap_or_else(|| "word/settings.xml".into());
        let theme = read_optional_text(zip, &theme_path)?
            .map(|xml| parse_supported_tree(&xml))
            .transpose()?;
        let settings = read_optional_text(zip, &settings_path)?
            .map(|xml| parse_supported_tree(&xml))
            .transpose()?;
        let scheme = theme
            .as_ref()
            .and_then(|tree| find_descendant(tree, "a:fontScheme"));
        let language = settings
            .as_ref()
            .and_then(|tree| find_descendant(tree, "w:themeFontLang"));
        Ok(Rc::new(Self {
            major: collection(scheme.and_then(|n| n.child("a:majorFont"))),
            minor: collection(scheme.and_then(|n| n.child("a:minorFont"))),
            latin_language: nonempty(language.and_then(|n| n.attr("w:val"))),
            east_asia_language: nonempty(language.and_then(|n| n.attr("w:eastAsia"))),
            complex_script_language: nonempty(language.and_then(|n| n.attr("w:bidi"))),
            unresolved: Cell::new(false),
        }))
    }
    fn resolve(&self, selector: &str) -> Option<FontSlot> {
        let reference = match selector {
            "majorAscii" => ThemeFontReference {
                set: ThemeFontSet::Major,
                region: FontRegion::Ascii,
            },
            "majorHAnsi" => ThemeFontReference {
                set: ThemeFontSet::Major,
                region: FontRegion::HighAnsi,
            },
            "majorEastAsia" => ThemeFontReference {
                set: ThemeFontSet::Major,
                region: FontRegion::EastAsia,
            },
            "majorBidi" => ThemeFontReference {
                set: ThemeFontSet::Major,
                region: FontRegion::ComplexScript,
            },
            "minorAscii" => ThemeFontReference {
                set: ThemeFontSet::Minor,
                region: FontRegion::Ascii,
            },
            "minorHAnsi" => ThemeFontReference {
                set: ThemeFontSet::Minor,
                region: FontRegion::HighAnsi,
            },
            "minorEastAsia" => ThemeFontReference {
                set: ThemeFontSet::Minor,
                region: FontRegion::EastAsia,
            },
            "minorBidi" => ThemeFontReference {
                set: ThemeFontSet::Minor,
                region: FontRegion::ComplexScript,
            },
            _ => return None,
        };
        let collection = match reference.set {
            ThemeFontSet::Major => self.major.as_ref(),
            ThemeFontSet::Minor => self.minor.as_ref(),
        }?;
        let (family, language) = match reference.region {
            FontRegion::Ascii | FontRegion::HighAnsi => {
                (collection.latin.clone(), self.latin_language.as_deref())
            }
            FontRegion::EastAsia => (
                collection.east_asia.clone(),
                self.east_asia_language.as_deref(),
            ),
            FontRegion::ComplexScript => (
                collection.complex_script.clone(),
                self.complex_script_language.as_deref(),
            ),
        };
        let family = language
            .and_then(model::language_script)
            .and_then(|script| collection.scripts.get(script))
            .cloned()
            .or(family);
        Some(FontSlot {
            family,
            scripts: collection.scripts.clone(),
            fallback_family: collection
                .latin
                .clone()
                .or_else(|| Some(FALLBACK_FONT.into())),
            theme: Some(reference),
        })
    }
    fn read_slot(&self, fonts: &XmlNode, direct: &str, theme: &str) -> Option<FontSlot> {
        let explicit = nonempty(fonts.attr(direct));
        if let Some(selector) = fonts.attr(theme).or_else(|| {
            (theme == "w:cstheme")
                .then(|| fonts.attr("w:csTheme"))
                .flatten()
        }) {
            if let Some(slot) = self.resolve(selector) {
                return Some(slot);
            }
            self.unresolved.set(true);
        }
        explicit.map(FontSlot::explicit)
    }
    pub(super) fn properties(&self, rpr: &XmlNode) -> Option<FontSlots> {
        let mut slots = FontSlots::default();
        if let Some(fonts) = rpr.child("w:rFonts") {
            slots.ascii = self.read_slot(fonts, "w:ascii", "w:asciiTheme");
            slots.h_ansi = self.read_slot(fonts, "w:hAnsi", "w:hAnsiTheme");
            slots.east_asia = self.read_slot(fonts, "w:eastAsia", "w:eastAsiaTheme");
            slots.complex_script = self.read_slot(fonts, "w:cs", "w:cstheme");
            slots.hint = match fonts.attr("w:hint") {
                Some("default") => Some(FontHint::Default),
                Some("eastAsia") => Some(FontHint::EastAsia),
                Some("cs") => Some(FontHint::ComplexScript),
                _ => None,
            };
        }
        slots.force_complex_script = on_off(rpr.child("w:cs"));
        if let Some(language) = rpr.child("w:lang") {
            slots.east_asia_language = nonempty(language.attr("w:eastAsia"));
            slots.complex_script_language = nonempty(language.attr("w:bidi"));
        }
        (slots != FontSlots::default()).then_some(slots)
    }
    pub(super) fn audit(&self, document: &mut Document) {
        if self.unresolved.get() {
            document.compatibility.push(model::CompatibilityIssue {
                code: "theme-fonts".into(),
                part: "word/theme".into(),
                message: THEME_WARNING.into(),
            });
        }
    }
}
