use crate::{Document,Paragraph,Block,Inline};
#[test]fn snapshots_share_bodies_and_unchanged_inline_arrays(){
    let mut a=Document::default();a.sections[0].blocks.push(Block::Paragraph(Paragraph::plain("a".repeat(100_000))));
    a.sections[0].blocks.push(Block::Paragraph(Paragraph::plain("b".repeat(100_000))));
    let frozen=a.clone();assert!(a.sections[0].blocks.shares_storage(&frozen.sections[0].blocks));
    if let Block::Paragraph(p)=&mut a.sections[0].blocks[0]{if let Inline::Text(t)=&mut p.content[0]{t.text.push('x');}}
    assert!(!a.sections[0].blocks.shares_storage(&frozen.sections[0].blocks));
    if let (Block::Paragraph(p),Block::Paragraph(q))=(&a.sections[0].blocks[1],&frozen.sections[0].blocks[1]){assert!(p.content.shares_storage(&q.content));}else{panic!();}
    if let Block::Paragraph(p)=&frozen.sections[0].blocks[0]{if let Inline::Text(t)=&p.content[0]{assert_eq!(t.text.len(),100_000);}}
}
