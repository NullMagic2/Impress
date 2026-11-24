use super::*;
fn props()->CharacterProperties {
    normalized_shaping_properties(CharacterProperties{font_family:Some(ligature_fixture::FAMILY.into()),..Default::default()})
}
fn fonts()->FontCatalog {FontCatalog::from_font_data(ligature_fixture::font())}
#[test] fn ff_and_ffl_keep_all_source_caret_positions() {
    let fonts = fonts(); let props = props();
    for text in ["ff", "ffl"] {
        let shaped = shape_token(text, &props, &fonts).unwrap(); assert_eq!(shaped.glyphs.len(), 1);
        let (_, hit) = vectorize_text_token(text, 0, text.len(), "p", 0, false, &props, &shaped,
            0.0, 1.0, shaped.advance_pt, 14.0, &fonts).unwrap();
        assert_eq!(hit.caret_stops.iter().map(|s| s.byte_offset).collect::<Vec<_>>(), (0..=text.len()).collect::<Vec<_>>());
    }
}

#[test]
fn unicode_quotes_and_accents_have_individual_carets_and_selection_rectangles() {
    let fonts = FontCatalog::from_font_data(include_bytes!("../../../../typography/tests/fixtures/neo-aegean/NeoAegean-Regular.otf").to_vec());
    let props = normalized_shaping_properties(CharacterProperties {
        font_family: Some(fonts.family_names()[0].clone()), ..Default::default()
    });
    for text in ["όλα\"\"\"\"\"\"", "α\u{301}ι\u{308}\u{301}é\"‘’«»", "ffi\"\""] {
        let shaped = shape_token(text, &props, &fonts).unwrap();
        let source_start = 57; // A run after multibyte text in the paragraph.
        let (_, hit) = vectorize_text_token(text, source_start, source_start + text.len(), "p", 0, false,
            &props, &shaped, 20.0, 1.0, shaped.advance_pt, 14.0, &fonts).unwrap();
        let expected: Vec<_> = text.grapheme_indices(true).map(|(i, _)| source_start + i)
            .chain(std::iter::once(source_start + text.len())).collect();
        assert_eq!(hit.caret_stops.iter().map(|s| s.byte_offset).collect::<Vec<_>>(), expected);
        for pair in hit.caret_stops.windows(2) {
            let point = render::Point::new(pair[0].x_pt + (pair[1].x_pt - pair[0].x_pt) * 0.75, -5.0);
            assert_eq!(hit.character_byte_offset(point), pair[0].byte_offset);
            assert_eq!(hit.nearest_byte_offset(point), pair[1].byte_offset);
            let selected = render::text_range_rect(0, &hit, pair[0].byte_offset..pair[1].byte_offset).unwrap();
            assert!((selected.rect.width - (pair[1].x_pt - pair[0].x_pt).abs()).abs() < 0.01);
        }
    }
}
