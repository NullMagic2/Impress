use super::*;
fn entries(node:&NodeId)->Vec<navigation::Entry>{[0,10,20].into_iter().map(|start|navigation::Entry{label:String::new(),node:node.clone(),start,end:start+3}).collect()}
#[test]fn next_uses_caret_after_replacement(){let id=NodeId::new();let e=entries(&id);let order=[(id.clone(),0)].into_iter().collect();let s=Selection::collapsed(DocumentPosition::new(id,13));assert_eq!(step_index(&e,None,Some(&s),true,&order),Some(2));}
