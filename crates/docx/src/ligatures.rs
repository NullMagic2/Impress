//! The representable Office 2010 ligature combinations. This is an adapter,
//! not a glyph substitution engine; absent font substitutions stay absent.
use model::{LigatureMode, OpenTypeTypography};

pub(crate) fn from_word(value: &str) -> Option<OpenTypeTypography> {
    let (ligatures, contextual_ligatures) = match value {
        "none" => (LigatureMode::None, false),
        "standard" => (LigatureMode::Standard, false),
        "standardContextual" => (LigatureMode::Standard, true),
        "standardDiscretional" => (LigatureMode::Discretionary, false),
        "standardContextualDiscretional" => (LigatureMode::Discretionary, true),
        _ => return None,
    };
    Some(OpenTypeTypography { ligatures, contextual_ligatures, ..Default::default() })
}

pub(crate) fn to_word(typography: &OpenTypeTypography) -> &'static str {
    match (typography.ligatures, typography.contextual_ligatures) {
        (LigatureMode::None, _) => "none",
        (LigatureMode::Standard, false) => "standard",
        (LigatureMode::Standard, true) => "standardContextual",
        (LigatureMode::Discretionary, false) => "standardDiscretional",
        (LigatureMode::Discretionary, true) => "standardContextualDiscretional",
    }
}

