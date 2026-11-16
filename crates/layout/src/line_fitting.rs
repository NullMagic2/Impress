//! Width-safe composition shared by body, table-cell, header and note paragraphs.
//! Only an overwide text/cached-field token is split, at BOTH grapheme and shaping-cluster
//! boundaries. Source bytes are never changed and a discretionary hyphen is not
//! invented. This avoids turning ordinary words/ligatures into character runs.
use super::*;

pub(super) fn split_overwide_text(tokens: Vec<InlineToken>, limit: f32) -> Vec<InlineToken> {
    let mut out = Vec::with_capacity(tokens.len());
    for token in tokens {
        if token.width > limit.max(1.0) && matches!(token.kind,InlineTokenKind::Field{..}) {
            out.extend(field_wrapping::fragments(&token));continue;
        }
        let InlineTokenKind::Text { text, properties, shaped } = &token.kind else { out.push(token); continue; };
        if token.width <= limit.max(1.0) || token.whitespace { out.push(token); continue; }
        let clusters: std::collections::BTreeSet<usize> = shaped.glyphs.iter().map(|g| g.cluster).collect();
        let boundaries: Vec<usize> = text.grapheme_indices(true).map(|(i, _)| i)
            .filter(|i| *i == 0 || clusters.contains(i)).chain(std::iter::once(text.len())).collect();
        if boundaries.len() <= 2 { out.push(token); continue; }
        // Bucket each glyph once (also works for visually reversed RTL glyphs),
        // rather than filtering a whole long run separately for every character.
        let mut pieces = vec![Vec::new(); boundaries.len() - 1];
        for glyph in &shaped.glyphs {
            let i = boundaries.partition_point(|b| *b <= glyph.cluster).saturating_sub(1).min(pieces.len()-1);
            let mut g = glyph.clone(); g.cluster -= boundaries[i]; pieces[i].push(g);
        }
        for (i, glyphs) in pieces.into_iter().enumerate() {
            let a=boundaries[i]; let b=boundaries[i+1];
            let advance_pt=glyphs.iter().map(|g| g.x_advance_pt).sum::<f32>();
            out.push(InlineToken { start:token.start+a, end:token.start+b,
                whitespace:false,width:advance_pt,ascent:token.ascent,descent:token.descent,bidi_level:token.bidi_level,
                kind:InlineTokenKind::Text { text:text[a..b].to_owned(),properties:properties.clone(),
                    shaped:ShapedRun { glyphs, advance_pt, units_per_em:shaped.units_per_em } } });
        }
    }
    out
}

pub(super) fn balanced_breaks(tokens:&[InlineToken], width:f32, expansion:f32) -> Vec<usize> {
    // Bounded paragraph optimization, never cubic work during typing. Very long
    // paragraphs use the linear fitter below; typography is still shaped normally.
    if tokens.len()>512 { return Vec::new(); }
    let items:Vec<_>=tokens.iter().map(|t|match &t.kind {
        InlineTokenKind::HyphenOpportunity{..}=>LineItem::Penalty{width:t.width,penalty:55,flagged:true},
        _ if t.whitespace=>LineItem::Glue{width:t.width,stretch:t.width*DEFAULT_SPACE_STRETCH,shrink:t.width*DEFAULT_SPACE_SHRINK},
        _=>LineItem::Box{width:t.width},
    }).collect();
    compose(&items,typography::ComposeOptions{line_width:width,tolerance:4.0,
        microtype:MicrotypeOptions{max_glyph_expansion:expansion,..Default::default()},..Default::default()})
        .into_iter().map(|b|b.item_index).collect()
}

