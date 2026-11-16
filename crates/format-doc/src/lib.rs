//! Native Word 97–2003 binary import and export, entirely inside Impress.
mod write;

use format::{DocumentFormat, FormatCapabilities, FormatError, FormatResult, ReadSeek, WriteSeek};
use model::{CompatibilityIssue, Document};
use std::io::{Cursor, Read};

#[derive(Default)]
pub struct DocFormat;

const WORD_STREAM: &str = "/WordDocument";
const WORD_MAGIC: u16 = 0xA5EC;
const WORD97_VERSION: u16 = 0x00C1;
const FIB_BASE_LEN: usize = 108;
const FIB_FLAGS_OFFSET: usize = 10;
const FIB_ENCRYPTED_MASK: u16 = 0x0100;
const MAX_DOC_BYTES: u64 = format::MAX_PACKAGE_TOTAL;
/// Bound rich-model assembly separately from the container's image byte budget.
pub(crate) const MAX_STORY_CHARACTERS: u64 = 4 * 1024 * 1024;
const STORY_COUNT_OFFSETS: &[usize] = &[76, 80, 84, 88, 92, 96, 100, 104];

impl DocumentFormat for DocFormat {
    fn id(&self) -> &'static str {
        "doc"
    }
    fn display_name(&self) -> &'static str {
        "Microsoft Word 97–2003 Document"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["doc"]
    }
    fn capabilities(&self) -> FormatCapabilities {
        FormatCapabilities {
            import: true,
            export: true,
            preserves_unknown_data: false,
            native_equations: false,
        }
    }
    fn import(&self, input: &mut dyn ReadSeek) -> FormatResult<Document> {
        let bytes = read_bounded(input)?;
        validate_binary(&bytes)?;
        let parsed = rwml::Document::open(&bytes)
            .map_err(|error| invalid(format!("Could not decode Word document: {error}")))?;
        let converted = parsed.to_impress_docx();
        if converted.is_empty() || converted.len() as u64 > MAX_DOC_BYTES {
            return Err(invalid(
                "Could not construct the editable Word document within the size limit",
            ));
        }
        let mut document = docx::DocxFormat.import(&mut Cursor::new(converted))?;
        for warning in parsed.impress_conversion_warnings() {
            let message = match warning {
                rwml::DocumentWarning::PackageReadOnly { .. } => continue,
                rwml::DocumentWarning::UnsupportedFieldEvaluation { .. } => {
                    "Unsupported Word fields retain their cached text"
                }
                rwml::DocumentWarning::TrackedChangesPresent { .. }
                | rwml::DocumentWarning::IncompleteRevisionView { .. } => {
                    "Word revision history is not fully preserved"
                }
                rwml::DocumentWarning::FloatingShapePlaceholderOnly { .. } => {
                    "Some floating Word shapes could not be imported"
                }
                rwml::DocumentWarning::ChartsPreservedButNotModeled { .. } => {
                    "Word charts are not supported by this importer"
                }
                rwml::DocumentWarning::OleObjectsPreservedButNotModeled { .. } => {
                    "Some embedded Word objects could not be imported"
                }
                rwml::DocumentWarning::UnsupportedMetafileImages { .. } => {
                    "Some Word metafile images may not display"
                }
                rwml::DocumentWarning::LegacyDocFlattenedSubdocuments { .. } => {
                    "Some Word auxiliary stories may be imported as ordinary text"
                }
            };
            document.compatibility.push(CompatibilityIssue {
                code: "doc.native-import".into(),
                part: "WordDocument".into(),
                message: message.into(),
            });
        }
        Ok(document)
    }
    fn export_warnings(&self, document: &Document) -> Vec<String> {
        write::export_warnings(document)
    }
    fn export(&self, document: &Document, output: &mut dyn WriteSeek) -> FormatResult<()> {
        write::export_doc(document, output)
    }
}

fn read_bounded(input: &mut dyn ReadSeek) -> FormatResult<Vec<u8>> {
    let mut bytes = Vec::new();
    input.take(MAX_DOC_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DOC_BYTES {
        return Err(invalid("Word document exceeds the 512 MiB size limit"));
    }
    Ok(bytes)
}

/// Preflight declared stream/character sizes before allocating the rich model.
fn validate_binary(bytes: &[u8]) -> FormatResult<()> {
    let mut compound = cfb::CompoundFile::open(Cursor::new(bytes))
        .map_err(|_| invalid("This is not a valid Word 97–2003 binary document. Open DOCX and RTF files using their original extensions."))?;
    let total = compound
        .walk()
        .filter(|entry| entry.is_stream())
        .try_fold(0_u64, |total, entry| total.checked_add(entry.len()))
        .filter(|total| *total <= MAX_DOC_BYTES)
        .ok_or_else(|| invalid("Word document streams exceed the size limit"))?;
    if total == 0 {
        return Err(invalid("The binary document has no WordDocument stream"));
    }
    let mut stream = compound
        .open_stream(WORD_STREAM)
        .map_err(|_| invalid("The binary document has no WordDocument stream"))?;
    let mut base = [0; FIB_BASE_LEN];
    stream
        .read_exact(&mut base)
        .map_err(|_| invalid("The Word document header is incomplete"))?;
    if u16::from_le_bytes([base[0], base[1]]) != WORD_MAGIC {
        return Err(invalid("This Word binary format is not supported"));
    }
    if u16::from_le_bytes([base[2], base[3]]) < WORD97_VERSION {
        return Err(invalid(
            "Word 6/95 documents are not supported. Save a Word 97–2003 or DOCX copy first.",
        ));
    }
    let flags = u16::from_le_bytes([base[FIB_FLAGS_OFFSET], base[FIB_FLAGS_OFFSET + 1]]);
    if flags & FIB_ENCRYPTED_MASK != 0 {
        return Err(FormatError::Unsupported { format: "DOC", feature: "Password-protected Word documents cannot be opened. Remove the password, then save a copy.".into() });
    }
    let characters: u64 = STORY_COUNT_OFFSETS
        .iter()
        .map(|&offset| {
            u32::from_le_bytes(
                base[offset..offset + 4]
                    .try_into()
                    .expect("fixed FIB field"),
            ) as u64
        })
        .sum();
    if characters > MAX_STORY_CHARACTERS {
        return Err(invalid(
            "Word document exceeds the supported character limit",
        ));
    }
    Ok(())
}

pub(crate) fn invalid(message: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        format: "DOC",
        message: message.into(),
    }
}
