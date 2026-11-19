//! Direct manipulation uses immutable hit geometry from the accepted layout.
//! Motion only changes an overlay; mouse release performs one semantic edit.
use crate::*;
use render::{ObjectHitRegion, ObjectKind, Rect, TableCellHit};
use editor::HeaderFooterVariant;
use crate::picture_chrome::{Handle, Target as PictureTarget};
use model::{AnchorReference, ImageAnchor, ImageWrap};
const MM_TO_PT: f32 = 72.0 / 25.4;
/// A picture dropped after a free move floats with this wrapping and text
/// distance (Word's "Square" with its default 3 mm gap).
const FREE_PICTURE_WRAP: ImageWrap = ImageWrap::Square;
const FREE_PICTURE_GAP_MM: f32 = 3.0;
/// Turns smaller than this, in degrees, are treated as a click.
const MIN_TURN_DEG: f32 = 0.5;
#[derive(Clone, Debug, PartialEq)]
pub enum SelectedObject {
    Picture(NodeId), Equation(NodeId), Cells { table_id: NodeId, cell_ids: Vec<NodeId> },
}
#[derive(Clone)]
pub struct Drag {
    pub page: usize,
    original: Rect,
    pub preview: Rect,
    start: Point,
    revision: u64,
    moved: bool,
    kind: DragKind,
}
#[derive(Clone)]
enum DragKind {
    Resize { id: NodeId, handle: Handle },
    /// Turning around `center` (page points) by `degrees`, clockwise.
    Rotate { id: NodeId, center: Point, degrees: f32 },
    /// An inline picture: moves freely and floats where it is dropped, or,
    /// with Shift (`in_text`), moves within the text to `target`.
    Move { id: NodeId, target: Option<DocumentPosition>, in_text: bool },
    FloatMove {id:NodeId},
    Column { info: TableCellHit, widths: Vec<f32> },
    Row { info: TableCellHit, height: f32 },
}
impl Drag {
    /// The moved or resized picture outline: (page, rect in page points,
    /// is a resize). None for table borders and before the pointer moves.
    pub(crate) fn picture_preview(&self) -> Option<(usize, Rect, bool)> {
        if !self.moved { return None; }
        match self.kind {
            DragKind::Resize { .. } => Some((self.page, self.preview, true)),
            DragKind::Move { .. } | DragKind::FloatMove { .. } => Some((self.page, self.preview, false)),
            DragKind::Rotate { .. } | DragKind::Column { .. } | DragKind::Row { .. } => None,
        }
    }
    /// The picture being turned: (page, its rect in page points, the turn so
    /// far in degrees). None before the pointer moves.
    pub(crate) fn rotation_preview(&self) -> Option<(usize, Rect, f32)> {
        match self.kind {
            DragKind::Rotate { degrees, .. } if self.moved => Some((self.page, self.original, degrees)),
            _ => None,
        }
    }
}
/// Shown when a picture is selected; the keys match picture_chrome's controls.
const PICTURE_SELECTED_STATUS: &str = "Picture selected";
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pointer { Arrow, Text, ResizeNwSe, ResizeNeSw, ResizeColumn, ResizeRow, Move, Hand, Rotate }
pub(crate) fn contains(r: Rect, p: Point) -> bool { p.x >= r.x && p.x <= r.x+r.width && p.y >= r.y && p.y <= r.y+r.height }
pub(crate) fn page_point(state: &CanvasState, page: usize, p: Point) -> Option<Point> {
    let rect = state.viewport.page_rect_px(&state.vector_document, page)?;
    let scale = state.viewport.points_to_pixels().max(0.01);
    Some(Point::new((p.x-rect.x)/scale, (p.y-rect.y)/scale))
}
pub(crate) fn region_for(state: &CanvasState, id: &NodeId) -> Option<(usize, ObjectHitRegion)> {
    let preferred = state.caret_page_hint;
    state.vector_document.pages.iter().enumerate()
        .filter_map(|(page,p)| p.object_regions.iter().find(|r| r.node_id==id.0 && matches!(r.kind,ObjectKind::Picture{..}|ObjectKind::Equation{..})).map(|r|(page,r.clone())))
        .min_by_key(|(page,_)| if Some(*page)==preferred {0} else {1})
}
/// A click at `p` on the equation the equation pane edits: where in it, as a
/// fraction of its size, for the pane to put its cursor there. Typst sets
/// each equation with a 2 pt margin around it, which is left out.
pub(crate) fn draft_equation_click(state: &mut CanvasState, p: Point) -> bool {
    let Some(id) = crate::equation_ui::draft_equation(state) else { return false; };
    let Some((page, region)) = picture_or_equation(state, p) else { return false; };
    if region.node_id != id.0 || !matches!(region.kind, ObjectKind::Equation{..}) { return false; }
    let Some(at) = click_fraction(state, page, &region, p) else { return false; };
    state.equation_slot_click = Some(at);
    true
}

