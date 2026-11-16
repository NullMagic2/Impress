//! Independent-producer input and bounded malformed Word97 documents.
use format::{DocumentFormat, FormatError};
use format_doc::DocFormat;
use model::*;
use std::io::{Cursor, Read, Write};

const INDEPENDENT_DOC: &[u8] = include_bytes!("fixtures/libreoffice-word97.doc");
const WORD_STREAM: &str = "/WordDocument";
const FIB_FLAGS: usize = 10;
const FIB_STORY_COUNT: usize = 76;
const FIB_PAIRS: usize = 154;
const CLX_PAIR: usize = 33;
const FIB_PAIR_BYTES: usize = 8;
const CHARACTER_LIMIT: u32 = 4 * 1024 * 1024;

fn import(bytes: &[u8]) -> Result<Document, FormatError> {
    DocFormat.import(&mut Cursor::new(bytes))
}
fn mutate_stream(bytes: &[u8], name: &str, mutate: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut compound = cfb::CompoundFile::open(Cursor::new(bytes.to_vec())).unwrap();
    let mut stream = Vec::new();
    compound
        .open_stream(name)
        .unwrap()
        .read_to_end(&mut stream)
        .unwrap();
    mutate(&mut stream);
    compound
        .create_stream(name)
        .unwrap()
        .write_all(&stream)
        .unwrap();
    compound.into_inner().into_inner()
}
fn mutate_word(mutate: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    mutate_stream(INDEPENDENT_DOC, WORD_STREAM, mutate)
}
fn table_stream(bytes: &[u8]) -> String {
    let mut compound = cfb::CompoundFile::open(Cursor::new(bytes)).unwrap();
    let mut word = Vec::new();
    compound
        .open_stream(WORD_STREAM)
        .unwrap()
        .read_to_end(&mut word)
        .unwrap();
    if u16::from_le_bytes(word[FIB_FLAGS..FIB_FLAGS + 2].try_into().unwrap()) & 0x200 != 0 {
        "/1Table"
    } else {
        "/0Table"
    }
    .into()
}
fn clx_range(bytes: &[u8]) -> (usize, usize) {
    let mut compound = cfb::CompoundFile::open(Cursor::new(bytes)).unwrap();
    let mut word = Vec::new();
    compound
        .open_stream(WORD_STREAM)
        .unwrap()
        .read_to_end(&mut word)
        .unwrap();
    let offset = FIB_PAIRS + CLX_PAIR * FIB_PAIR_BYTES;
    (
        u32::from_le_bytes(word[offset..offset + 4].try_into().unwrap()) as usize,
        u32::from_le_bytes(word[offset + 4..offset + 8].try_into().unwrap()) as usize,
    )
}
fn rejected(bytes: &[u8]) {
    assert!(
        import(bytes).is_err(),
        "malformed native DOC must fail instead of producing a partial document"
    );
}

