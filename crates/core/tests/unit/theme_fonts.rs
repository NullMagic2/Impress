//! Regional shaping choices stay editable through sparse style inheritance.
use super::*;
fn regional() -> CharacterProperties {
    CharacterProperties {
        font_family: Some("Latin fallback".into()),
        font_slots: Some(FontSlots {
            ascii: Some(FontSlot::explicit("ASCII")),
            h_ansi: Some(FontSlot::explicit("Greek")),
            east_asia: Some(FontSlot {
                scripts: [
                    ("Jpan", "Japanese"),
                    ("Hans", "Chinese"),
                    ("Hant", "Traditional"),
                    ("Kore", "Korean"),
                ]
                .into_iter()
                .map(|(a, b)| (a.into(), b.into()))
                .collect(),
                fallback_family: Some("CJK fallback".into()),
                ..Default::default()
            }),
            complex_script: Some(FontSlot::explicit("Arabic")),
            ..Default::default()
        }),
        ..Default::default()
    }
}
#[test]
fn shaping_spans_keep_graphemes_and_choose_regional_fonts() {
    let properties = regional();
    let text = "Áα中文한";
    let spans = properties.font_spans(text);
    assert_eq!(
        spans
            .iter()
            .map(|s| (&text[s.range.clone()], s.family))
            .collect::<Vec<_>>(),
        vec![
            ("Á", Some("ASCII")),
            ("α", Some("Greek")),
            ("中文", Some("Chinese")),
            ("한", Some("Korean"))
        ]
    );
    let mut japanese = properties.clone();
    japanese.language = Some("ja-JP".into());
    assert_eq!(japanese.font_family_for('中'), Some("Japanese"));
    japanese.font_slots.as_mut().unwrap().east_asia_language = Some("zh-TW".into());
    assert_eq!(japanese.font_family_for('中'), Some("Traditional"));
    japanese.right_to_left = Some(true);
    assert_eq!(japanese.font_family_for('A'), Some("Arabic"));
}
#[test]
fn regional_overrides_are_sparse_and_manual_font_choices_replace_the_theme() {
    let mut base = regional();
    base.overlay(&CharacterProperties {
        font_slots: Some(FontSlots {
            h_ansi: Some(FontSlot::explicit("Override")),
            ..Default::default()
        }),
        ..Default::default()
    });
    assert_eq!(base.font_family_for('a'), Some("ASCII"));
    assert_eq!(base.font_family_for('α'), Some("Override"));
    base.overlay(&CharacterProperties {
        font_family: Some("Manual".into()),
        ..Default::default()
    });
    assert!(base.font_slots.is_none());
    assert_eq!(base.font_family_for('中'), Some("Manual"));
    base = regional();
    base.set_font_family(Some("Selected".into()));
    assert_eq!(base.font_family_for('α'), Some("Selected"));
    assert!(base.font_slots.is_none());
}
#[test]
fn serialized_old_properties_remain_valid_and_regional_slots_roundtrip() {
    let old: CharacterProperties = serde_json::from_str("{\"font_family\":\"Old\"}").unwrap();
    assert!(old.font_slots.is_none());
    assert_eq!(old.font_family_for('α'), Some("Old"));
    let properties = regional();
    let encoded = serde_json::to_string(&properties).unwrap();
    assert_eq!(
        serde_json::from_str::<CharacterProperties>(&encoded).unwrap(),
        properties
    );
}
