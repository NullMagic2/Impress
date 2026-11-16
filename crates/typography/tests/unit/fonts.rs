use super::*;

#[test]
fn script_classifier_selects_arabic_chain() {
    assert!(script_fallback_families("مرحبا").iter().any(|name| name.contains("Arabic")));
}


#[test]
fn neo_aegean_typographic_and_style_linked_families_resolve_their_faces() {
    let mut db=Database::new();
    for bytes in [include_bytes!("../fixtures/neo-aegean/NeoAegean-Regular.otf").as_slice(),
        include_bytes!("../fixtures/neo-aegean/NeoAegean-Italic.otf").as_slice(),
        include_bytes!("../fixtures/neo-aegean/NeoAegean-CompactLight.otf").as_slice(),
        include_bytes!("../fixtures/neo-aegean/NeoAegean-CompactLightItalic.otf").as_slice(),
        include_bytes!("../fixtures/neo-aegean/NeoAegean-CompactBold.otf").as_slice(),
        include_bytes!("../fixtures/neo-aegean/NeoAegean-CompactBoldItalic.otf").as_slice()] {db.load_font_data(bytes.to_vec());}
    add_family_aliases(&mut db);let catalog=FontCatalog{store:Arc::new(FontStore::new(db))};
    for (family,bold,italic,expected) in [
        ("Neo Aegean",false,false,"NeoAegean-Regular"),
        ("Neo Aegean",false,true,"NeoAegean-Italic"),
        ("Neo Aegean",true,false,"NeoAegean-CompactBold"),
        ("Neo Aegean",true,true,"NeoAegean-CompactBoldItalic"),
        ("Neo Aegean Compact Light",false,false,"NeoAegean-CompactLight"),
        ("Neo Aegean Compact Light",false,true,"NeoAegean-CompactLightItalic"),
        ("Neo Aegean Compact",true,false,"NeoAegean-CompactBold"),
        ("Neo Aegean Compact",true,true,"NeoAegean-CompactBoldItalic")] {
        assert!(catalog.family_names().iter().any(|name|name==family));
        let font=catalog.resolve(family,bold,italic).unwrap();assert_eq!(catalog.database().face(font.id).unwrap().post_script_name,expected);
        assert!(catalog.font_covers_text(font,"ἄνθρωπος Greek Latin ABC"));
        let shaped=catalog.shape(font,"ἄνθρωπος",&crate::ShapeOptions::default()).unwrap().unwrap();
        assert!(shaped.advance_pt>0.0);assert!(shaped.glyphs.iter().all(|g|g.glyph_id!=0));
    }
}

#[test]
fn resolved_font_ids_cannot_alias_a_different_catalog() {
    let a=FontCatalog::from_font_data(include_bytes!("../fixtures/neo-aegean/NeoAegean-Regular.otf").to_vec());
    let b=FontCatalog::from_font_data(include_bytes!("../fixtures/neo-aegean/NeoAegean-Italic.otf").to_vec());
    let fa=a.resolve("Neo Aegean",false,false).unwrap();let fb=b.resolve("Neo Aegean",false,true).unwrap();
    assert_eq!(fa.id,fb.id,"Fixture deliberately reuses a database-local ID");assert_ne!(fa,fb);
    assert!(a.with_font_data(fb,|_,_|()).is_none());assert!(b.with_font_data(fa,|_,_|()).is_none());
}
