//! Wheel input modifies only the viewport. Fractional wheel notches are preserved.
//! Scrolling glides: each notch adds to a distance the UI timer covers over a
//! few frames, instead of jumping the page in one.
use crate::*;
/// Share of the remaining glide covered on each 16 ms frame; a notch settles
/// in about 120 ms, and quick notches add up rather than queue.
const GLIDE_SHARE: f32 = 0.35;
/// Smallest step of a glide, in pixels, so it always ends.
const GLIDE_MIN_STEP: f32 = 1.0;
#[derive(Clone,Copy,Debug)]
pub struct Input {pub rotation:i32,pub horizontal:bool,pub shift:bool,pub control:bool,pub units:u32,pub page_scroll:bool}
pub fn distance(input:Input,dpi:f32,page:f32)->f32 {
    let unit=if input.page_scroll {page.max(1.0)*0.9} else {input.units.min(100) as f32*18.0*dpi.max(72.0)/96.0};
    input.rotation as f32/120.0*unit
}
pub fn apply(state:&mut CanvasState,input:Input){
    let document=Arc::clone(&state.vector_document);
    if input.control && !input.horizontal {
        let anchor=Point::new(state.viewport.viewport_px.width*0.5,state.viewport.viewport_px.height*0.5);
        let zoom=state.viewport.zoom*1.1_f32.powf(input.rotation as f32/120.0);
        state.viewport.zoom_at(anchor,zoom,&document);
    }else{
        let horizontal=input.horizontal||input.shift;
        let extent=if horizontal{state.viewport.viewport_px.width}else{state.viewport.viewport_px.height};
        let amount=distance(input,state.viewport.dpi,extent);
        // Native horizontal positive delta goes right. Vertical positive goes up.
        let amount=if input.horizontal{amount}else{-amount};
        if horizontal {state.wheel_glide.0+=amount;} else {state.wheel_glide.1+=amount;}
        glide(state);
    }
    state.needs_present=true;
}
/// One frame of the wheel glide. Returns true when the view moved.
pub fn glide(state:&mut CanvasState)->bool{
    let (x,y)=state.wheel_glide;
    if x==0.0 && y==0.0 {return false;}
    let (dx,dy)=(glide_step(x),glide_step(y));
    state.wheel_glide=(x-dx,y-dy);
    let document=Arc::clone(&state.vector_document);
    let before=state.viewport.scroll_px;
    state.viewport.scroll_by(dx,dy,&document);
    // At an end of the document the rest of the glide has nowhere to go.
    if state.viewport.scroll_px==before {state.wheel_glide=(0.0,0.0);return false;}
    state.needs_present=true;
    true
}
fn glide_step(remaining:f32)->f32{
    if remaining.abs()<=GLIDE_MIN_STEP {return remaining;}
    remaining.signum()*(remaining.abs()*GLIDE_SHARE).max(GLIDE_MIN_STEP)
}
