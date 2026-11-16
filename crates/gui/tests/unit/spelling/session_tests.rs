use super::*;
#[test]
fn correction_rebases_unicode_and_removes_overlaps() {
    let node = NodeId::new();
    let first = Issue {
        node: node.clone(),
        start: 2,
        end: 6,
        word: "αβ".into(),
        language: "el-GR".into(),
        suggestions: Vec::new(),
    };
    let mut issues = vec![
        Issue {
            node: node.clone(),
            start: 7,
            end: 11,
            word: "word".into(),
            language: "el-GR".into(),
            suggestions: Vec::new(),
        },
        Issue {
            node,
            start: 3,
            end: 6,
            word: "β".into(),
            language: "el-GR".into(),
            suggestions: Vec::new(),
        },
    ];
    adjust_after_replacement(&mut issues, &first, 2);
    assert_eq!(issues.len(), 1);
    assert_eq!((issues[0].start, issues[0].end), (5, 9));
}
