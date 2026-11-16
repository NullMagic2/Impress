use model::{Block, Document, Equation, EquationNode, Paragraph};
use format::DocumentFormat;
use format_native::NativeImpressFormat;
use std::io::Cursor;

#[test]
fn native_round_trip_preserves_equation_ast() {
    let mut document = Document::default();
    document.sections[0].blocks.push(Block::Paragraph(Paragraph::plain("Euler wrote:")));
    document.sections[0].blocks.push(Block::Equation(Equation::block(EquationNode::row([
        EquationNode::ident("e"),
        EquationNode::Superscript { base: Box::new(EquationNode::ident("i")), exponent: Box::new(EquationNode::ident("pi")) },
        EquationNode::op("+"), EquationNode::number("1"), EquationNode::op("="), EquationNode::number("0")
    ]))));

    let format = NativeImpressFormat;
    let mut bytes = Cursor::new(Vec::new());
    format.export(&document, &mut bytes).unwrap();
    bytes.set_position(0);
    let loaded = format.import(&mut bytes).unwrap();
    assert_eq!(loaded, document);
}

#[test]
fn embedded_font_payload_and_alias_roundtrip() {
    let mut document=Document::default();let id=model::ResourceId::new();
    document.resources.insert(id.clone(),model::Resource{id:id.clone(),media_type:typography::EMBEDDED_FONT_MEDIA_TYPE.into(),original_name:None,
        font_faces:vec![model::EmbeddedFontFace{family:"Unavailable Test Family".into(),bold:true,italic:true,face_index:0}],data:typst_assets::fonts().next().unwrap().to_vec().into()});
    let mut bytes=Cursor::new(Vec::new());NativeImpressFormat.export(&document,&mut bytes).unwrap();bytes.set_position(0);
    let loaded=NativeImpressFormat.import(&mut bytes).unwrap();assert_eq!(loaded,document);
    let fonts=typography::FontCatalog::for_resources(&loaded.resources);let resolved=fonts.resolve("Unavailable Test Family",true,true).unwrap();
    assert_eq!(fonts.with_font_data(resolved,|data,_|data.to_vec()).unwrap(),*document.resources[&id].data);
}
