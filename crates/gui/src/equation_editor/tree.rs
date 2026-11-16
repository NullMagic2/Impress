//! Structural equation editing: rows of atoms and nodes whose slots are rows.
//!
//! The model is what the visual editor manipulates. It converts losslessly
//! enough to and from the document's `EquationNode`: constructs the visual
//! editor does not edit are kept whole as opaque items.
use model::EquationNode as N;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomKind { Ident, Number, Operator, Relation, Symbol, Text }

#[derive(Clone, Debug, PartialEq)]
pub struct Atom { pub kind: AtomKind, pub text: String }

impl Atom {
    pub fn new(kind: AtomKind, text: impl Into<String>) -> Self { Self { kind, text: text.into() } }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Slots: numerator, denominator.
    Fraction,
    /// Slot: radicand.
    Sqrt,
    /// Slots: index, radicand.
    Root,
    /// Scripts attach to the item before them. Slot: exponent.
    Sup,
    /// Slot: subscript.
    Sub,
    /// Slots: subscript, superscript.
    SubSup,
    /// Invisible grouping, used for multi-item script bases.
    Group,
    Delimited { left: String, right: String },
    /// Slot: argument, drawn in parentheses.
    Function(String),
    /// Large operator (`sum`, `integral`, …). Slots: lower, upper, body.
    Nary(String),
    /// Slot: body.
    Accent(String),
    /// Row-major cells.
    Matrix { cols: usize },
    Boxed,
    /// Upright text; its slot holds text atoms.
    Text,
}

