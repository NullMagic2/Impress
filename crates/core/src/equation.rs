use crate::NodeId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Equation {
    pub id: NodeId,
    pub display: EquationDisplay,
    pub root: EquationNode,
    /// Optional source as entered by a user in code view. The semantic AST remains
    /// the preferred editable representation; RawTypst is used when source cannot
    /// yet be represented structurally.
    pub source_hint: Option<String>,
    pub alt_text: Option<String>,
}

impl Equation {
    pub fn inline(root: EquationNode) -> Self {
        Self { id: NodeId::new(), display: EquationDisplay::Inline, root, source_hint: None, alt_text: None }
    }

    pub fn block(root: EquationNode) -> Self {
        Self { id: NodeId::new(), display: EquationDisplay::Block, root, source_hint: None, alt_text: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquationDisplay { Inline, Block }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EquationNode {
    Row { children: Vec<EquationNode> },
    Identifier { value: String },
    Number { value: String },
    Text { value: String },
    Symbol { value: String },
    Operator { value: String },
    Relation { value: String },
    Fraction { numerator: Box<EquationNode>, denominator: Box<EquationNode> },
    Radical { radicand: Box<EquationNode>, index: Option<Box<EquationNode>> },
    Superscript { base: Box<EquationNode>, exponent: Box<EquationNode> },
    Subscript { base: Box<EquationNode>, subscript: Box<EquationNode> },
    Scripts { base: Box<EquationNode>, subscript: Option<Box<EquationNode>>, superscript: Option<Box<EquationNode>> },
    Matrix { rows: Vec<Vec<EquationNode>> },
    Delimited { left: String, body: Box<EquationNode>, right: String },
    Function { name: String, argument: Box<EquationNode> },
    Accent { accent: String, body: Box<EquationNode> },
    Nary {
        operator: String,
        body: Box<EquationNode>,
        subscript: Option<Box<EquationNode>>,
        superscript: Option<Box<EquationNode>>,
    },
    Limit { base: Box<EquationNode>, limit: Box<EquationNode>, position: LimitPosition },
    PreScripts {
        base: Box<EquationNode>,
        subscript: Option<Box<EquationNode>>,
        superscript: Option<Box<EquationNode>>,
    },
    Bar { body: Box<EquationNode>, position: VerticalPosition },
    GroupCharacter { body: Box<EquationNode>, character: String, position: VerticalPosition },
    EquationArray { rows: Vec<EquationNode> },
    Boxed { body: Box<EquationNode> },
    Phantom { body: Box<EquationNode> },
    Space { em: f32 },
    RawTypst { source: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitPosition { Lower, Upper }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerticalPosition { Over, Under }

impl EquationNode {
    pub fn row(children: impl IntoIterator<Item = EquationNode>) -> Self {
        Self::Row { children: children.into_iter().collect() }
    }

    pub fn ident(value: impl Into<String>) -> Self { Self::Identifier { value: value.into() } }
    pub fn number(value: impl Into<String>) -> Self { Self::Number { value: value.into() } }
    pub fn op(value: impl Into<String>) -> Self { Self::Operator { value: value.into() } }
}

/// What the equation editor leaves in a slot not filled in yet.
pub const EQUATION_PLACEHOLDER: &str = "\u{25A1}";

impl EquationNode {
    /// Whether the formula holds anything besides empty slots: an equation
    /// whose boxes were never filled in is no content (a blank document with
    /// one is still blank, for document recovery). An operator or function
    /// name counts, as it carries meaning of its own.
    pub fn has_content(&self) -> bool {
        use EquationNode::*;
        let any = |nodes: &[&EquationNode]| nodes.iter().any(|n| n.has_content());
        let opt = |n: &Option<Box<EquationNode>>| n.as_deref().is_some_and(EquationNode::has_content);
        match self {
            Row { children } => children.iter().any(EquationNode::has_content),
            Symbol { value } => value != EQUATION_PLACEHOLDER && !value.trim().is_empty(),
            Identifier { value } | Number { value } | Text { value } | Operator { value } | Relation { value } => !value.trim().is_empty(),
            Fraction { numerator, denominator } => any(&[numerator, denominator]),
            Radical { radicand, index } => radicand.has_content() || opt(index),
            Superscript { base, exponent } => any(&[base, exponent]),
            Subscript { base, subscript } => any(&[base, subscript]),
            Scripts { base, subscript, superscript } | PreScripts { base, subscript, superscript } => base.has_content() || opt(subscript) || opt(superscript),
            Matrix { rows } => rows.iter().flatten().any(EquationNode::has_content),
            Delimited { body, .. } | Accent { body, .. } | Bar { body, .. } | GroupCharacter { body, .. } | Boxed { body } | Phantom { body } => body.has_content(),
            Function { .. } | Nary { .. } => true,
            Limit { base, limit, .. } => any(&[base, limit]),
            EquationArray { rows } => rows.iter().any(EquationNode::has_content),
            Space { .. } => false,
            RawTypst { source } => !source.trim().is_empty(),
        }
    }
}
