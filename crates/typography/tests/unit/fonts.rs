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

#[test]
fn missing_office_fonts_use_installed_substitutes_but_original_faces_win() {
    let mut db = Database::new();
    db.load_font_data(include_bytes!("../fixtures/neo-aegean/NeoAegean-Regular.otf").to_vec());
    let mut substitute = db.faces().next().unwrap().clone();
    db.remove_face(substitute.id);
    substitute.families = vec![("Carlito".into(), fontdb::Language::Unknown)];
    db.push_face_info(substitute);
    let catalog = FontCatalog { store: Arc::new(FontStore::new(db.clone())) };
    assert_eq!(catalog.resolve("Calibri", false, false), catalog.resolve("Carlito", false, false));
    assert_eq!(catalog.resolve_for_text("Calibri", false, false, "Latin text"), catalog.resolve("Carlito", false, false));

    db.load_font_data(include_bytes!("../fixtures/neo-aegean/NeoAegean-Italic.otf").to_vec());
    let alias = db.faces().find(|face| face.style == Style::Italic).unwrap().id;
    let mut store = FontStore::new(db.clone());
    store.embedded.insert(("calibri".into(), false, false), vec![ResolvedFont {
        id: alias, face_index: 0, catalog_id: store.identity,
    }]);
    let catalog = FontCatalog { store: Arc::new(store) };
    assert_eq!(catalog.resolve_for_text("Calibri", false, false, "Latin text"),
        catalog.resolve("Carlito", false, false), "a mislabeled embedded font must not replace the compatible font");

    let mut original = db.faces().find(|face| face.style == Style::Italic).unwrap().clone();
    db.remove_face(original.id);
    original.families = vec![("Calibri".into(), fontdb::Language::Unknown)];
    original.style = Style::Normal;
    db.push_face_info(original);
    let mut store = FontStore::new(db);
    let alias = store.db.faces().find(|face| face.families[0].0 == "Carlito").unwrap().id;
    store.embedded.insert(("calibri".into(), false, false), vec![ResolvedFont {
        id: alias, face_index: 0, catalog_id: store.identity,
    }]);
    let catalog = FontCatalog { store: Arc::new(store) };
    assert_ne!(catalog.resolve("Calibri", false, false), catalog.resolve("Carlito", false, false));
    assert!(catalog.resolve("Calibri", false, false).is_some());
}