#[test]
fn independent_word97_import_keeps_text_table_page_setup_and_live_header() {
    let document = import(INDEPENDENT_DOC).expect("independent Word97 fixture");
    let paragraphs = document_paragraphs(&document);
    let heading = paragraphs
        .iter()
        .find(|p| inline_text(&p.content) == "Résumé Ελληνικά & α β")
        .unwrap();
    let Inline::Text(run) = &heading.content[0] else {
        panic!("Greek heading text")
    };
    assert_eq!(run.properties.bold, Some(false));
    let table = document
        .sections
        .iter()
        .flat_map(|section| section.blocks.iter())
        .find_map(|block| {
            if let Block::Table(table) = block {
                Some(table)
            } else {
                None
            }
        })
        .expect("independent native table");
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].cells.len(), 1);
    let numbered = paragraphs
        .iter()
        .find(|p| inline_text(&p.content) == "Numbered merged cell")
        .unwrap();
    let list_id = numbered
        .properties
        .list_id
        .as_ref()
        .expect("native list reference");
    assert_eq!(
        document.lists[list_id].levels[0].format,
        ListFormat::Decimal
    );
    assert_eq!(document.lists[list_id].levels[0].start_at, 1);
    assert_eq!(document.sections.len(), 2);
    assert!((document.sections[0].properties.page_width_mm - 210.0).abs() < 0.04);
    assert!((document.sections[1].properties.page_width_mm - 215.9).abs() < 0.04);
    assert!((document.sections[1].properties.page_height_mm - 279.4).abs() < 0.04);
    let normal = document.styles.id_by_name("Normal").unwrap();
    let normal = document.styles.resolve_character(Some(&normal));
    assert_eq!(normal.font_family.as_deref(), Some("Arial"));
    assert_eq!(normal.font_size_pt, Some(12.0));
    let header = document.sections[0]
        .headers
        .default
        .as_ref()
        .expect("default header");
    let mut header_paragraphs = Vec::new();
    visit_paragraphs(&header.blocks, &mut header_paragraphs);
    assert!(header_paragraphs
        .iter()
        .flat_map(|p| p.content.iter())
        .any(|inline| matches!(
            inline,
            Inline::Field(Field {
                kind: FieldKind::PageNumber,
                ..
            })
        )));
    let image = paragraphs
        .iter()
        .flat_map(|p| p.content.iter())
        .find_map(|inline| {
            if let Inline::Image(image) = inline {
                Some(image)
            } else {
                None
            }
        })
        .expect("native raster");
    assert!((image.width_mm - 50.8).abs() < 0.04);
    assert!((image.height_mm - 25.4).abs() < 0.04);
    assert!(image::load_from_memory(&document.resources[&image.resource].data).is_ok());
}
#[test]
fn independent_word97_import_keeps_anchored_note_comment_and_bookmark() {
    let document = import(INDEPENDENT_DOC).unwrap();
    assert!(
        !document
            .compatibility
            .iter()
            .any(|issue| issue.message.contains("ordinary text")),
        "valid source stories must be promoted instead of diagnosed as flattened"
    );
    assert_eq!(footnote_references(&document).len(), 1);
    let note = document.review.footnotes.values().next().unwrap();
    assert_eq!(note.kind, NoteKind::Footnote);
    let mut paragraphs = Vec::new();
    visit_paragraphs(&note.blocks, &mut paragraphs);
    assert_eq!(
        paragraphs
            .iter()
            .map(|p| inline_text(&p.content))
            .collect::<Vec<_>>()
            .join("\n")
            .trim(),
        "One note α."
    );
    let bookmark = document
        .review
        .bookmarks
        .values()
        .find(|bookmark| bookmark.name == "Topic")
        .unwrap();
    assert!(valid_range(&document, bookmark.range.as_ref().unwrap()));
    let comment = document.review.comments.values().next().unwrap();
    assert_eq!(comment.author, "Reviewer");
    assert_eq!(comment.text, "Check Greek & accents.");
    assert!(valid_range(&document, comment.range.as_ref().unwrap()));
    assert_eq!(comment.range, bookmark.range);
}
#[test]
fn non_compound_missing_word_stream_and_truncated_headers_are_rejected() {
    for bytes in [
        &b"not a Word document"[..],
        &b"PK\x03\x04not a DOCX package"[..],
        &INDEPENDENT_DOC[..256],
    ] {
        rejected(bytes);
    }
    let mut compound = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    compound
        .create_stream("/Unrelated")
        .unwrap()
        .write_all(b"unrelated data")
        .unwrap();
    rejected(&compound.into_inner().into_inner());
    rejected(&mutate_word(|word| word.truncate(64)));
}
#[test]
fn unsupported_old_or_password_protected_word_documents_fail_clearly() {
    let old = mutate_word(|word| word[2..4].copy_from_slice(&0x0065u16.to_le_bytes()));
    let error = import(&old).unwrap_err().to_string();
    assert!(error.contains("Word 6/95"));
    let encrypted = mutate_word(|word| {
        let flags = u16::from_le_bytes(word[FIB_FLAGS..FIB_FLAGS + 2].try_into().unwrap());
        word[FIB_FLAGS..FIB_FLAGS + 2].copy_from_slice(&(flags | 0x100).to_le_bytes());
    });
    let error = import(&encrypted).unwrap_err();
    assert!(matches!(
        error,
        FormatError::Unsupported { format: "DOC", .. }
    ));
    assert!(error.to_string().contains("password"));
    rejected(&mutate_word(|word| {
        word[0..2].copy_from_slice(&0x0000u16.to_le_bytes())
    }));
}
#[test]
fn excessive_story_counts_and_missing_piece_tables_are_rejected() {
    let excessive = mutate_word(|word| {
        word[FIB_STORY_COUNT..FIB_STORY_COUNT + 4]
            .copy_from_slice(&(CHARACTER_LIMIT + 1).to_le_bytes())
    });
    let error = import(&excessive).unwrap_err().to_string();
    assert!(error.contains("character limit"));
    let absent = mutate_word(|word| {
        let offset = FIB_PAIRS + CLX_PAIR * FIB_PAIR_BYTES;
        word[offset..offset + 8].fill(0);
    });
    rejected(&absent);
    let outside = mutate_word(|word| {
        let offset = FIB_PAIRS + CLX_PAIR * FIB_PAIR_BYTES;
        word[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    });
    rejected(&outside);
}
#[test]
fn truncated_piece_tables_and_text_pointers_outside_the_stream_are_rejected() {
    let (offset, length) = clx_range(INDEPENDENT_DOC);
    let stream = table_stream(INDEPENDENT_DOC);
    let truncated = mutate_word(|word| {
        let pair = FIB_PAIRS + CLX_PAIR * FIB_PAIR_BYTES;
        word[pair + 4..pair + 8].copy_from_slice(&3u32.to_le_bytes());
    });
    rejected(&truncated);
    let outside = mutate_stream(INDEPENDENT_DOC, &stream, |table| {
        let mut position = offset;
        while table[position] == 1 {
            let length =
                u16::from_le_bytes(table[position + 1..position + 3].try_into().unwrap()) as usize;
            position += 3 + length;
        }
        assert_eq!(table[position], 2);
        let piece_bytes =
            u32::from_le_bytes(table[position + 1..position + 5].try_into().unwrap()) as usize;
        let pieces = (piece_bytes - 4) / 12;
        assert!(pieces > 0 && position + 5 + piece_bytes <= offset + length);
        let first = position + 5 + (pieces + 1) * 4;
        table[first + 2..first + 6].copy_from_slice(&0x7FFFFF00u32.to_le_bytes());
    });
    rejected(&outside);
}

#[test]
fn non_monotonic_and_excessive_piece_character_ranges_are_rejected() {
    let (offset, _) = clx_range(INDEPENDENT_DOC);
    let stream = table_stream(INDEPENDENT_DOC);
    let mutate_positions = |mutation: fn(&mut [u8], usize, usize)| {
        mutate_stream(INDEPENDENT_DOC, &stream, |table| {
            let mut position = offset;
            while table[position] == 1 {
                let length =
                    u16::from_le_bytes(table[position + 1..position + 3].try_into().unwrap())
                        as usize;
                position += 3 + length;
            }
            assert_eq!(table[position], 2);
            let length =
                u32::from_le_bytes(table[position + 1..position + 5].try_into().unwrap()) as usize;
            let pieces = (length - 4) / 12;
            assert!(pieces > 0);
            mutation(table, position + 5, pieces);
        })
    };
    let reversed = mutate_positions(|table, start, _| {
        table[start..start + 4].copy_from_slice(&u32::MAX.to_le_bytes())
    });
    rejected(&reversed);
    let excessive = mutate_positions(|table, start, pieces| {
        table[start + pieces * 4..start + (pieces + 1) * 4]
            .copy_from_slice(&(CHARACTER_LIMIT + 1).to_le_bytes())
    });
    rejected(&excessive);
}

#[test]
fn relative_character_toggle_negates_the_paragraph_style_without_flattening_it() {
    const CHPX_PAIR: usize = 12;
    const FKP_BYTES: usize = 512;
    let mut document = Document::default();
    document
        .styles
        .styles
        .get_mut(&StyleId::from(STYLE_NORMAL))
        .unwrap()
        .character
        .bold = Some(true);
    let mut paragraph = Paragraph::plain("Relative toggle α");
    let Inline::Text(run) = &mut paragraph.content[0] else {
        panic!("text run")
    };
    run.properties.bold = Some(false);
    document.sections[0]
        .blocks
        .push(Block::Paragraph(paragraph));
    let mut output = Cursor::new(Vec::new());
    DocFormat.export(&document, &mut output).unwrap();
    let bytes = output.into_inner();
    let table_name = table_stream(&bytes);
    let mut compound = cfb::CompoundFile::open(Cursor::new(&bytes)).unwrap();
    let mut word = Vec::new();
    let mut table = Vec::new();
    compound
        .open_stream(WORD_STREAM)
        .unwrap()
        .read_to_end(&mut word)
        .unwrap();
    compound
        .open_stream(&table_name)
        .unwrap()
        .read_to_end(&mut table)
        .unwrap();
    let pair = FIB_PAIRS + CHPX_PAIR * FIB_PAIR_BYTES;
    let offset = u32::from_le_bytes(word[pair..pair + 4].try_into().unwrap()) as usize;
    let length = u32::from_le_bytes(word[pair + 4..pair + 8].try_into().unwrap()) as usize;
    let pages = (length - 4) / 8;
    assert!(pages > 0);
    let page = u32::from_le_bytes(
        table[offset + (pages + 1) * 4..offset + (pages + 2) * 4]
            .try_into()
            .unwrap(),
    ) as usize
        * FKP_BYTES;
    let runs = usize::from(word[page + FKP_BYTES - 1]);
    let properties = page + usize::from(word[page + (runs + 1) * 4]) * 2;
    let size = usize::from(word[properties]);
    let sprm = word[properties + 1..properties + 1 + size]
        .windows(3)
        .position(|value| value == [0x35, 0x08, 0x00])
        .expect("direct bold false");
    let toggled = mutate_stream(&bytes, WORD_STREAM, |word| {
        word[properties + 1 + sprm + 2] = 0x81
    });
    let actual = import(&toggled).expect("native relative style toggle");
    let paragraph = document_paragraphs(&actual)
        .into_iter()
        .find(|p| inline_text(&p.content) == "Relative toggle α")
        .unwrap();
    let Inline::Text(run) = &paragraph.content[0] else {
        panic!("text run")
    };
    assert_eq!(
        actual
            .styles
            .resolve_character(paragraph.style.as_ref())
            .bold,
        Some(true)
    );
    assert_eq!(run.properties.bold, Some(false));
    assert_eq!(
        actual
            .styles
            .resolve_run(
                paragraph.style.as_ref(),
                run.style.as_ref(),
                &run.properties
            )
            .bold,
        Some(false)
    );
}

#[test]
fn piece_character_overlays_keep_explicit_off_separate_from_style_inheritance() {
    let mut document = Document::default();
    let normal = document
        .styles
        .styles
        .get_mut(&StyleId::from(STYLE_NORMAL))
        .unwrap();
    normal.character.bold = Some(true);
    normal.character.italic = Some(true);
    document.sections[0]
        .blocks
        .push(Block::Paragraph(Paragraph::plain("AB")));
    let mut output = Cursor::new(Vec::new());
    DocFormat.export(&document, &mut output).unwrap();
    let source = output.into_inner();
    let stream = table_stream(&source);
    let (offset, length) = clx_range(&source);
    let mut compound = cfb::CompoundFile::open(Cursor::new(&source)).unwrap();
    let mut table = Vec::new();
    compound
        .open_stream(&stream)
        .unwrap()
        .read_to_end(&mut table)
        .unwrap();
    let original = &table[offset..offset + length];
    assert_eq!(original[0], 2);
    let piece_bytes = u32::from_le_bytes(original[1..5].try_into().unwrap()) as usize;
    assert_eq!(piece_bytes, 16);
    let end = u32::from_le_bytes(original[9..13].try_into().unwrap());
    let original_pcd = &original[13..21];
    for complex in [false, true] {
        let mut clx = Vec::new();
        if complex {
            clx.push(1);
            clx.extend_from_slice(&6u16.to_le_bytes());
            clx.extend_from_slice(&[0x35, 0x08, 0, 0x36, 0x08, 0]);
        }
        clx.push(2);
        clx.extend_from_slice(&28u32.to_le_bytes());
        for position in [0u32, 1, end] {
            clx.extend_from_slice(&position.to_le_bytes());
        }
        let mut first = original_pcd.to_vec();
        first[6..8].copy_from_slice(&(if complex { 1u16 } else { 0xAA }).to_le_bytes());
        clx.extend_from_slice(&first);
        let mut second = original_pcd.to_vec();
        let fc = u32::from_le_bytes(second[2..6].try_into().unwrap());
        second[2..6].copy_from_slice(&(fc + 2).to_le_bytes());
        second[6..8].fill(0);
        clx.extend_from_slice(&second);
        let new_offset = table.len();
        let patched = mutate_stream(&source, &stream, |table| table.extend_from_slice(&clx));
        let patched = mutate_stream(&patched, WORD_STREAM, |word| {
            let pair = FIB_PAIRS + CLX_PAIR * FIB_PAIR_BYTES;
            word[pair..pair + 4].copy_from_slice(&(new_offset as u32).to_le_bytes());
            word[pair + 4..pair + 8].copy_from_slice(&(clx.len() as u32).to_le_bytes());
        });
        let actual = import(&patched).expect("piece overlay on inherited style");
        let paragraph = document_paragraphs(&actual)
            .into_iter()
            .find(|p| inline_text(&p.content) == "AB")
            .unwrap();
        let text = paragraph
            .content
            .iter()
            .filter_map(|inline| {
                if let Inline::Text(run) = inline {
                    Some(run)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(text.len(), 2);
        assert_eq!(text[0].text, "A");
        assert_eq!(text[1].text, "B");
        assert_eq!(text[0].properties.bold, Some(false));
        assert_eq!(text[1].properties.bold, None);
        assert_eq!(
            actual
                .styles
                .resolve_run(
                    paragraph.style.as_ref(),
                    text[0].style.as_ref(),
                    &text[0].properties
                )
                .bold,
            Some(false)
        );
        assert_eq!(
            actual
                .styles
                .resolve_run(
                    paragraph.style.as_ref(),
                    text[1].style.as_ref(),
                    &text[1].properties
                )
                .bold,
            Some(true)
        );
        if complex {
            assert_eq!(text[0].properties.italic, Some(false));
            assert_eq!(text[1].properties.italic, None);
        }
    }
}

fn hyperlinks(items: &[Inline], out: &mut Vec<(String, String)>) {
    for item in items {
        if let Inline::Hyperlink(link) = item {
            out.push((link.target.clone(), inline_text(&link.content)));
            hyperlinks(&link.content, out);
        }
    }
}
#[test]
fn independent_binary_hyperlink_field_data_keeps_external_and_bookmark_targets() {
    let imported = import(INDEPENDENT_DOC).expect("independent hyperlinks");
    let mut native = Cursor::new(Vec::new());
    DocFormat.export(&imported, &mut native).unwrap();
    native.set_position(0);
    let reopened = DocFormat
        .import(&mut native)
        .expect("editable hyperlink roundtrip");
    for document in [&imported, &reopened] {
        let mut links = Vec::new();
        for paragraph in document_paragraphs(document) {
            hyperlinks(&paragraph.content, &mut links);
        }
        assert!(
            links.iter().any(
                |(target, label)| target == "https://example.invalid/path?a=1&b=2"
                    && label == "External & link"
            ),
            "external hyperlink missing: {links:?}"
        );
        assert!(
            links
                .iter()
                .any(|(target, label)| target == "#Topic" && label.contains("Ελληνικά")),
            "bookmark hyperlink missing: {links:?}"
        );
        assert!(
            !document.compatibility.iter().any(
                |issue| issue.message.contains("HYPERLINK") && issue.message.contains("cached")
            ),
            "recognized hyperlinks must not be diagnosed as cached fields"
        );
    }
}