/// Where `p` falls in an equation's region, as a fraction of its width and
/// height, leaving out the 2 pt margin Typst sets around each equation.
fn click_fraction(state: &CanvasState, page: usize, region: &ObjectHitRegion, p: Point) -> Option<(f32, f32)> {
    const MARGIN_PT: f32 = 2.0;
    let point = page_point(state, page, p)?;
    let r = region.rect;
    let (w, h) = ((r.width - 2.0 * MARGIN_PT).max(1.0), (r.height - 2.0 * MARGIN_PT).max(1.0));
    Some((((point.x - r.x - MARGIN_PT) / w).clamp(0.0, 1.0), ((point.y - r.y - MARGIN_PT) / h).clamp(0.0, 1.0)))
}


pub(crate) fn picture_or_equation(state: &CanvasState, p: Point) -> Option<(usize, ObjectHitRegion)> {
    for page in state.viewport.visible_pages(&state.vector_document) {
        let point = page_point(state,page,p)?;
        if let Some(r) = state.vector_document.pages[page].object_regions.iter().rev()
            .find(|r| matches!(r.kind, ObjectKind::Picture{..}|ObjectKind::Equation{..}) && contains(r.rect,point)
                && !text_over_behind_picture(state,page,r,point)) { return Some((page,r.clone())); }
    } None
}
/// As in Word, a click on text lying over a Behind Text picture goes to the
/// text; the picture is picked where no text covers it, or once selected.
fn text_over_behind_picture(state: &CanvasState, page: usize, region: &ObjectHitRegion, point: Point) -> bool {
    let id = NodeId::from(region.node_id.as_str());
    if state.editor.picture_wrap(&id) != ImageWrap::BehindText { return false; }
    if matches!(&state.selected_object, Some(SelectedObject::Picture(selected)) if *selected == id) { return false; }
    state.vector_document.pages[page].hit_regions.iter().any(|t| contains(t.rect, point))
}
/// Corners clockwise from the top left.
pub(crate) fn corners(rect: Rect) -> [Point;4] { [Point::new(rect.x,rect.y),Point::new(rect.x+rect.width,rect.y),Point::new(rect.x+rect.width,rect.y+rect.height),Point::new(rect.x,rect.y+rect.height)] }
/// The selected picture's resize or rotate handle under `point`:
/// (page, region, control).
fn picture_handle_hit(state: &CanvasState, point: Point) -> Option<(usize,ObjectHitRegion,PictureTarget)> {
    let target=crate::picture_chrome::target(state,point).filter(|t|*t!=PictureTarget::Delete)?;
    let (page,region)=selected_picture(state)?;
    Some((page,region,target))
}
/// The cursor over a resize handle: diagonal on corners, straight on edges.
fn handle_pointer(handle: Handle) -> Pointer {
    match handle.sides() {
        (0, _) => Pointer::ResizeRow,
        (_, 0) => Pointer::ResizeColumn,
        (h, v) if h == v => Pointer::ResizeNwSe,
        _ => Pointer::ResizeNeSw,
    }
}
/// The selected picture's region and the page it is shown on.
pub(crate) fn selected_picture(state: &CanvasState) -> Option<(usize,ObjectHitRegion)> {
    let SelectedObject::Picture(id) = state.selected_object.as_ref()? else {return None;};
    region_for(state,id)
}
/// Esc on a selected picture or equation: deselects it. Returns false when
/// neither is selected.
pub fn deselect(state:&mut CanvasState)->bool {
    if !matches!(state.selected_object,Some(SelectedObject::Picture(_)|SelectedObject::Equation(_))) {return false;}
    state.selected_object=None;state.action_status=None;refresh_overlay(state);state.needs_present=true;true
}
fn table_boundary(state:&CanvasState,p:Point)->Option<(usize,ObjectHitRegion,bool)> {
    let tolerance=4.0/state.viewport.points_to_pixels().max(0.01);
    let mut found:Option<(f32,usize,ObjectHitRegion,bool)>=None;
    for page in state.viewport.visible_pages(&state.vector_document) {
        let Some(p)=page_point(state,page,p) else{continue;};
        for region in &state.vector_document.pages[page].object_regions {
            let ObjectKind::TableCell(info)=&region.kind else{continue;};
            let r=region.rect;
            let col=(p.x-r.x-r.width).abs(); let row=(p.y-r.y-r.height).abs();
            let is_col=col<=tolerance && p.y>=r.y && p.y<=r.y+r.height;
            let is_row=info.can_resize_row && row<=tolerance && p.x>=r.x && p.x<=r.x+r.width;
            if !is_col && !is_row {continue;}
            let distance=if is_col {col} else{row};
            // Nested cells win ties; they are the smaller visual object.
            let rank=distance + r.width*r.height*0.0000001;
            if found.as_ref().is_none_or(|old|rank<old.0) {found=Some((rank,page,region.clone(),is_col));}
        }
    }
    found.map(|(_,page,region,col)|(page,region,col))
}
/// The table border being dragged, or hovered: (page, table id, is a column
/// border, position in page points).
pub fn resize_guide(state:&CanvasState)->Option<(usize,String,bool,f32)> {
    if let Some(drag)=&state.object_drag {
        return match &drag.kind {
            DragKind::Column{info,..}=>Some((drag.page,info.table_id.clone(),true,drag.preview.x+drag.preview.width)),
            DragKind::Row{info,..}=>Some((drag.page,info.table_id.clone(),false,drag.preview.y+drag.preview.height)),
            _=>None,
        };
    }
    if crate::table_ui::over_control(state)||crate::equation_ui::previewing(state){return None;}
    let (page,region,column)=table_boundary(state,state.table_pointer?)?;
    let ObjectKind::TableCell(info)=&region.kind else{return None;};
    let r=region.rect;
    Some((page,info.table_id.clone(),column,if column{r.x+r.width}else{r.y+r.height}))
}
fn paper_pointer(viewport: &render::ViewportState, document: &render::VectorDocument, point: Point) -> Pointer {
    if viewport.page_rects_px(document).into_iter().any(|page| contains(page, point)) {
        Pointer::Text
    } else {
        Pointer::Arrow
    }
}

