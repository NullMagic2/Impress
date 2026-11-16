use super::*;
#[test]fn typst_source_builds_structures(){
    assert!(matches!(parse("a/b").unwrap(),N::Fraction{..}));
    let n=parse("frac(α+β, sqrt(2))").unwrap();assert!(matches!(n,N::Fraction{..}));assert_eq!(parse(&format(&n)).unwrap(),n);
    assert!(matches!(parse("integral 4").unwrap(),N::Nary{..}));
}
#[test]fn semantic_json_cannot_inject_raw_typst(){let node=N::RawTypst{source:"#read(\"private\")".into()};let json=format!("@ast {}",serde_json::to_string(&node).unwrap());assert!(parse(&json).is_err());}
