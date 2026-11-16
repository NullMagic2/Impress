//! The slots of an equation on the page, without the equation pane: which
//! slot (a fraction's numerator, a power's exponent, an empty box…) lies under
//! a click, where it is, and how to fill it. The formula is laid out as the
//! pane lays it out, which follows Typst's rendering closely enough to find
//! the slot a click on the rendered equation meant.
use super::layout::{self, Fonts, MathBox};
use super::tree::{Editor, Path};
use model::EquationNode;

/// Pixels per em for the off-screen layout; only proportions are used.
const SIZE: f32 = 40.0;

/// A slot found under a click.
pub struct Slot {
    pub path: Path,
    /// Its box as fractions of the equation's width and height: x, y, w, h.
    pub area: (f32, f32, f32, f32),
}

fn laid_out(root: &EquationNode) -> Option<(Editor, MathBox)> {
    let fonts = Fonts::get()?;
    let editor = Editor::from_node(root);
    let mbox = layout::layout(fonts, &editor.root, SIZE, &Vec::new());
    Some((editor, mbox))
}

fn slot(mbox: &MathBox, path: Path) -> Option<Slot> {
    let row = mbox.row(&path)?;
    let (w, h) = (mbox.w.max(1.0), mbox.height().max(1.0));
    let area = (row.rect.x / w, (row.rect.y + mbox.asc) / h, row.rect.w / w, row.rect.h / h);
    Some(Slot { path, area })
}

/// The slot under a click at (`fx`, `fy`), fractions of the equation's width
/// and height; None on the equation's own top-level row (not in a slot).
pub fn slot_at(root: &EquationNode, fx: f32, fy: f32) -> Option<Slot> {
    let (_, mbox) = laid_out(root)?;
    let (path, _) = mbox.hit(fx * mbox.w, -mbox.asc + fy * mbox.height())?;
    if path.is_empty() { return None; }
    slot(&mbox, path)
}

/// The slot at `path`, if the equation still has it.
pub fn slot_by_path(root: &EquationNode, path: &Path) -> Option<Slot> {
    let (_, mbox) = laid_out(root)?;
    slot(&mbox, path.clone())
}

/// The first empty slot after `after` in reading order, wrapping around.
pub fn next_empty(root: &EquationNode, after: &Path) -> Option<Path> {
    let (editor, mbox) = laid_out(root)?;
    let empty: Vec<&Path> = mbox.rows.iter().map(|r| &r.path)
        .filter(|p| !p.is_empty() && editor.row(p).is_empty() && mbox.row(p).is_some_and(|r| r.rect.w > 0.0)).collect();
    let at = empty.iter().position(|p| *p == after);
    let next = match at { Some(i) => empty.get(i + 1).or(empty.first()), None => empty.first() };
    next.map(|p| (*p).clone()).filter(|p| p != after)
}

/// Whether a selected slot is already an empty placeholder.
pub fn is_empty(root: &EquationNode, path: &Path) -> bool {
    Editor::from_node(root).row(path).is_empty()
}

/// Previous structural row, or None when navigation leaves the equation.
pub fn previous(root: &EquationNode, path: &Path) -> Option<Path> {
    let mut editor = Editor::from_node(root);
    editor.set_cursor(path.clone(), 0);
    editor.move_left(false);
    (!editor.cursor.path.is_empty() && editor.cursor.path != *path).then_some(editor.cursor.path)
}

/// The equation with the slot at `path` holding `text`, typed as in the
/// equation pane (so `1/2` makes a fraction there). Empty text empties it.
pub fn fill(root: &EquationNode, path: &Path, text: &str) -> Result<EquationNode, String> {
    let mut editor = Editor::from_node(root);
    let len = editor.row(path).len();
    editor.select(path.clone(), 0, len);
    editor.delete_selection();
    for c in text.trim().chars() { editor.type_char(c); }
    editor.resolve_command();
    editor.to_node_live()
}
