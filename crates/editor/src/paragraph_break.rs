//! An empty paragraph still owns the character format of its paragraph mark.
//! Keep that format semantic so caret movement, Undo and saving don't lose it.
use super::*;

impl EditorState {
    pub(super) fn split_paragraph_pair(&self, before: &Paragraph, at: usize) -> Result<(Paragraph, Paragraph), String> {
        let (left, right) = split_content(&before.content, at)?;
        // A split inside existing text continues the same paragraph style.
        // The configured following style applies when starting a new one at
        // the end, e.g. Heading 1 -> Normal, or Body -> Body.
        let style = if at >= flat_content_len(&before.content) {
            self.following_style(before.style.as_ref())
        } else { before.style.clone() };
        let same_style = style == before.style;
        let mut head = before.clone();
        head.content = self.split_content_with_mark(left, true).into();
        let tail = Paragraph {
            id: NodeId::new(), style,
            properties: if same_style { before.properties.clone() } else { Default::default() },
            content: self.split_content_with_mark(right, same_style).into(),
        };
        Ok((head, tail))
    }

    fn split_content_with_mark(&self, content: Vec<Inline>, inherit: bool) -> Vec<Inline> {
        if flat_content_len(&content) != 0 { return normalized_content(content); }
        let mut mark = TextRun::plain("");
        if inherit {
            mark.style = self.typing_style.clone();
            mark.properties = self.typing_properties.clone();
        }
        vec![Inline::Text(mark)]
    }
}

#[cfg(test)] #[path = "../tests/unit/paragraph_break.rs"] mod tests;