impl Kind {
    pub fn slot_count(&self) -> usize {
        match self {
            Kind::Fraction | Kind::Root | Kind::SubSup => 2,
            Kind::Nary(_) => 3,
            Kind::Matrix { cols } => cols * 2,
            _ => 1,
        }
    }
    /// Slots that may stay empty in a finished equation.
    fn optional(&self, slot: usize) -> bool { matches!(self, Kind::Nary(_)) && slot < 2 }
    /// The slot a selection moves into when the node wraps it.
    fn primary_slot(&self) -> usize {
        match self { Kind::Root => 1, Kind::Nary(_) => 2, _ => 0 }
    }
    /// Backspace at the start of these removes the node and keeps its content.
    fn unwraps(&self) -> bool {
        matches!(self, Kind::Group | Kind::Delimited { .. } | Kind::Function(_) | Kind::Accent(_)
            | Kind::Boxed | Kind::Text | Kind::Sqrt)
    }
    fn is_script(&self) -> bool { matches!(self, Kind::Sup | Kind::Sub | Kind::SubSup) }
    /// Slot reached by Up (`up`) or Down from `slot`, when the node has one.
    fn vertical(&self, slots: usize, slot: usize, up: bool) -> Option<usize> {
        match (self, slot, up) {
            (Kind::Fraction, 1, true) | (Kind::Root, 1, true) => Some(0),
            (Kind::Fraction, 0, false) | (Kind::Root, 0, false) => Some(1),
            (Kind::SubSup, 0, true) => Some(1),
            (Kind::SubSup, 1, false) => Some(0),
            (Kind::Nary(_), 0 | 2, true) => Some(1),
            (Kind::Nary(_), 1 | 2, false) => Some(0),
            (Kind::Matrix { cols }, s, true) if s >= *cols => Some(s - cols),
            (Kind::Matrix { cols }, s, false) if s + cols < slots => Some(s + cols),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node { pub kind: Kind, pub slots: Vec<Row> }

impl Node {
    pub fn new(kind: Kind) -> Self { let slots = vec![Vec::new(); kind.slot_count()]; Self { kind, slots } }
    fn is_empty(&self) -> bool { self.slots.iter().all(Vec::is_empty) }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Atom(Atom),
    Node(Node),
    /// An imported construct the visual editor keeps whole.
    Opaque(N),
    /// A `\name` command being typed.
    Command(String),
}

pub type Row = Vec<Item>;
/// Steps from the root row: (item index of a node, slot index).
pub type Path = Vec<(usize, usize)>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub path: Path,
    pub offset: usize,
    /// Selection anchor within the same row.
    pub anchor: Option<usize>,
}

/// Names that type as one upright operator, like Typst's built-ins.
pub const FUNCTIONS: &[&str] = &["arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "coth", "sin", "cos",
    "tan", "cot", "sec", "csc", "log", "ln", "lg", "exp", "lim", "max", "min", "det", "gcd", "deg", "arg",
    "dim", "ker", "sup", "inf", "mod"];

pub const EMPTY_SLOT: &str = "Fill in the dashed boxes to finish the equation.";

/// What an empty slot shows on the page while the equation is being typed.
pub const PLACEHOLDER: &str = model::EQUATION_PLACEHOLDER;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Editor { pub root: Row, pub cursor: Cursor }

impl Editor {
    pub fn from_node(node: &N) -> Self {
        let root = row_from(node);
        let offset = root.len();
        Self { root, cursor: Cursor { path: Vec::new(), offset, anchor: None } }
    }

    pub fn to_node(&self) -> Result<N, String> { row_node(&self.root, true, false) }

    /// The equation as it stands while it is being typed, for the page to
    /// follow every keystroke: empty slots are placeholder boxes and a `\name`
    /// being typed shows as its text. Fails only when there is nothing yet.
    pub fn to_node_live(&self) -> Result<N, String> {
        if self.root.is_empty() { return Err(EMPTY_SLOT.into()); }
        row_node(&self.root, true, true)
    }

    pub fn row(&self, path: &[(usize, usize)]) -> &Row {
        let mut row = &self.root;
        for &(index, slot) in path {
            let Some(Item::Node(node)) = row.get(index) else { return row; };
            row = &node.slots[slot];
        }
        row
    }

    fn row_mut(&mut self, path: &[(usize, usize)]) -> &mut Row {
        let mut row = &mut self.root;
        for &(index, slot) in path {
            row = match &mut row[index] { Item::Node(node) => &mut node.slots[slot], _ => unreachable!("path to a node") };
        }
        row
    }

    fn node_at(&self, path: &[(usize, usize)]) -> Option<&Node> {
        let (&(index, _), parent) = path.split_last()?;
        match self.row(parent).get(index) { Some(Item::Node(node)) => Some(node), _ => None }
    }

    fn node_at_mut(&mut self, path: &[(usize, usize)]) -> Option<&mut Node> {
        let (&(index, _), parent) = path.split_last()?;
        let parent = parent.to_vec();
        match self.row_mut(&parent).get_mut(index) { Some(Item::Node(node)) => Some(node), _ => None }
    }

    pub fn current_row(&self) -> &Row { self.row(&self.cursor.path) }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.cursor.anchor?;
        (anchor != self.cursor.offset).then(|| (anchor.min(self.cursor.offset), anchor.max(self.cursor.offset)))
    }

    fn in_text(&self) -> bool { self.node_at(&self.cursor.path).is_some_and(|n| n.kind == Kind::Text) }

    /// Innermost matrix around the cursor: its path and the cursor's cell.
    pub fn matrix(&self) -> Option<(Path, usize, usize)> {
        for depth in (1..=self.cursor.path.len()).rev() {
            let path = &self.cursor.path[..depth];
            if let Some(Node { kind: Kind::Matrix { cols }, .. }) = self.node_at(path) {
                return Some((path.to_vec(), path[depth - 1].1, *cols));
            }
        }
        None
    }

    pub fn has_command(&self) -> bool {
        self.cursor.offset > 0 && matches!(self.current_row().get(self.cursor.offset - 1), Some(Item::Command(_)))
    }

    /// The command being typed, if any.
    pub fn pending_command(&self) -> Option<&str> {
        match self.current_row().get(self.cursor.offset.checked_sub(1)?) { Some(Item::Command(s)) => Some(s), _ => None }
    }

    pub fn set_cursor(&mut self, path: Path, offset: usize) {
        self.resolve_command();
        let len = self.row(&path).len();
        self.cursor = Cursor { path, offset: offset.min(len), anchor: None };
    }

    /// End slot editing without changing its content. Later palette input
    /// belongs after this structure, one level out, until a slot is reselected.
    pub fn deactivate_slot(&mut self) {
        self.leave_node(true);
        self.cursor.anchor = None;
    }

    pub fn select_all(&mut self) {
        self.settle();
        self.cursor = Cursor { path: Vec::new(), offset: self.root.len(), anchor: Some(0) };
    }

    /// Selects `start..end` of the row at `path`.
    pub fn select(&mut self, path: Path, start: usize, end: usize) {
        self.resolve_command();
        let len = self.row(&path).len();
        self.cursor = Cursor { path, offset: end.min(len), anchor: Some(start.min(len)) };
    }

    fn take_selection(&mut self) -> Option<Row> {
        let (start, end) = self.selection()?;
        let path = self.cursor.path.clone();
        let taken = self.row_mut(&path).drain(start..end).collect();
        self.cursor.offset = start;
        self.cursor.anchor = None;
        Some(taken)
    }

    pub fn delete_selection(&mut self) -> bool {
        let deleted = self.take_selection().is_some();
        self.cursor.anchor = None;
        deleted
    }

    fn insert_item(&mut self, item: Item) {
        self.delete_selection();
        let (path, offset) = (self.cursor.path.clone(), self.cursor.offset);
        self.row_mut(&path).insert(offset, item);
        self.cursor.offset += 1;
    }

    fn enter(&mut self, index: usize, slot: usize, at_end: bool) {
        self.cursor.path.push((index, slot));
        self.cursor.offset = if at_end { self.current_row().len() } else { 0 };
        self.cursor.anchor = None;
    }

    /// Puts the cursor in the first empty slot of the node at `index`, or
    /// after the node when every slot is filled.
    fn enter_first_empty(&mut self, index: usize) {
        let row = self.current_row();
        let Some(Item::Node(node)) = row.get(index) else { return; };
        match node.slots.iter().position(Vec::is_empty) {
            Some(slot) => self.enter(index, slot, false),
            None => self.cursor.offset = index + 1,
        }
    }

    /// Resolves a pending command and turns trailing letters that spell a
    /// function name into that name.
    fn settle(&mut self) {
        self.resolve_command();
        self.finish_function();
    }

    fn finish_function(&mut self) {
        if self.cursor.anchor.is_some() { return; }
        let (start, letters) = self.trailing_letters();
        let end = self.cursor.offset;
        let Some(name) = FUNCTIONS.iter().filter(|f| letters.ends_with(**f)).max_by_key(|f| f.len()) else { return; };
        let _ = start;
        let from = end - name.len();
        let path = self.cursor.path.clone();
        self.row_mut(&path).splice(from..end, [Item::Atom(Atom::new(AtomKind::Ident, *name))]);
        self.cursor.offset = from + 1;
    }

    fn trailing_letters(&self) -> (usize, String) {
        let row = self.current_row();
        let end = self.cursor.offset.min(row.len());
        let mut start = end;
        while start > 0 && matches!(&row[start - 1], Item::Atom(a) if a.kind == AtomKind::Ident
            && a.text.chars().count() == 1 && a.text.chars().all(|c| c.is_ascii_alphabetic())) { start -= 1; }
        (start, row[start..end].iter().map(|i| match i { Item::Atom(a) => a.text.as_str(), _ => "" }).collect())
    }

    pub fn insert_atom(&mut self, atom: Atom) {
        self.resolve_command();
        if atom.kind != AtomKind::Ident || !atom.text.chars().all(|c| c.is_ascii_alphabetic()) { self.finish_function(); }
        let atom = if self.in_text() { Atom::new(AtomKind::Text, atom.text) } else { atom };
        self.insert_item(Item::Atom(atom));
    }

    /// Inserts a structure. A selection becomes its primary content.
    pub fn insert_node(&mut self, kind: Kind) {
        self.settle();
        if kind.is_script() {
            // A selection becomes the base.
            if let Some(selected) = self.take_selection() {
                let base = if selected.len() == 1 { selected.into_iter().next().expect("one item") }
                    else { Item::Node(Node { kind: Kind::Group, slots: vec![selected] }) };
                self.insert_item(base);
            }
            match kind {
                Kind::Sup => self.script(true),
                Kind::Sub => self.script(false),
                _ => {
                    self.insert_item(Item::Node(Node::new(Kind::SubSup)));
                    let index = self.cursor.offset - 1;
                    self.enter(index, 0, false);
                }
            }
            return;
        }
        let mut node = Node::new(kind);
        if let Some(selected) = self.take_selection() {
            let slot = node.kind.primary_slot();
            node.slots[slot] = selected;
        }
        self.insert_item(Item::Node(node));
        let index = self.cursor.offset - 1;
        self.cursor.offset = index;
        self.enter_first_empty(index);
    }

    /// Handles a typed character, including the typing shortcuts.
    pub fn type_char(&mut self, c: char) {
        if self.has_command() {
            if c.is_ascii_alphabetic() {
                let offset = self.cursor.offset;
                let path = self.cursor.path.clone();
                if let Some(Item::Command(name)) = self.row_mut(&path).get_mut(offset - 1) { name.push(c); }
                return;
            }
            self.resolve_command();
            if c == ' ' || c == '\t' { return; }
        }
        if !c.is_alphabetic() && !self.in_text() { self.finish_function(); }
        if self.in_text() {
            if c == '"' { self.leave_node(true); }
            else { self.insert_item(Item::Atom(Atom::new(AtomKind::Text, c.to_string()))); }
            return;
        }
        match c {
            '\\' => { self.delete_selection(); self.insert_item(Item::Command(String::new())); }
            '/' => self.fraction(),
            '^' => self.script(true),
            '_' => self.script(false),
            '(' => self.insert_node(Kind::Delimited { left: "(".into(), right: ")".into() }),
            '[' => self.insert_node(Kind::Delimited { left: "[".into(), right: "]".into() }),
            '{' => self.insert_node(Kind::Delimited { left: "{".into(), right: "}".into() }),
            '|' if !self.close_delimiter("|") => self.insert_node(Kind::Delimited { left: "|".into(), right: "|".into() }),
            '|' => {}
            ')' | ']' | '}' => { if !self.close_delimiter(&c.to_string()) { self.insert_atom(Atom::new(AtomKind::Symbol, c.to_string())); } }
            '"' => self.insert_node(Kind::Text),
            ' ' => {
                let script = self.node_at(&self.cursor.path).is_some_and(|n| n.kind.is_script());
                if script && self.cursor.offset == self.current_row().len() { self.leave_node(true); }
            }
            '0'..='9' | '.' => self.insert_atom(Atom::new(AtomKind::Number, c.to_string())),
            '*' => self.insert_atom(Atom::new(AtomKind::Operator, "·")),
            '-' => self.insert_atom(Atom::new(AtomKind::Operator, "−")),
            '=' => self.combine(&[("<", "≤"), (">", "≥"), ("!", "≠"), ("=", "≡")], AtomKind::Relation, "="),
            '>' => self.combine(&[("−", "→"), ("=", "⇒"), ("<", "≠")], AtomKind::Relation, ">"),
            '<' => self.insert_atom(Atom::new(AtomKind::Relation, "<")),
            '+' | '±' | '×' | '÷' | '·' | '!' | ',' | ';' | ':' | '\'' => self.insert_atom(Atom::new(AtomKind::Operator, c.to_string())),
            c if c.is_alphabetic() => { self.insert_atom(Atom::new(AtomKind::Ident, c.to_string())); self.auto_function(); }
            c if c.is_control() => {}
            c => self.insert_atom(Atom::new(AtomKind::Symbol, c.to_string())),
        }
    }

    /// `<` then `=` becomes `≤`, and so on.
    fn combine(&mut self, pairs: &[(&str, &str)], kind: AtomKind, plain: &str) {
        if self.selection().is_none() && self.cursor.offset > 0 {
            let (path, offset) = (self.cursor.path.clone(), self.cursor.offset);
            if let Some(Item::Atom(previous)) = self.row_mut(&path).get_mut(offset - 1) {
                if let Some((_, merged)) = pairs.iter().find(|(first, _)| previous.text == *first) {
                    *previous = Atom::new(AtomKind::Relation, *merged);
                    return;
                }
            }
        }
        self.insert_atom(Atom::new(kind, plain));
    }

    /// Letters that spell a known function become one upright name.
    fn auto_function(&mut self) {
        let end = self.cursor.offset;
        let (_, letters) = self.trailing_letters();
        // Longest name that ends the letters and is not the start of a longer one being typed.
        let Some(name) = FUNCTIONS.iter().filter(|f| letters.ends_with(**f)).max_by_key(|f| f.len()) else { return; };
        if FUNCTIONS.iter().any(|f| f.len() > name.len() && f.starts_with(*name)) { return; }
        let from = end - name.len();
        let path = self.cursor.path.clone();
        let row = self.row_mut(&path);
        row.splice(from..end, [Item::Atom(Atom::new(AtomKind::Ident, *name))]);
        self.cursor.offset = from + 1;
    }

    fn close_delimiter(&mut self, right: &str) -> bool {
        let Some(node) = self.node_at(&self.cursor.path) else { return false; };
        let closes = matches!(&node.kind, Kind::Delimited { right: r, .. } if r == right);
        if closes && self.cursor.offset == self.current_row().len() { self.leave_node(true); true } else { false }
    }

    fn fraction(&mut self) {
        let numerator = match self.take_selection() {
            Some(selected) => selected,
            None => {
                let row = self.current_row();
                let end = self.cursor.offset;
                let mut start = end;
                while start > 0 && match &row[start - 1] {
                    Item::Atom(a) => matches!(a.kind, AtomKind::Ident | AtomKind::Number | AtomKind::Symbol),
                    Item::Node(_) | Item::Opaque(_) => true,
                    Item::Command(_) => false,
                } { start -= 1; }
                let path = self.cursor.path.clone();
                self.cursor.offset = start;
                self.row_mut(&path).drain(start..end).collect()
            }
        };
        let filled = !numerator.is_empty();
        let node = Node { kind: Kind::Fraction, slots: vec![numerator, Vec::new()] };
        self.insert_item(Item::Node(node));
        let index = self.cursor.offset - 1;
        self.enter(index, if filled { 1 } else { 0 }, false);
    }

    fn script(&mut self, sup: bool) {
        self.delete_selection();
        // `x_i^2`: the other script at the end of a script slot goes on the
        // same base, as in LaTeX.
        let at_end = self.cursor.offset == self.current_row().len() && !self.current_row().is_empty();
        let kind = self.node_at(&self.cursor.path).map(|n| n.kind.clone());
        if at_end && matches!((&kind, sup), (Some(Kind::Sub), true) | (Some(Kind::Sup), false)) {
            let path = self.cursor.path.clone();
            if let Some(node) = self.node_at_mut(&path) {
                node.kind = Kind::SubSup;
                if sup { node.slots.push(Vec::new()); } else { node.slots.insert(0, Vec::new()); }
            }
            let (index, _) = self.cursor.path.pop().expect("inside a script");
            self.enter(index, if sup { 1 } else { 0 }, false);
            return;
        }
        let offset = self.cursor.offset;
        let path = self.cursor.path.clone();
        if offset > 0 {
            if let Some(Item::Node(node)) = self.row_mut(&path).get_mut(offset - 1) {
                match (&node.kind, sup) {
                    (Kind::Sub, true) => { node.kind = Kind::SubSup; node.slots.push(Vec::new()); self.enter(offset - 1, 1, true); return; }
                    (Kind::Sup, false) => { node.kind = Kind::SubSup; node.slots.insert(0, Vec::new()); self.enter(offset - 1, 0, true); return; }
                    (Kind::Sup, true) => { self.enter(offset - 1, 0, true); return; }
                    (Kind::Sub, false) => { self.enter(offset - 1, 0, true); return; }
                    (Kind::SubSup, _) => { self.enter(offset - 1, if sup { 1 } else { 0 }, true); return; }
                    _ => {}
                }
            }
        }
        self.insert_item(Item::Node(Node::new(if sup { Kind::Sup } else { Kind::Sub })));
        let index = self.cursor.offset - 1;
        self.enter(index, 0, false);
    }

    /// Replaces a finished `\name` with what it names; unknown names stay
    /// as letters.
    pub fn resolve_command(&mut self) {
        let Some(name) = self.pending_command().map(str::to_owned) else { return; };
        let path = self.cursor.path.clone();
        let offset = self.cursor.offset;
        self.row_mut(&path).remove(offset - 1);
        self.cursor.offset -= 1;
        self.cursor.anchor = None;
        if name.is_empty() { return; }
        match super::palette::command(&name) {
            Some(super::palette::Insert::Atom(kind, text)) => self.insert_atom(Atom::new(kind, text)),
            Some(super::palette::Insert::Node(kind)) => self.insert_node(kind),
            None => for c in name.chars() { self.insert_atom(Atom::new(AtomKind::Ident, c.to_string())); },
        }
    }

    /// Moves the cursor out of its node, after it (`after`) or before it.
    fn leave_node(&mut self, after: bool) -> bool {
        let Some((index, _)) = self.cursor.path.pop() else { return false; };
        self.cursor.offset = if after { index + 1 } else { index };
        self.cursor.anchor = None;
        true
    }

    /// Enter the preceding structure all the way to its last content row.
    fn descend_backwards(&mut self) {
        loop {
            let Some(index) = self.cursor.offset.checked_sub(1) else { return; };
            let Some(Item::Node(node)) = self.current_row().get(index) else { return; };
            let Some(slot) = node.slots.iter().rposition(|row| !row.is_empty()) else { return; };
            self.enter(index, slot, true);
        }
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() { return; }
        let (path, offset) = (self.cursor.path.clone(), self.cursor.offset);
        let command = match offset.checked_sub(1).and_then(|o| self.row(&path).get(o)) {
            Some(Item::Command(name)) => Some(name.is_empty()),
            _ => None,
        };
        if let Some(empty) = command {
            if empty { self.row_mut(&path).remove(offset - 1); self.cursor.offset -= 1; }
            else if let Some(Item::Command(name)) = self.row_mut(&path).get_mut(offset - 1) { name.pop(); }
            return;
        }
        if offset > 0 {
            match &self.row(&path)[offset - 1] {
                Item::Node(node) if !node.is_empty() => {
                    self.descend_backwards();
                }
                _ => { self.row_mut(&path).remove(offset - 1); self.cursor.offset -= 1; }
            }
            return;
        }
        let Some(&(index, slot)) = path.last() else { return; };
        let node = self.node_at(&path).expect("cursor inside a node").clone();
        let parent = path[..path.len() - 1].to_vec();
        if node.is_empty() {
            self.row_mut(&parent).remove(index);
            self.cursor = Cursor { path: parent, offset: index, anchor: None };
        } else if slot > 0 {
            self.cursor.path.last_mut().expect("inside a node").1 = slot - 1;
            self.cursor.offset = node.slots[slot - 1].len();
            self.cursor.anchor = None;
            self.descend_backwards();
        } else if node.kind.unwraps() {
            let content = node.slots[0].clone();
            self.row_mut(&parent).splice(index..=index, content);
            self.cursor = Cursor { path: parent, offset: index, anchor: None };
        } else {
            self.cursor = Cursor { path: parent, offset: index, anchor: None };
        }
    }

    pub fn delete_forward(&mut self) {
        self.settle();
        if self.delete_selection() { return; }
        let (path, offset) = (self.cursor.path.clone(), self.cursor.offset);
        let len = self.row(&path).len();
        if offset < len {
            match &self.row(&path)[offset] {
                Item::Node(node) if !node.is_empty() => self.enter(offset, 0, false),
                _ => { self.row_mut(&path).remove(offset); }
            }
            return;
        }
        let Some(&(index, slot)) = path.last() else { return; };
        let node = self.node_at(&path).expect("cursor inside a node").clone();
        let parent = path[..path.len() - 1].to_vec();
        if slot + 1 < node.slots.len() {
            self.cursor.path.last_mut().expect("inside a node").1 = slot + 1;
            self.cursor.offset = 0;
        } else if node.is_empty() {
            self.row_mut(&parent).remove(index);
            self.cursor = Cursor { path: parent, offset: index, anchor: None };
        } else if node.kind.unwraps() {
            let content = node.slots[0].clone();
            let end = index + content.len();
            self.row_mut(&parent).splice(index..=index, content);
            self.cursor = Cursor { path: parent, offset: end, anchor: None };
        } else {
            self.cursor = Cursor { path: parent, offset: index + 1, anchor: None };
        }
    }

    pub fn move_right(&mut self, select: bool) {
        self.settle();
        let len = self.current_row().len();
        if select {
            let anchor = self.cursor.anchor.unwrap_or(self.cursor.offset);
            if self.cursor.offset < len { self.cursor.offset += 1; self.cursor.anchor = Some(anchor); }
            else if let Some((index, _)) = self.cursor.path.pop() { self.cursor.offset = index + 1; self.cursor.anchor = Some(index); }
            return;
        }
        if let Some((_, end)) = self.selection() { self.cursor.offset = end; self.cursor.anchor = None; return; }
        self.cursor.anchor = None;
        let offset = self.cursor.offset;
        if offset < len {
            if matches!(self.current_row()[offset], Item::Node(_)) { self.enter(offset, 0, false); }
            else { self.cursor.offset += 1; }
        } else if let Some(&(_, slot)) = self.cursor.path.last() {
            let slots = self.node_at(&self.cursor.path).map_or(1, |n| n.slots.len());
            if slot + 1 < slots { self.cursor.path.last_mut().expect("inside a node").1 = slot + 1; self.cursor.offset = 0; }
            else { self.leave_node(true); }
        }
    }

    pub fn move_left(&mut self, select: bool) {
        self.settle();
        if select {
            let anchor = self.cursor.anchor.unwrap_or(self.cursor.offset);
            if self.cursor.offset > 0 { self.cursor.offset -= 1; self.cursor.anchor = Some(anchor); }
            else if let Some((index, _)) = self.cursor.path.pop() { self.cursor.offset = index; self.cursor.anchor = Some(index + 1); }
            return;
        }
        if let Some((start, _)) = self.selection() { self.cursor.offset = start; self.cursor.anchor = None; return; }
        self.cursor.anchor = None;
        let offset = self.cursor.offset;
        if offset > 0 {
            if let Item::Node(node) = &self.current_row()[offset - 1] { let last = node.slots.len() - 1; self.enter(offset - 1, last, true); }
            else { self.cursor.offset -= 1; }
        } else if let Some(&(_, slot)) = self.cursor.path.last() {
            if slot > 0 {
                self.cursor.path.last_mut().expect("inside a node").1 = slot - 1;
                self.cursor.offset = self.current_row().len();
            } else { self.leave_node(false); }
        }
    }

    pub fn home(&mut self, select: bool) {
        self.settle();
        let anchor = self.cursor.anchor.unwrap_or(self.cursor.offset);
        self.cursor.offset = 0;
        self.cursor.anchor = select.then_some(anchor);
    }

    pub fn end(&mut self, select: bool) {
        self.settle();
        let anchor = self.cursor.anchor.unwrap_or(self.cursor.offset);
        self.cursor.offset = self.current_row().len();
        self.cursor.anchor = select.then_some(anchor);
    }

    /// Tab: the next slot of the current structure, then out of it.
    pub fn next_slot(&mut self, backwards: bool) {
        self.settle();
        self.cursor.anchor = None;
        let Some(&(_, slot)) = self.cursor.path.last() else {
            self.cursor.offset = if backwards { 0 } else { self.root.len() };
            return;
        };
        let slots = self.node_at(&self.cursor.path).map_or(1, |n| n.slots.len());
        if !backwards && slot + 1 < slots {
            self.cursor.path.last_mut().expect("inside a node").1 = slot + 1; self.cursor.offset = 0;
        } else if backwards && slot > 0 {
            self.cursor.path.last_mut().expect("inside a node").1 = slot - 1;
            self.cursor.offset = self.current_row().len();
        } else {
            self.leave_node(!backwards);
        }
    }

    /// Row reached by Up/Down: the nearest enclosing structure with a slot
    /// above or below. The view chooses the offset from the caret position.
    pub fn vertical_target(&self, up: bool) -> Option<Path> {
        for depth in (1..=self.cursor.path.len()).rev() {
            let path = &self.cursor.path[..depth];
            let node = self.node_at(path)?;
            if let Some(slot) = node.kind.vertical(node.slots.len(), path[depth - 1].1, up) {
                let mut target = path.to_vec();
                target[depth - 1].1 = slot;
                return Some(target);
            }
        }
        None
    }

    /// Adds or removes a matrix row or column next to the cursor's cell.
    pub fn matrix_edit(&mut self, add: bool, column: bool) {
        self.resolve_command();
        let Some((path, cell, cols)) = self.matrix() else { return; };
        let depth = path.len();
        let node = self.node_at_mut(&path).expect("matrix node");
        let rows = node.slots.len() / cols;
        let (row, col) = (cell / cols, cell % cols);
        let mut grid: Vec<Vec<Row>> = node.slots.chunks(cols).map(|r| r.to_vec()).collect();
        let (new_rows, new_cols, target) = match (add, column) {
            (true, false) if rows < 64 => { grid.insert(row + 1, vec![Vec::new(); cols]); (rows + 1, cols, (row + 1, 0)) }
            (true, true) if cols < 64 => { for r in &mut grid { r.insert(col + 1, Vec::new()); } (rows, cols + 1, (row, col + 1)) }
            (false, false) if rows > 1 => { grid.remove(row); (rows - 1, cols, (row.min(rows - 2), col)) }
            (false, true) if cols > 1 => { for r in &mut grid { r.remove(col); } (rows, cols - 1, (row, col.min(cols - 2))) }
            _ => return,
        };
        node.kind = Kind::Matrix { cols: new_cols };
        node.slots = grid.into_iter().flatten().collect();
        let _ = new_rows;
        self.cursor.path.truncate(depth);
        self.cursor.path[depth - 1].1 = target.0 * new_cols + target.1;
        self.cursor.offset = 0;
        self.cursor.anchor = None;
    }
}

// ---- Conversion to the document model ----------------------------------

/// `live` fills empty slots with placeholders instead of failing.
fn row_node(row: &Row, required: bool, live: bool) -> Result<N, String> {
    if row.is_empty() {
        return match (required, live) {
            (false, _) => Ok(N::Row { children: Vec::new() }),
            (true, true) => Ok(N::Symbol { value: PLACEHOLDER.into() }),
            (true, false) => Err(EMPTY_SLOT.into()),
        };
    }
    let mut out: Vec<N> = Vec::new();
    let mut number = String::new();
    for item in row {
        if let Item::Atom(Atom { kind: AtomKind::Number, text }) = item { number.push_str(text); continue; }
        if !number.is_empty() { out.push(N::Number { value: std::mem::take(&mut number) }); }
        let node = match item {
            Item::Atom(atom) => match atom.kind {
                AtomKind::Ident => N::Identifier { value: atom.text.clone() },
                AtomKind::Operator => N::Operator { value: atom.text.clone() },
                AtomKind::Relation => N::Relation { value: atom.text.clone() },
                AtomKind::Symbol => N::Symbol { value: atom.text.clone() },
                AtomKind::Text => N::Text { value: atom.text.clone() },
                AtomKind::Number => unreachable!("numbers are merged above"),
            },
            Item::Command(name) if live => N::Text { value: format!("\\{name}") },
            Item::Command(name) => return Err(format!("Finish \\{name} with Space or Enter.")),
            Item::Opaque(node) => node.clone(),
            Item::Node(node) => {
                let slot = |i: usize| row_node(&node.slots[i], !node.kind.optional(i), live);
                let optional = |i: usize| -> Result<Option<Box<N>>, String> {
                    Ok(if node.slots[i].is_empty() { None } else { Some(Box::new(slot(i)?)) })
                };
                match &node.kind {
                    Kind::Sup | Kind::Sub | Kind::SubSup => {
                        let base = Box::new(out.pop().unwrap_or(N::Row { children: Vec::new() }));
                        match node.kind {
                            Kind::Sup => N::Superscript { base, exponent: Box::new(slot(0)?) },
                            Kind::Sub => N::Subscript { base, subscript: Box::new(slot(0)?) },
                            _ => N::Scripts { base, subscript: Some(Box::new(slot(0)?)), superscript: Some(Box::new(slot(1)?)) },
                        }
                    }
                    Kind::Fraction => N::Fraction { numerator: Box::new(slot(0)?), denominator: Box::new(slot(1)?) },
                    Kind::Sqrt => N::Radical { radicand: Box::new(slot(0)?), index: None },
                    Kind::Root => N::Radical { radicand: Box::new(slot(1)?), index: Some(Box::new(slot(0)?)) },
                    Kind::Group => slot(0)?,
                    Kind::Delimited { left, right } => N::Delimited { left: left.clone(), body: Box::new(slot(0)?), right: right.clone() },
                    Kind::Function(name) => N::Function { name: name.clone(), argument: Box::new(slot(0)?) },
                    Kind::Nary(operator) => N::Nary { operator: operator.clone(), body: Box::new(slot(2)?), subscript: optional(0)?, superscript: optional(1)? },
                    Kind::Accent(accent) => N::Accent { accent: accent.clone(), body: Box::new(slot(0)?) },
                    Kind::Matrix { cols } => N::Matrix { rows: node.slots.chunks(*cols)
                        .map(|cells| cells.iter().map(|c| row_node(c, true, live)).collect::<Result<Vec<_>, _>>())
                        .collect::<Result<Vec<_>, _>>()? },
                    Kind::Boxed => N::Boxed { body: Box::new(slot(0)?) },
                    Kind::Text => {
                        if node.slots[0].is_empty() {
                            if !live { return Err(EMPTY_SLOT.into()); }
                            out.push(N::Symbol { value: PLACEHOLDER.into() });
                            continue;
                        }
                        N::Text { value: node.slots[0].iter().map(|i| match i { Item::Atom(a) => a.text.as_str(), _ => "" }).collect() }
                    }
                }
            }
        };
        out.push(node);
    }
    if !number.is_empty() { out.push(N::Number { value: number }); }
    Ok(if out.len() == 1 { out.pop().expect("one child") } else { N::Row { children: out } })
}

// ---- Conversion from the document model --------------------------------

pub fn row_from(node: &N) -> Row { let mut row = Vec::new(); push_items(node, &mut row); row }

fn node(kind: Kind, slots: Vec<Row>) -> Item { Item::Node(Node { kind, slots }) }

fn push_items(n: &N, out: &mut Row) {
    let atom = |kind, text: &String| Item::Atom(Atom::new(kind, text.clone()));
    match n {
        N::Row { children } => for child in children { push_items(child, out) },
        N::Identifier { value } => out.push(atom(AtomKind::Ident, value)),
        N::Number { value } => out.extend(value.chars().map(|c| Item::Atom(Atom::new(AtomKind::Number, c.to_string())))),
        N::Text { value } => out.push(node(Kind::Text, vec![value.chars().map(|c| Item::Atom(Atom::new(AtomKind::Text, c.to_string()))).collect()])),
        // An unfilled slot is kept in the document as a placeholder symbol:
        // it opens as an empty slot again, ready to fill.
        N::Symbol { value } if value == PLACEHOLDER => {}
        N::Symbol { value } => out.push(atom(AtomKind::Symbol, value)),
        N::Operator { value } => out.push(atom(AtomKind::Operator, value)),
        N::Relation { value } => out.push(atom(AtomKind::Relation, value)),
        N::Fraction { numerator, denominator } => out.push(node(Kind::Fraction, vec![row_from(numerator), row_from(denominator)])),
        N::Radical { radicand, index: None } => out.push(node(Kind::Sqrt, vec![row_from(radicand)])),
        N::Radical { radicand, index: Some(index) } => out.push(node(Kind::Root, vec![row_from(index), row_from(radicand)])),
        N::Superscript { base, exponent } => { push_base(base, out); out.push(node(Kind::Sup, vec![row_from(exponent)])); }
        N::Subscript { base, subscript } => { push_base(base, out); out.push(node(Kind::Sub, vec![row_from(subscript)])); }
        N::Scripts { base, subscript, superscript } => {
            push_base(base, out);
            match (subscript, superscript) {
                (Some(sub), Some(sup)) => out.push(node(Kind::SubSup, vec![row_from(sub), row_from(sup)])),
                (Some(sub), None) => out.push(node(Kind::Sub, vec![row_from(sub)])),
                (None, Some(sup)) => out.push(node(Kind::Sup, vec![row_from(sup)])),
                (None, None) => {}
            }
        }
        N::Matrix { rows } if !rows.is_empty() && rows.iter().all(|r| r.len() == rows[0].len() && !r.is_empty()) =>
            out.push(node(Kind::Matrix { cols: rows[0].len() }, rows.iter().flatten().map(row_from).collect())),
        N::Delimited { left, body, right } => out.push(node(Kind::Delimited { left: left.clone(), right: right.clone() }, vec![row_from(body)])),
        N::Function { name, argument } => out.push(node(Kind::Function(name.clone()), vec![row_from(argument)])),
        N::Accent { accent, body } => out.push(node(Kind::Accent(accent.clone()), vec![row_from(body)])),
        N::Nary { operator, body, subscript, superscript } => out.push(node(Kind::Nary(operator.clone()), vec![
            subscript.as_deref().map(row_from).unwrap_or_default(),
            superscript.as_deref().map(row_from).unwrap_or_default(),
            row_from(body)])),
        N::Boxed { body } => out.push(node(Kind::Boxed, vec![row_from(body)])),
        other => out.push(Item::Opaque(other.clone())),
    }
}

/// A script base that is more than one item is grouped, so the script
/// attaches to all of it.
fn push_base(base: &N, out: &mut Row) {
    let items = row_from(base);
    let single = items.len() == 1 && !matches!(&items[0], Item::Node(n) if n.kind.is_script());
    if single { out.extend(items); } else { out.push(node(Kind::Group, vec![items])); }
}

#[cfg(test)] #[path = "../../tests/unit/equation_editor/tree.rs"] mod tests;
