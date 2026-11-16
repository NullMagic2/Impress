use model::*;
use format_rtf::{decode,encode};
fn text(d:&Document)->String{document_paragraphs(d).iter().map(|p|inline_text(&p.content)).collect::<Vec<_>>().join("\n")}
#[test]fn unicode_greek_portuguese_emoji_and_braces(){let mut d=Document::default();d.sections[0].blocks.push(Block::Paragraph(Paragraph::plain("Ελληνικά ação 😀 {x} \\")));let bytes=encode(&d).unwrap();let r=decode(&bytes).unwrap();assert_eq!(text(&r),text(&d));}
#[test]fn object_payload_is_not_executed(){let d=decode(br"{\rtf1 visible{\object{\objdata 4142}}\par}").unwrap();assert_eq!(text(&d),"visible");assert!(!d.compatibility.is_empty());}
#[test]fn malformed_input_is_rejected(){for s in [b"{\\rtf1 broken".as_slice(),b"{\\rtf1\\'zz}",b"{\\rtf1\\bin10 x}",b"{\\rtf1}trailing"]{assert!(decode(s).is_err());}}

