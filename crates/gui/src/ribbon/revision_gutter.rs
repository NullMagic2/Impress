//! The deletion lane follows the document's vertical scroll. As a sibling of
//! the canvas, it stays within the window even when a wide table fills the page.
use super::*;
use unicode_segmentation::UnicodeSegmentation;
use wxdragon::{
    DeviceContext,
    dc::{AutoBufferedPaintDC, PenStyle},
    window::BackgroundStyle,
};

#[derive(Clone, Copy, PartialEq)]
struct ViewKey {
    epoch: u64,
    revision: u64,
    generation: Option<u64>,
    layout: usize,
    viewport: render::ViewportState,
}

#[derive(Default)]
pub(super) struct Gutter {
    key: Cell<Option<ViewKey>>,
    rows: RefCell<Vec<Row>>,
    hover: Cell<Option<usize>>,
}

struct Row {
    y: i32,
    height: i32,
    text: String,
    truncated: bool,
}

struct Group {
    anchor: i32,
    text: String,
}

fn group_rows(entries: Vec<(i32, String)>, capacity: usize) -> Vec<Group> {
    if capacity == 0 {
        return Vec::new();
    }
    let mut lines = Vec::<Group>::new();
    for (anchor, text) in entries {
        if let Some(line) = lines.last_mut().filter(|line| anchor - line.anchor <= 1) {
            line.text.push_str(" · ");
            line.text.push_str(&text);
        } else {
            lines.push(Group { anchor, text });
        }
    }
    if lines.len() <= capacity {
        return lines;
    }
    // Preserve a representation of every visible change when the zoom makes
    // more document lines fit than the annotation font can display.
    let count = lines.len();
    let mut merged = Vec::<Group>::with_capacity(capacity);
    for (index, line) in lines.into_iter().enumerate() {
        let slot = index * capacity / count;
        if let Some(group) = merged.get_mut(slot) {
            group.text.push_str(" · ");
            group.text.push_str(&line.text);
        } else {
            merged.push(line);
        }
    }
    merged
}

fn fitted<'a>(dc: &impl DeviceContext, text: &'a str, width: i32) -> std::borrow::Cow<'a, str> {
    if dc.get_text_extent(text).0 <= width {
        return text.into();
    }
    let offsets: Vec<_> = text
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .collect();
    let count = offsets
        .partition_point(|&end| dc.get_text_extent(&format!("{}…", &text[..end])).0 <= width);
    let end = offsets[count.saturating_sub(1)];
    format!("{}…", text[..end].trim_end()).into()
}

pub(super) fn bind(ribbon: &Rc<Ribbon>) {
    let panel = ribbon.ui.revision_gutter_panel;
    panel.set_background_style(BackgroundStyle::Paint);
    panel.on_erase_background(|_| {});
    let weak = Rc::downgrade(ribbon);
    panel.on_paint(move |_| {
        if let Some(ribbon) = weak.upgrade() {
            ribbon.paint_revision_gutter();
        }
    });
    panel.on_size(move |event| {
        panel.refresh(false, None);
        event.skip(true);
    });
    let weak = Rc::downgrade(ribbon);
    panel.on_mouse_motion(move |event| {
        let (Some(ribbon), WindowEventData::MouseMotion(mouse)) = (weak.upgrade(), &event) else {
            return;
        };
        let Some(point) = mouse.get_position() else {
            return;
        };
        let gutter = &ribbon.revision_gutter;
        let rows = gutter.rows.borrow();
        let row = rows
            .iter()
            .position(|row| row.truncated && point.y >= row.y && point.y < row.y + row.height);
        if gutter.hover.replace(row) != row {
            panel.set_tooltip(row.map_or("", |index| rows[index].text.as_str()));
        }
        event.skip(true);
    });
}

impl Ribbon {
    pub(super) fn sync_revision_gutter(&self) {
        let (show, key) = {
            let state = self.state.borrow();
            let show =
                self.mode.get() != ViewMode::Typst && revision_tools::shows_deletions(&state);
            (
                show,
                show.then(|| ViewKey {
                    epoch: state.documents.epoch,
                    revision: state.editor.revision,
                    generation: state.display_generation,
                    layout: Arc::as_ptr(&state.vector_document) as usize,
                    viewport: state.viewport,
                }),
            )
        };
        let panel = self.ui.revision_gutter_panel;
        if panel.is_shown() != show {
            panel.show(show);
            self.ui.canvas_container.layout();
        }
        if self.revision_gutter.key.replace(key) != key && show {
            panel.refresh(false, None);
        }
    }

    fn paint_revision_gutter(&self) {
        let panel = self.ui.revision_gutter_panel;
        let dc = AutoBufferedPaintDC::new(&panel);
        let size = panel.get_client_size();
        let d = |n| panel.from_dip_int(n);
        dc.set_background(Colour::rgb(246, 248, 251));
        dc.clear();
        dc.set_pen(Colour::rgb(207, 216, 229), 1, PenStyle::Solid);
        dc.draw_line(0, 0, 0, size.height);
        if let Some(font) = panel.get_font() {
            dc.set_font(&font);
        }
        let (_, text_height) = dc.get_text_extent("Ag");
        let gap = d(2).max(1);
        let line_height = text_height.max(1) + gap;
        let content_left = d(19);
        let content_width = size.width - content_left - d(10);
        let vertical_pad = d(3);
        let usable = (size.height - 2 * vertical_pad).max(0);
        let gutter = &self.revision_gutter;
        gutter.hover.set(None);
        panel.set_tooltip("");
        let mut rows = gutter.rows.borrow_mut();
        rows.clear();
        if content_width < d(10) {
            return;
        }

        let groups = group_rows(
            revision_tools::deletion_rows(&self.state.borrow()),
            (usable / line_height) as usize,
        );
        let mut y = vertical_pad;
        let count = groups.len();
        let ink = Colour::rgb(158, 44, 48);
        dc.set_text_foreground(ink);
        for (index, group) in groups.into_iter().enumerate() {
            let latest = size.height - vertical_pad - (count - index) as i32 * line_height;
            y = (group.anchor - text_height / 2).clamp(y, latest);
            let centre = y + text_height / 2;
            let fitted = fitted(&dc, &group.text, content_width);
            let truncated = fitted.as_ref() != group.text;
            dc.set_pen(Colour::rgb(187, 104, 106), 1, PenStyle::Solid);
            dc.draw_line(d(3), group.anchor, d(9), centre);
            dc.draw_line(d(9), centre, content_left - d(4), centre);
            dc.draw_text(fitted.as_ref(), content_left, y);
            dc.set_pen(ink, 1, PenStyle::Solid);
            dc.draw_line(
                content_left,
                centre,
                content_left + dc.get_text_extent(fitted.as_ref()).0,
                centre,
            );
            rows.push(Row {
                y,
                height: line_height,
                text: group.text,
                truncated,
            });
            y += line_height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_line_and_dense_deletions_share_space_without_disappearing() {
        let entries = [(12, "first"), (12, "second"), (21, "third"), (24, "fourth")]
            .into_iter()
            .map(|(y, text)| (y, text.to_owned()))
            .collect();
        let groups = group_rows(entries, 2);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].text, "first · second · third");
        assert_eq!(groups[1].text, "fourth");
    }
}
