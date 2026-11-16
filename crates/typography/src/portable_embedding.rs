//! Choose an equivalent vendor TrueType face for older Windows font loaders.
use crate::{FontCatalog, check_embedding_permission, standalone_font};
use std::io::Read;
use ttf_parser::{Face, Tag};

/// A companion file is optional; reject unexpectedly large files before parsing.
const MAX_COMPANION_FONT_BYTES: u64 = 64 * 1024 * 1024;
/// These tables preserve font identity, character mapping, metrics and shaping.
const EQUIVALENT_TABLES: &[[u8; 4]] = &[
    *b"name", *b"cmap", *b"hmtx", *b"hhea", *b"vmtx", *b"vhea", *b"GDEF", *b"GPOS", *b"GSUB",
    *b"kern",
];

/// Use a genuine TrueType release of this same face when it is available in the
/// catalogue or beside its installed OpenType file. Outlines are never converted
/// and unrelated faces, versions or permissions are never substituted.
pub fn compatible_truetype_font(
    catalogue: &FontCatalog,
    data: &[u8],
    index: u32,
) -> Option<Vec<u8>> {
    let source = Face::parse(data, index).ok()?;
    if source.tables().cff.is_none() || source.is_variable() {
        return None;
    }
    let postscript_names = source
        .names()
        .into_iter()
        .filter(|name| name.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
        .filter_map(|name| name.to_string())
        .collect::<Vec<_>>();
    for face in catalogue.database().faces() {
        if !postscript_names.contains(&face.post_script_name) {
            continue;
        }
        if let Some(Some(bytes)) = catalogue
            .database()
            .with_face_data(face.id, |bytes, index| {
                equivalent_truetype(&source, bytes, index)
            })
        {
            return Some(bytes);
        }
        let path = match &face.source {
            fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => path,
            fontdb::Source::Binary(_) => continue,
        };
        // Vendors commonly ship both outline formats under the same basename.
        // Do not search arbitrary folders or fetch replacement fonts online.
        for extension in ["ttf", "TTF"] {
            let Ok(file) = std::fs::File::open(path.with_extension(extension)) else {
                continue;
            };
            let mut bytes = Vec::new();
            if file
                .take(MAX_COMPANION_FONT_BYTES + 1)
                .read_to_end(&mut bytes)
                .is_err()
                || bytes.len() as u64 > MAX_COMPANION_FONT_BYTES
            {
                continue;
            }
            if let Some(bytes) = equivalent_truetype(&source, &bytes, 0) {
                return Some(bytes);
            }
        }
    }
    None
}

fn equivalent_truetype(source: &Face<'_>, data: &[u8], index: u32) -> Option<Vec<u8>> {
    let candidate = Face::parse(data, index).ok()?;
    if font_table(&candidate, b"glyf").is_none()
        || candidate.is_variable()
        || source.units_per_em() != candidate.units_per_em()
        || source.number_of_glyphs() != candidate.number_of_glyphs()
        || EQUIVALENT_TABLES
            .iter()
            .any(|tag| font_table(source, tag) != font_table(&candidate, tag))
        || check_embedding_permission(data, index).is_err()
    {
        return None;
    }
    standalone_font(data, index).ok()
}

fn font_table<'a>(face: &Face<'a>, tag: &[u8; 4]) -> Option<&'a [u8]> {
    face.raw_face().table(Tag::from_bytes(tag))
}
