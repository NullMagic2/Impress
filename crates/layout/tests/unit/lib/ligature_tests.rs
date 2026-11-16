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
