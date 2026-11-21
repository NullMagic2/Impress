//! Follow semantic hyperlinks without turning a normal editing click into navigation.
use crate::*;

/// Use the glyph interval under the pointer, rather than its nearest caret:
/// the trailing half of a link's last character still belongs to that link.
pub fn target_at(document: &Document, scene: &render::VectorDocument, viewport: &ViewportState,
    point: Point) -> Option<String> {
    let scale=viewport.points_to_pixels();
    let (page,rect)=viewport.page_rects_px(scene).into_iter().enumerate()
        .find(|(_,r)|contains(*r,point))?;
    let local=Point::new((point.x-rect.x)/scale,(point.y-rect.y)/scale);
    for region in scene.pages[page].hit_regions.iter().rev().filter(|r|contains(r.rect,local)) {
        let coordinate = region.inline_coordinate(local);
        let offset=region.caret_stops.windows(2).find_map(|pair| {
            let (a,b)=(&pair[0],&pair[1]);
            (coordinate>=a.x_pt.min(b.x_pt) && coordinate<a.x_pt.max(b.x_pt))
                .then_some(a.byte_offset.min(b.byte_offset))
        }).or_else(||region.caret_stops.is_empty().then_some(region.byte_range.start));
        let Some(offset)=offset.filter(|v|region.byte_range.contains(v)) else {continue;};
        let paragraph=model::document_paragraphs(document).into_iter().find(|p|p.id.0==region.node_id)?;
        if let Some(target)=inline_target(&paragraph.content,offset) {return Some(target.to_owned());}
    }
    None
}

fn contains(rect:render::Rect,p:Point)->bool {
    p.x>=rect.x && p.x<rect.max_x() && p.y>=rect.y && p.y<rect.max_y()
}

fn inline_target(items:&[model::Inline],offset:usize)->Option<&str> {
    let mut start=0;
    for item in items {
        let end=start+model::inline_flat_len(item);
        if (start..end).contains(&offset) {
            return if let model::Inline::Hyperlink(link)=item {
                inline_target(&link.content,offset-start).or(Some(link.target.as_str()))
            }else{None};
        }
        start=end;
    }
    None
}

pub fn under(state:&CanvasState,point:Point)->Option<String> {
    if equation_ui::previewing(state) || state.display_generation!=Some(state.layout_generation) {return None;}
    target_at(&state.editor.document,&state.vector_document,&state.viewport,point)
}

/// Restrict browser navigation to URL schemes; document bookmarks stay internal.
fn browser_url(target:&str)->Result<&str,String> {
    let url=target.trim();
    let scheme=url.split_once(':').map(|(scheme,_)|scheme.to_ascii_lowercase());
    if url.chars().any(char::is_control) || !matches!(scheme.as_deref(),Some("http"|"https"|"mailto"|"ftp"|"ftps")) {
        return Err("This hyperlink does not contain a supported web or email URL.".into());
    }
    Ok(url)
}

pub fn follow(state:&Rc<RefCell<CanvasState>>,target:&str)->Result<(),String> {
    if let Some(name)=target.strip_prefix('#') {
        let mut s=state.borrow_mut();
        let range=s.editor.document.review.bookmarks.values().find(|b|b.name==name)
            .and_then(|b|b.range.clone()).ok_or("The hyperlink's bookmark is no longer available.")?;
        s.editor.go_to_range(&range)?;
        s.selected_object=None;s.preferred_caret_x_pt=None;s.caret_visual_line=None;s.caret_page_hint=None;
        refresh_overlay(&mut s);reveal_review_caret(&mut s);wake_caret(&mut s);
        return Ok(());
    }
    // Launch outside a CanvasState borrow: native launchers can dispatch events.
    if wxdragon::utils::launch_default_browser(browser_url(target)?,wxdragon::utils::BrowserLaunchFlags::Default) {
        Ok(())
    }else{Err("Could not open the hyperlink in your default browser.".into())}
}

#[cfg(test)] #[path="../tests/unit/hyperlinks.rs"] mod tests;