pub fn hover(state:&CanvasState,p:Point)->Pointer {
    if crate::equation_ui::previewing(state) {
        return paper_pointer(&state.viewport, &state.vector_document, p);
    }
    if let Some(drag)=&state.object_drag {
        match &drag.kind {
            DragKind::Resize{handle,..}=>return handle_pointer(*handle),
            DragKind::Rotate{..}=>return Pointer::Rotate,
            DragKind::Move{..}|DragKind::FloatMove{..}=>return Pointer::Move,
            _=>{}
        }
    }
    match crate::picture_chrome::target(state,p) {
        Some(PictureTarget::Handle(handle))=>return handle_pointer(handle),
        Some(PictureTarget::Rotate)=>return Pointer::Rotate,
        Some(PictureTarget::Delete|PictureTarget::Layout)=>return Pointer::Hand,
        None=>{}
    }
    if let Some(pointer)=crate::table_ui::pointer(state) {return pointer;}
    if let Some((_,_,column))=table_boundary(state,p){return if column{Pointer::ResizeColumn}else{Pointer::ResizeRow};}
    if picture_or_equation(state,p).is_some_and(|(_,r)|matches!(r.kind,ObjectKind::Picture{source:Some(_)})||state.editor.picture_anchor(&NodeId::from(r.node_id.as_str())).is_some()){return Pointer::Move;}
    paper_pointer(&state.viewport, &state.vector_document, p)
}
/// The cell of `table` under `point`, or, past the table's edge, the nearest
/// one, so a drag beyond the table keeps selecting its outer cells.
fn cell_toward(state:&CanvasState,table:&NodeId,point:Point)->Option<(usize,NodeId)>{
    let ours=|page:usize|state.vector_document.pages[page].object_regions.iter().filter(move|r|matches!(&r.kind,ObjectKind::TableCell(c)if c.table_id==table.0));
    // The page whose part of the table is nearest the pointer.
    let mut best:Option<(f32,usize,Point,Rect)>=None;
    for page in state.viewport.visible_pages(&state.vector_document){
        let Some(p)=page_point(state,page,point)else{continue;};
        let Some(bounds)=ours(page).map(|r|r.rect).reduce(|a,b|{let (x,y)=(a.x.min(b.x),a.y.min(b.y));Rect::new(x,y,(a.x+a.width).max(b.x+b.width)-x,(a.y+a.height).max(b.y+b.height)-y)})else{continue;};
        let dx=(bounds.x-p.x).max(p.x-bounds.x-bounds.width).max(0.0);let dy=(bounds.y-p.y).max(p.y-bounds.y-bounds.height).max(0.0);
        let distance=dx*dx+dy*dy;
        if best.is_none_or(|b|distance<b.0){best=Some((distance,page,p,bounds));}
    }
    let (_,page,p,bounds)=best?;
    let q=Point::new(p.x.clamp(bounds.x+0.5,bounds.x+bounds.width-0.5),p.y.clamp(bounds.y+0.5,bounds.y+bounds.height-0.5));
    ours(page).collect::<Vec<_>>().into_iter().rev().find(|r|contains(r.rect,q)).map(|r|(page,NodeId::from(r.node_id.as_str())))
}
fn select_cell_rectangle(state:&mut CanvasState,table:NodeId,first:&NodeId,last:&NodeId,page:usize){
    match state.editor.table_rectangle_between(&table,first,last){
        Ok(cell_ids)=>{
            let changed=!matches!(&state.selected_object,Some(SelectedObject::Cells{table_id,cell_ids:old})if *table_id==table&&*old==cell_ids);
            let count=cell_ids.len();
            state.selected_object=Some(SelectedObject::Cells{table_id:table,cell_ids});state.caret_page_hint=Some(page);
            if changed{state.action_status=Some(format!("{count} cells selected — Copy/Paste preserves merges; Delete clears them. Click normally to edit text."));refresh_overlay(state);state.needs_present=true;}
        }
        Err(error)=>state.action_status=Some(error),
    }
}
/// Shift+click from a table cell into another cell of the same table selects
/// the rectangle of cells between them, as in Word; dragging on extends it.
/// A Shift+click in the same cell, or outside the table, extends the text.
fn extend_cell_selection(state:&mut CanvasState,point:Point)->bool{
    let Some(anchor)=state.editor.selection.as_ref().map(|s|s.anchor.node_id.clone())else{return false;};
    let Some((table,first))=state.editor.table_cell_of(&anchor)else{return false;};
    let Some(hit)=state.viewport.hit_test(&state.vector_document,point)else{return false;};
    if state.editor.table_cell_of(&NodeId::from(hit.node_id.as_str())).is_none_or(|(t,c)|t!=table||c==first){return false;}
    let Some((page,last))=cell_toward(state,&table,point)else{return false;};
    state.editor.set_caret(anchor,state.editor.selection.as_ref().map_or(0,|s|s.anchor.byte_offset));
    state.dragging_selection=true;
    state.cell_drag_anchor=Some((table.clone(),first.clone()));
    select_cell_rectangle(state,table,&first,&last,page);true
}
/// A mouse selection that started in a table cell: once the pointer reaches
/// another cell it selects whole cells (the rectangle between the two), as
/// in Word; back in the first cell it selects text again. Returns true while
/// cells are selected.
pub fn drag_cells(state:&mut CanvasState,point:Point)->bool{
    let Some((table,anchor))=state.cell_drag_anchor.clone()else{return false;};
    let Some((page,cell))=cell_toward(state,&table,point)else{return matches!(state.selected_object,Some(SelectedObject::Cells{..}));};
    if cell==anchor{
        if state.selected_object.take().is_some(){refresh_overlay(state);state.needs_present=true;}
        return false;
    }
    // Only the cells are selected, not text running across them.
    if let Some(selection)=state.editor.selection.clone(){if !selection.is_collapsed(){state.editor.set_caret(selection.anchor.node_id.clone(),selection.anchor.byte_offset);}}
    select_cell_rectangle(state,table,&anchor,&cell,page);true
}
pub fn down(state:&mut CanvasState,point:Point)->bool {
    if crate::equation_ui::previewing(state) {return true;}
    if state.display_generation != Some(state.layout_generation) {state.selected_object=None;return false;}
    for page in state.viewport.visible_pages(&state.vector_document){if let Some(p)=page_point(state,page,point){if let Some(id)=state.vector_document.pages[page].object_regions.iter().rev().find_map(|r|match &r.kind{ObjectKind::CommentThread{comment_id}if contains(r.rect,p)=>Some(comment_id.clone()),_=>None}){state.pending_comment=Some(NodeId::from(id.as_str()));return true;}}}

    if state.shift_click&&extend_cell_selection(state,point){return true;}
    if let Some((page,region,target))=picture_handle_hit(state,point) {
        let Some(start)=page_point(state,page,point) else{return true;};
        let id=NodeId::from(region.node_id.as_str());
        let r=region.rect;
        let kind=match target {
            PictureTarget::Rotate=>DragKind::Rotate{id,center:Point::new(r.x+r.width*0.5,r.y+r.height*0.5),degrees:0.0},
            PictureTarget::Handle(handle)=>DragKind::Resize{id,handle},
            PictureTarget::Delete|PictureTarget::Layout=>return true,
        };
        state.object_drag=Some(Drag {page,original:r,preview:r,start,revision:state.editor.revision,moved:false,kind});
        return true;
    }
    if let Some((page,region,column))=table_boundary(state,point) {
        let ObjectKind::TableCell(info)=region.kind else{unreachable!()};
        let id=NodeId::from(info.table_id.as_str());
        if let Err(e)=state.editor.select_cell(&id,info.row_index,info.cell_index){state.action_status=Some(e);return true;}
        let kind=if column {DragKind::Column {widths:info.column_widths_pt.clone(),info}}
            else{DragKind::Row{height:info.row_height_pt,info}};
        state.selected_object=None;state.caret_page_hint=Some(page);
        state.object_drag=Some(Drag {page,original:region.rect,preview:region.rect,start:page_point(state,page,point).unwrap(),revision:state.editor.revision,moved:false,kind});
        refresh_overlay(state);return true;
    }
    if let Some((page,region))=picture_or_equation(state,point) {
        let id=NodeId::from(region.node_id.as_str());state.caret_page_hint=Some(page);
        match &region.kind {
            ObjectKind::Picture{source}=>{
                state.selected_object=Some(SelectedObject::Picture(id.clone()));
                state.action_status=Some(PICTURE_SELECTED_STATUS.into());
                if let Some((paragraph,offset))=source {
                    state.editor.set_caret(NodeId::from(paragraph.as_str()),*offset);
                    state.editor.extend_selection(NodeId::from(paragraph.as_str()),*offset+editor::INLINE_OBJECT_MARKER.len());
                    state.object_drag=Some(Drag {page,original:region.rect,preview:region.rect,start:page_point(state,page,point).unwrap(),revision:state.editor.revision,moved:false,
                        kind:DragKind::Move{id,target:None,in_text:false}});
                }else if state.editor.picture_anchor(&id).is_some(){
                    state.object_drag=Some(Drag{page,original:region.rect,preview:region.rect,start:page_point(state,page,point).unwrap(),revision:state.editor.revision,moved:false,kind:DragKind::FloatMove{id}});
                }
            }
            ObjectKind::Equation{source}=>{
                // A click on a slot (a box, a numerator, an exponent…)
                // selects it, with a blue box, for typing into it.
                if state.equation_draft.is_none() {
                    if let Some((fx,fy)) = click_fraction(state,page,&region,point) {
                        if crate::slot_ui::select_at(state,id.clone(),fx,fy) { refresh_overlay(state);return true; }
                    }
                }
                // An inline equation opens for editing in place; one on its
                // own line is selected, and a double-click opens the pane.
                if source.is_some() && state.equation_draft.is_none() {
                    state.selected_object=None;
                    state.inline_equation_request=Some(Some(id));
                    refresh_overlay(state);return true;
                }
                state.selected_object=Some(SelectedObject::Equation(id));
                if let Some((paragraph,offset))=source {state.editor.set_caret(NodeId::from(paragraph.as_str()),*offset);state.editor.extend_selection(NodeId::from(paragraph.as_str()),*offset+editor::INLINE_OBJECT_MARKER.len());}
                state.action_status=Some("Double-click the equation, or choose Edit equation.".into());
            }
            _=>{}
        }
        refresh_overlay(state);return true;
    }
    state.selected_object=None;
    false
}
/// The picture resized by dragging `handle` to `p`. A corner remains
/// anchored at its opposite corner; Shift temporarily unlocks the aspect
/// ratio, and normal dragging uses a projected uniform scale (no drift). An
/// edge handle moves only its edge; the opposite edge stays.
fn resized(original:Rect,p:Point,handle:Handle,unlock:bool)->Rect {
    let min=MM_TO_PT; let max=1000.0*MM_TO_PT;
    let (horizontal,vertical)=handle.sides();
    if !handle.is_corner() {
        let mut rect=original;
        if horizontal!=0 {
            let fixed=if horizontal<0{original.x+original.width}else{original.x};
            rect.width=((p.x-fixed)*horizontal as f32).clamp(min,max);
            rect.x=if horizontal<0{fixed-rect.width}else{fixed};
        } else {
            let fixed=if vertical<0{original.y+original.height}else{original.y};
            rect.height=((p.y-fixed)*vertical as f32).clamp(min,max);
            rect.y=if vertical<0{fixed-rect.height}else{fixed};
        }
        return rect;
    }
    let (sx,sy)=(horizontal as f32,vertical as f32);
    let opposite=Point::new(if sx<0.0{original.x+original.width}else{original.x},if sy<0.0{original.y+original.height}else{original.y});
    let requested_w=((p.x-opposite.x)*sx).clamp(min,max);
    let requested_h=((p.y-opposite.y)*sy).clamp(min,max);
    let (w,h)=if unlock{(requested_w,requested_h)}else{
        let scale=(requested_w*original.width+requested_h*original.height)/(original.width*original.width+original.height*original.height).max(0.001);
        let lower=(min/original.width.max(0.001)).max(min/original.height.max(0.001));
        let upper=(max/original.width.max(0.001)).min(max/original.height.max(0.001));
        let scale=scale.clamp(lower,upper.max(lower));(original.width*scale,original.height*scale)
    };
    Rect::new(if sx<0.0{opposite.x-w}else{opposite.x},if sy<0.0{opposite.y-h}else{opposite.y},w,h)
}
pub fn motion(state:&mut CanvasState,point:Point,unlock:bool)->bool {
    let Some(mut drag)=state.object_drag.take() else{return false;};
    let Some(p)=page_point(state,drag.page,point) else{state.object_drag=Some(drag);return true;};
    let dx=p.x-drag.start.x;let dy=p.y-drag.start.y;
    drag.moved|=dx.abs()+dy.abs()>3.0/state.viewport.points_to_pixels().max(0.01);
    match &mut drag.kind {
        DragKind::Resize{handle,..}=>{
            let handle_start=handle.point(drag.original);
            drag.preview=resized(drag.original,Point::new(handle_start.x+dx,handle_start.y+dy),*handle,unlock);
            let hint=if handle.is_corner(){"Shift unlocks proportions; "}else{""};
            state.action_status=Some(format!("Picture: {:.1} × {:.1} mm — {hint}Escape cancels",drag.preview.width/MM_TO_PT,drag.preview.height/MM_TO_PT));
        }
        DragKind::Rotate{center,degrees,..}=>{
            *degrees=crate::picture_chrome::rotation_degrees(*center,drag.start,p,unlock);
            state.action_status=Some(format!("Rotate: {:.0}° — Shift snaps to {:.0}°; Escape cancels",*degrees,crate::picture_chrome::ROTATE_SNAP_DEG));
        }
        DragKind::FloatMove{..}=>{
            drag.preview=on_page(state,drag.page,Rect::new(drag.original.x+dx,drag.original.y+dy,drag.original.width,drag.original.height));
            state.action_status=Some("Release to place the picture here. Escape cancels; the paragraph anchor is retained.".into());
        }
        DragKind::Move{target,in_text,..}=>{
            *in_text=unlock;
            *target=state.viewport.hit_test(&state.vector_document,point).map(|hit|DocumentPosition::new(NodeId::from(hit.node_id.as_str()),hit.byte_offset));
            let moved=Rect::new(drag.original.x+dx,drag.original.y+dy,drag.original.width,drag.original.height);
            drag.preview=if unlock{moved}else{on_page(state,drag.page,moved)};
            state.action_status=Some(if unlock{"Release to move the picture to the blue insertion point in the text. Escape cancels.".into()}
                else{"Release to place the picture here; text wraps around it (Shift: move within the text). Escape cancels.".into()});
        }
        DragKind::Column{info,widths}=>{
            let column=(info.column_index+info.column_span).saturating_sub(1);let original=&info.column_widths_pt;
            if column<original.len(){
                let min=2.0*MM_TO_PT;
                let dx=if column+1==original.len() {
                    let boundary=drag.original.max_x()+dx;
                    let margins=state.vector_document.pages.get(drag.page).and_then(|page|page.margin_rect);
                    margins.map_or(dx,|m|crate::table_snap::edge(boundary,[m.x,m.max_x()],crate::table_snap::TOLERANCE_DIP*table_ui::Metrics::of(state).s/state.viewport.points_to_pixels().max(0.01))-drag.original.max_x())
                }else{dx};
                let delta=if column+1<original.len(){dx.clamp(min-original[column],(original[column+1]-min).max(min-original[column]))}else{dx.clamp(min-original[column],500.0*MM_TO_PT-original[column])};
                *widths=original.clone();widths[column]+=delta;if column+1<widths.len(){widths[column+1]-=delta;}
                drag.preview.width=(drag.original.width+delta).max(min);
                state.action_status=Some(format!("Column {}: {:.1} mm — release to apply",column+1,widths[column]/MM_TO_PT));
            }
        }
        DragKind::Row{info,height}=>{
            *height=(info.row_height_pt+dy).clamp(2.0*MM_TO_PT,500.0*MM_TO_PT);
            drag.preview.height=*height;
            state.action_status=Some(format!("Minimum row height: {:.1} mm — text may require more room",*height/MM_TO_PT));
        }
    }
    state.object_drag=Some(drag);refresh_overlay(state);true
}
pub fn up(state:&mut CanvasState)->Result<(),String> {
    let Some(drag)=state.object_drag.take() else{return Ok(());};
    if !drag.moved {refresh_overlay(state);return Ok(());}
    if drag.revision!=state.editor.revision {refresh_overlay(state);return Err("The document changed during the drag; no resize was applied.".into());}
    let done=match &drag.kind {
        DragKind::Resize{..}=>Some(format!("Picture resized to {:.1} × {:.1} mm",drag.preview.width/MM_TO_PT,drag.preview.height/MM_TO_PT)),
        DragKind::Rotate{degrees,..}=>Some(format!("Picture rotated by {degrees:.0}°")),
        _=>None,
    };
    let result=match drag.kind {
        DragKind::Resize{id,..}=>state.editor.resize_picture(&id,drag.preview.width/MM_TO_PT,drag.preview.height/MM_TO_PT),
        DragKind::Rotate{id,degrees,..}=>if degrees.abs()<MIN_TURN_DEG{Ok(())}else{state.editor.rotate_picture(&id,degrees)},
        DragKind::FloatMove{id}=>state.editor.move_floating_picture(&id,(drag.preview.x-drag.original.x)/MM_TO_PT,(drag.preview.y-drag.original.y)/MM_TO_PT),
        DragKind::Move{id,target,in_text:true}=>match target{Some(target)=>state.editor.move_inline_picture(&id,target),None=>Ok(())},
        DragKind::Move{id,target,in_text:false}=>float_at(state,&id,drag.page,drag.preview,target),
        DragKind::Column{info,widths}=>state.editor.resize_table_grid(&NodeId::from(info.table_id.as_str()),widths.into_iter().map(|w|w/MM_TO_PT).collect()),
        DragKind::Row{info,height}=>state.editor.resize_table_row(&NodeId::from(info.table_id.as_str()),info.row_index,height/MM_TO_PT),
    };
    if state.editor.revision!=drag.revision{
        submit_layout(state);
        if let Some(message)=done{state.action_status=Some(message);}
    }
    refresh_overlay(state);result
}
/// `rect` (page points) kept on its page, so a dragged picture cannot be
/// dropped where nobody can reach it.
fn on_page(state:&CanvasState,page:usize,rect:Rect)->Rect {
    let Some(bounds)=state.viewport.page_rect_px(&state.vector_document,page) else{return rect;};
    let scale=state.viewport.points_to_pixels().max(0.01);
    let (width,height)=(bounds.width/scale,bounds.height/scale);
    Rect::new(rect.x.clamp(0.0,(width-rect.width).max(0.0)),rect.y.clamp(0.0,(height-rect.height).max(0.0)),rect.width,rect.height)
}
/// The top-level body paragraph a picture dropped at `top` (page points) on
/// `page` is anchored to: the last one starting above it on that page, else
/// the first one on the page.
pub(crate) fn anchor_paragraph(state:&CanvasState,page:usize,top:f32)->Option<NodeId> {
    let body=|id:&str|state.editor.document.sections.iter().any(|s|s.blocks.iter().any(|b|matches!(b,model::Block::Paragraph(p)if p.id.0==id)));
    let mut starts:Vec<(&str,f32)>=Vec::new();
    for region in &state.vector_document.pages.get(page)?.hit_regions {
        if !body(&region.node_id) {continue;}
        match starts.iter_mut().find(|(id,_)|*id==region.node_id) {
            Some(start)=>start.1=start.1.min(region.rect.y),
            None=>starts.push((region.node_id.as_str(),region.rect.y)),
        }
    }
    let above=starts.iter().filter(|(_,y)|*y<=top).max_by(|a,b|a.1.total_cmp(&b.1));
    let chosen=above.or_else(||starts.iter().min_by(|a,b|a.1.total_cmp(&b.1)))?;
    Some(NodeId::from(chosen.0))
}
/// Drops an inline picture where it was freely moved: it floats there,
/// anchored to the paragraph above, in one undo step. Pictures that cannot
/// float (in tables, headers or multi-column sections) move within the text
/// to `fallback` instead.
fn float_at(state:&mut CanvasState,id:&NodeId,page:usize,rect:Rect,fallback:Option<DocumentPosition>)->Result<(),String> {
    let floated=anchor_paragraph(state,page,rect.y).ok_or_else(||"Drop the picture on a page with text to anchor it to.".to_string()).and_then(|paragraph|{
        let anchor=ImageAnchor{paragraph,reference:AnchorReference::Page,x_mm:rect.x/MM_TO_PT,y_mm:rect.y/MM_TO_PT,gap_mm:FREE_PICTURE_GAP_MM,locked:false};
        state.editor.position_picture(id,anchor,FREE_PICTURE_WRAP)
    });
    match (floated,fallback) {
        (Ok(()),_)=>{state.action_status=None;Ok(())},
        (Err(reason),Some(target))=>{state.editor.move_inline_picture(id,target)?;state.action_status=Some(format!("Moved within the text: {reason}"));Ok(())},
        (Err(reason),None)=>Err(reason),
    }
}
pub fn cancel_drag(state:&mut CanvasState) {
    if state.object_drag.take().is_some(){state.action_status=Some("Drag cancelled — document unchanged".into());refresh_overlay(state);}
}
pub fn double_click(state:&mut CanvasState,p:Point) {
    if crate::equation_ui::previewing(state)||state.display_generation!=Some(state.layout_generation){return;}
    if crate::footnote_ui::double_click(state,p){return;}
    if let Some((page,region))=picture_or_equation(state,p){
        if matches!(region.kind,ObjectKind::Equation{source:Some(_)})&&state.equation_draft.is_none(){state.caret_page_hint=Some(page);state.inline_equation_request=Some(Some(NodeId::from(region.node_id.as_str())));}
        else if matches!(region.kind,ObjectKind::Equation{..}){state.caret_page_hint=Some(page);state.pending_equation=Some(NodeId::from(region.node_id.as_str()));}else if matches!(region.kind,ObjectKind::Picture{..}){state.selected_object=Some(SelectedObject::Picture(NodeId::from(region.node_id.as_str())));}
        return;
    }
    for page in state.viewport.visible_pages(&state.vector_document){
        let Some(point)=page_point(state,page,p)else{continue;};
        let target=state.vector_document.pages[page].object_regions.iter().find_map(|r|match &r.kind{
            ObjectKind::HeaderFooter{section_id,header,variant} if contains(r.rect,point)=>Some((section_id.clone(),*header,*variant)),_=>None});
        if let Some((id,header,variant))=target{
            let variant=HeaderFooterVariant::from_index(variant);
            let revision=state.editor.revision;
            match state.editor.enter_header_footer_at(&NodeId::from(id.as_str()),header,variant){
                Ok(())=>{state.selected_object=None;state.caret_page_hint=Some(page);if state.editor.revision!=revision{submit_layout(state);}refresh_overlay(state);},
                Err(error)=>state.action_status=Some(error),
            }return;
        }
    }
    if let Some(hit) = state.viewport.hit_test(&state.vector_document, p) {
        if state.editor.select_word_at(NodeId::from(hit.node_id.as_str()), hit.byte_offset) {
            state.selected_object = None;
            state.dragging_selection = false;
            state.cell_drag_anchor = None;
            state.drag_pointer = None;
            state.preferred_caret_x_pt = None;
            state.caret_visual_line = Some(hit.visual_line);
            state.caret_page_hint = Some(hit.page_index);
            refresh_overlay(state);
            wake_caret(state);
        }
    }

}
/// Overlay marks owned by direct manipulation: the blue insertion point
/// while an inline picture is dragged. Selection outlines, handles and drag
/// ghosts are chrome (picture_chrome, table_ui); header/footer bands are
/// marked by header_footer_ui.
pub fn adorn(state:&mut CanvasState){
    let mut adornments=Vec::new();
    if let Some(drag)=&state.object_drag{
        if let DragKind::Move{target:Some(target),..}=&drag.kind{
            if drag.moved{let selection=Selection::collapsed(target.clone());let overlay=editor_overlay(&Some(selection),&state.vector_document,None);
                if let Some(caret)=overlay.caret{adornments.push(caret);}}
        }
    }
    state.overlay.adornments=adornments;
    // A selected inline picture or equation also selects its marker in the
    // text (for copy and cut); its outline shows the selection, so the text
    // highlight would only tint the object.
    if matches!(state.selected_object,Some(SelectedObject::Picture(_)|SelectedObject::Equation(_))){state.overlay.selections.clear();}
}
/// 0 cell, 1 row, 2 logical column (including intersecting merged cells), 3 table.
pub fn select_cells(state:&mut CanvasState,mode:u8)->Result<(),String>{
    let context=state.editor.table_cell_context().ok_or("Place the caret in a table first")?;
    let cell_ids=state.editor.table_selection_band(&context.table_id,&context.cell_id,mode)?;
    state.selected_object=Some(SelectedObject::Cells{table_id:context.table_id,cell_ids});refresh_overlay(state);Ok(())
}
pub fn delete_selection(state:&mut CanvasState)->Result<bool,String>{
    let Some(selected)=state.selected_object.clone()else{return Ok(false);};
    match selected{
        SelectedObject::Picture(id)=>{state.editor.delete_picture(&id)?;},
        SelectedObject::Cells{table_id,cell_ids}=>{
            let context=state.editor.table_cell_context();state.editor.clear_cells(&table_id,&cell_ids)?;
            if let Some(c)=context{state.editor.select_cell(&table_id,c.row_index,c.cell_index)?;}
        },
        SelectedObject::Equation(id)=>{state.editor.delete_equation(&id)?;},
    }
    state.selected_object=None;submit_layout(state);refresh_overlay(state);Ok(true)
}
#[cfg(test)] #[path = "../tests/unit/objects_ui.rs"] mod tests;
