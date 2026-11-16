use super::*;
#[test]fn all_small_diffs_reconstruct_target(){for a in ["","α","same same","😀 e\u{301}","ababa"]{for b in ["","β","same changed","😀 é","babab"]{let mut result=a.to_string();for h in text_changes(a,b).iter().rev(){result.replace_range(h.old.clone(),&b[h.new.clone()]);}assert_eq!(result,b);}}}
