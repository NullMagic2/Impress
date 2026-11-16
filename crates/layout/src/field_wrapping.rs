//! Cached field values can wrap visually while remaining one semantic object.
//! All fragments address the original three-byte object marker, never offsets
//! into its cached display string (which is not editable paragraph text).
use super::*;

pub(super) fn fragments(token: &InlineToken) -> Vec<InlineToken> {
    let InlineTokenKind::Field { pieces, properties, lift } = &token.kind else { return vec![token.clone()]; };
    let mut out = Vec::new();
    for piece in pieces {
        let clusters: std::collections::BTreeSet<usize> = piece.shaped.glyphs.iter().map(|g| g.cluster).collect();
        let boundaries: Vec<usize> = piece.text.grapheme_indices(true).map(|(i, _)| i)
            .filter(|i| *i == 0 || clusters.contains(i))
            .chain(std::iter::once(piece.text.len())).collect();
        let mut glyphs = vec![Vec::new(); boundaries.len().saturating_sub(1)];
        for glyph in &piece.shaped.glyphs {
            if glyphs.is_empty() { break; }
            let index = boundaries.partition_point(|b| *b <= glyph.cluster).saturating_sub(1).min(glyphs.len() - 1);
            let mut glyph = glyph.clone(); glyph.cluster -= boundaries[index]; glyphs[index].push(glyph);
        }
        for (i, glyphs) in glyphs.into_iter().enumerate() {
            let text = piece.text[boundaries[i]..boundaries[i + 1]].to_owned();
            let advance_pt = glyphs.iter().map(|g| g.x_advance_pt).sum();
            out.push(InlineToken { start: token.start, end: token.end,
                whitespace: text.chars().all(char::is_whitespace), width: advance_pt,
                ascent: token.ascent, descent: token.descent, bidi_level: token.bidi_level,
                kind: InlineTokenKind::Field { properties: properties.clone(), lift: *lift,
                    pieces: vec![FieldFontPiece { text, properties: piece.properties.clone(),
                        shaped: ShapedRun { glyphs, advance_pt, units_per_em: piece.shaped.units_per_em } }] },
            });
        }
    }
    out
}

pub(super) fn reshape(token: &mut InlineToken, fonts: &FontCatalog) -> Result<(), LayoutError> {
    let InlineTokenKind::Field { pieces, .. } = &mut token.kind else { return Ok(()); };
    // Adjacent fragments in one line regain normal kerning, ligatures and
    // joining. Regional font changes remain distinct paint runs.
    let mut merged: Vec<FieldFontPiece> = Vec::new();
    for piece in std::mem::take(pieces) {
        if let Some(last) = merged.last_mut().filter(|last| last.properties == piece.properties) {
            last.text.push_str(&piece.text);
        } else { merged.push(piece); }
    }
    for piece in &mut merged { piece.shaped = shape_token(&piece.text, &piece.properties, fonts)?; }
    token.width = merged.iter().map(|p| p.shaped.advance_pt).sum();
    *pieces = merged;
    Ok(())
}