/// Return one fully reshaped line and its exclusive source-token end. The last
/// line has the same width constraint as every other line. Every call progresses.
pub(super) fn next_line(tokens:&[InlineToken], start:usize, source:&str,
    props:&model::ParagraphProperties, width:f32, indent:f32,
    preferred_end:Option<usize>, fonts:&FontCatalog) -> Result<(usize,Vec<InlineToken>),LayoutError> {
    let goal=(width-indent).max(1.0);
    let mut used=0.0; let mut end=start; let mut last_break=None;
    while end<tokens.len() {
        let t=&tokens[end];
        let advance=token_advance(t,false,0.0,1.0);
        // A trailing space stays in the source selection but does not require
        // a visible blank column past the text edge at an automatic wrap.
        if used+advance>goal+0.01 && end>start {
            if t.whitespace && source.get(t.start..t.end)!=Some("\t") {end+=1;last_break=Some(end);}
            break;
        }
        used+=advance; end+=1;
        if t.whitespace || matches!(t.kind,InlineTokenKind::HyphenOpportunity{..}) {
            let hyphen=if matches!(t.kind,InlineTokenKind::HyphenOpportunity{..}){t.width}else{0.0};
            if used+hyphen<=goal+0.01 {last_break=Some(end);}
        }
        if used>goal {break;}
    }
    if end<tokens.len() {if let Some(b)=last_break {end=b;}}
    if let Some(b)=preferred_end.filter(|b|*b>start && *b<end){end=b;}
    end=end.max(start+1).min(tokens.len());
    loop {
        let mut line=reshape_line_text(&tokens[start..end],fonts)?;
        // Preserve bytes/caret stops without painting automatic trailing spaces
        // into the adjacent cell. Explicit tabs retain their own positioning.
        if end<tokens.len() {
            for t in line.iter_mut().rev() {
                if !t.whitespace || source.get(t.start..t.end)==Some("\t"){break;}
                t.width=0.0;
            }
        }
        let tabs=line.iter().any(|t|source.get(t.start..t.end)==Some("\t"));
        let tab_result=if tabs {align_tabs(&mut line,source,props,width,indent)} else {Ok(())};
        let natural=line.iter().enumerate().map(|(i,t)|token_advance(t,i+1==line.len(),0.0,1.0)).sum::<f32>();
        if tab_result.is_ok() && natural<=goal+0.01 {return Ok((end,line));}
        if end==start+1 {
            // A single grapheme/object may be wider than an extremely narrow
            // cell. Keep its source editable; the cell/body clip is authoritative.
            if tabs {
                if let Some(t)=line.first_mut().filter(|t|matches!(t.kind,InlineTokenKind::TabLeader(_))){t.width=goal;}
                else {tab_result?;}
            }
            return Ok((end,line));
        }
        // Recheck the shaped output, not only pre-shaping estimates. Retain all
        // removed tokens for the next line; never truncate a paragraph tail.
        end-=1;
    }
}

pub(super) fn justification(tokens:&[InlineToken], goal:f32, expansion:f32)->(f32,f32) {
    let mut natural=0.0; let mut boxes=0.0; let mut stretch=0.0; let mut shrink=0.0;
    for (i,t) in tokens.iter().enumerate(){let w=token_advance(t,i+1==tokens.len(),0.0,1.0);natural+=w;
        if t.whitespace {stretch+=w*DEFAULT_SPACE_STRETCH;shrink+=w*DEFAULT_SPACE_SHRINK;}
        else if matches!(t.kind,InlineTokenKind::Text{..}){boxes+=w;}}
    if stretch<=0.0{return (0.0,1.0);}
    let expansion=if tokens.iter().any(|t|matches!(t.kind,InlineTokenKind::Equation{..}|InlineTokenKind::Image{..}|InlineTokenKind::Field{..})){0.0}else{expansion};
    let (ratio,scale,_)=typography::choose_line_adjustment(goal,natural,boxes,stretch,shrink,expansion,35.0);
    // The compositor tolerance ranks possible breaks; it must not turn an
    // already wrapped justified line into a ragged line. If its next word
    // cannot fit, distribute the remaining width over its interior spaces.
    if ratio.is_finite() && ratio >= -1.0 {(ratio,scale)} else {(0.0,1.0)}
}
