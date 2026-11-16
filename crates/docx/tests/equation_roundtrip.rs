use model::{Block, Document, Equation, EquationNode};
use format::DocumentFormat;
use docx::DocxFormat;
use std::io::Cursor;

#[test]
fn docx_round_trip_keeps_basic_native_math() {
    let mut document = Document::default();
    let equation = Equation::block(EquationNode::Fraction {
        numerator: Box::new(EquationNode::ident("x")),
        denominator: Box::new(EquationNode::Radical {
            radicand: Box::new(EquationNode::ident("y")),
            index: None,
        }),
    });
    document.sections[0].blocks.push(Block::Equation(equation));

    let format = DocxFormat;
    let mut bytes = Cursor::new(Vec::new());
    format.export(&document, &mut bytes).unwrap();
    bytes.set_position(0);
    let imported = format.import(&mut bytes).unwrap();

    assert!(matches!(imported.sections[0].blocks.first(), Some(Block::Equation(_))));
}
