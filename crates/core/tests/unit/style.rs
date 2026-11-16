use super::*;

#[test]
fn direct_formatting_wins_over_style() {
    let styles = StyleSheet::default();
    let direct = CharacterProperties { bold: Some(false), ..Default::default() };
    let props = styles.resolve_run(Some(&StyleId::from(STYLE_HEADING_1)), None, &direct);
    assert_eq!(props.bold, Some(false));
}
