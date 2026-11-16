//! Platform-independent button-face rasterizer. Only Ribbon chrome is rasterized;
//! document text/pages remain vector geometry. No selected state is baked into icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment { Single, First, Middle, Last }
impl Segment {
    pub fn from_name(name: &str) -> Self { match name {
        "first" => Self::First, "middle" => Self::Middle, "last" => Self::Last, _ => Self::Single,
    }}
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State { pub selected: bool, pub hot: bool, pub pressed: bool, pub disabled: bool }
#[derive(Clone, Copy, Debug)]
pub struct Palette { pub outside: [u8;3], pub top: [u8;3], pub bottom: [u8;3], pub border: [u8;3], pub text: [u8;3] }
pub fn palette(state: State, outside: [u8;3]) -> Palette {
    let (top, bottom, border, text) = if state.disabled {
        (outside,outside,outside,[143,150,160])
    } else if state.pressed {
        ([225,232,242],[225,232,242],[202,211,223],[36,48,66])
    } else if state.selected {
        if state.hot { ([231,239,253],[231,239,253],[168,190,232],[29,61,120]) }
        else { ([229,237,252],[229,237,252],[181,200,238],[29,61,120]) }
    } else if state.hot {
        ([238,242,248],[238,242,248],[238,242,248],[36,48,66])
    } else { (outside,outside,outside,[36,48,66]) };
    Palette { outside, top, bottom, border, text }
}

/// Dialog command buttons (OK, Cancel, Format) on the pale chrome: the Ribbon's
/// rounded face with a resting outline, as in the interface reference `.btn`.
/// `primary` is the default button: filled with the interface accent.
pub const DIALOG_CHROME: [u8;3] = [232,236,242];
pub fn dialog_palette(state: State, primary: bool) -> Palette { outlined_palette(state, primary, DIALOG_CHROME) }
/// Slightly raised Styles-pane buttons: a restrained vertical tint gives the
/// actions their reference relief without introducing a heavy bevel.
pub fn pane_palette(state: State, primary: bool) -> Palette {
    let outside=[255,255,255];
    let (top,bottom,border,text)=if state.disabled {
        ([250,251,253],[246,248,251],[223,228,236],[143,150,160])
    }else if primary {
        let (top,bottom)=if state.pressed {([43,79,154],[57,99,183])}
            else if state.hot {([79,121,205],[56,101,192])}
            else {([70,112,199],[56,99,187])};
        (top,bottom,[64,105,186],[255,255,255])
    }else if state.pressed {
        ([221,230,244],[237,242,250],[166,183,207],[36,48,66])
    }else if state.hot {
        ([255,255,255],[235,241,250],[168,190,222],[36,48,66])
    }else {([255,255,255],[246,249,253],[198,210,226],[36,48,66])};
    Palette{outside,top,bottom,border,text}
}
fn outlined_palette(state: State, primary: bool, outside: [u8;3]) -> Palette {
    let (fill, border, text) = if state.disabled {
        ([247,249,251],[223,228,236],[143,150,160])
    } else if primary {
        let fill = if state.pressed {[42,80,156]} else if state.hot {[50,93,178]} else {[58,104,194]};
        (fill,fill,[255,255,255])
    } else if state.pressed {
        ([225,232,242],[169,184,205],[36,48,66])
    } else if state.hot {
        ([238,242,248],[169,184,205],[36,48,66])
    } else { ([255,255,255],[202,211,223],[36,48,66]) };
    Palette { outside, top: fill, bottom: fill, border, text }
}

/// 32-bit opaque BGRA, top down, ready for a BI_RGB DIB. The full control rect
/// is highlighted, including the label. Adjacent segments share their inner seams.
pub fn render(width: u32, height: u32, scale: f32, segment: Segment, colors: Palette) -> Vec<u8> {
    let mut out=vec![0; (width as usize)*(height as usize)*4];
    if width == 0 || height == 0 { return out; }
    let radius=(5.0*scale).min(width.min(height) as f32/2.0);
    let stroke=scale.max(1.0);
    // Omit the left inner border after the first segment: one shared seam,
    // not two outlines plus margins between every pair of buttons.
    let left=if matches!(segment,Segment::Middle|Segment::Last) { -1.0 } else { 0.5+stroke };
    let covers=|px:f32,py:f32| {
        let outer=coverage(px,py,0.5,0.5,width as f32-0.5,height as f32-0.5,radius,segment);
        let inner=coverage(px,py,left,0.5+stroke,width as f32-0.5-stroke,height as f32-0.5-stroke,(radius-stroke).max(0.0),segment).min(outer);
        (outer,inner)
    };
    // Away from the rounded corners the outlines' coverage varies along one
    // axis only, so it is worked out once per column and once per row.
    let margin=(radius+stroke+2.0).ceil() as u32;
    let band=|v:u32,size:u32| v>=margin && v+margin<size;
    let across:Vec<(f32,f32)>=if band(height/2,height) {
        (0..width).map(|x| covers(x as f32+0.5,(height/2) as f32+0.5)).collect()
    } else { Vec::new() };
    let down:Vec<(f32,f32)>=if band(width/2,width) {
        (0..height).map(|y| covers((width/2) as f32+0.5,y as f32+0.5)).collect()
    } else { Vec::new() };
    for y in 0..height {
        let t=(y as f32 / height.saturating_sub(1).max(1) as f32).clamp(0.0,1.0);
        let fill=[0,1,2].map(|c| colors.top[c] as f32*(1.0-t)+colors.bottom[c] as f32*t);
        for x in 0..width {
            let (outer,inner)=if band(y,height) { across[x as usize] }
                else if band(x,width) { down[y as usize] }
                else { covers(x as f32+0.5,y as f32+0.5) };
            let i=((y*width+x)*4) as usize;
            for c in 0..3 {
                let value=colors.outside[c] as f32*(1.0-outer)+colors.border[c] as f32*(outer-inner)+fill[c]*inner;
                out[i+2-c]=value.round().clamp(0.0,255.0) as u8;
            }
            out[i+3]=255;
        }
    }
    out
}

/// Connected tabs have rounded top corners and subtly rounded lower corners.
pub fn render_tab(width:u32,height:u32,scale:f32,state:State)->Vec<u8> {
    let mut out=vec![0;(width as usize)*(height as usize)*4];
    let outside=[232u8,236,242];
    let fill=if state.selected {[255,255,255]} else if state.hot {[223,229,238]} else {outside};
    let border=if state.selected {[223,228,236]} else {fill};
    let radius=5.0*scale;
    let lower_radius=if state.selected {5.0*scale} else {0.0};
    let stroke=scale.max(1.0);
    for y in 0..height { for x in 0..width {
        let px=x as f32+0.5;let py=y as f32+0.5;
        // The active tab flows out into the ribbon: its straight sides sit
        // slightly inboard, then flare toward the full control width below.
        // This is the concave white-to-title transition in the HTML reference.
        let lower_inset=if lower_radius>0.0 && py>height as f32-lower_radius {
            lower_radius-((lower_radius*lower_radius
                -(py-(height as f32-lower_radius)).powi(2)).max(0.0)).sqrt()
        } else { 0.0 };
        let shoulder=if state.selected {lower_radius} else {0.0};
        let left=shoulder-lower_inset;
        let right=width as f32-shoulder+lower_inset;
        let corner=if py<radius && px<left+radius {
                radius-(left+radius-px).hypot(radius-py)
            } else if py<radius && px>right-radius {
                radius-(px-(right-radius)).hypot(radius-py)
            } else if px<left {px-left}
            else if px>right {right-px}
            else {1.0};
        let coverage=(corner+0.5).clamp(0.0,1.0);
        // Trace the small corner arcs so the 5-DIP radius remains legible
        // without restoring a horizontal stripe across the tab top.
        let arc=py<radius && ((px<left+radius || px>right-radius)
            && corner>=-0.5 && corner<stroke);
        let edge=state.selected && ((px-left).abs()<stroke || (px-right).abs()<stroke || arc);
        let color=if edge {border} else {fill};
        let i=((y*width+x)*4) as usize;
        for c in 0..3 {out[i+2-c]=((outside[c] as f32*(1.0-coverage)+color[c] as f32*coverage).round()) as u8;}
        out[i+3]=255;
    }}out
}
fn coverage(x:f32,y:f32,l:f32,t:f32,r:f32,b:f32,rad:f32,segment:Segment)->f32 {
    if r <= l || b <= t { return 0.0; }
    let round_left=matches!(segment,Segment::Single|Segment::First);
    let round_right=matches!(segment,Segment::Single|Segment::Last);
    let radius=if (x < (l+r)*0.5 && round_left)||(x >= (l+r)*0.5 && round_right) { rad } else { 0.0 };
    let qx=(x-(l+r)*0.5).abs()-(r-l)*0.5+radius;
    let qy=(y-(t+b)*0.5).abs()-(b-t)*0.5+radius;
    let distance=qx.max(0.0).hypot(qy.max(0.0))+qx.max(qy).min(0.0)-radius;
    (0.5-distance).clamp(0.0,1.0)
}
/// Composite a transparent icon without altering or highlighting its pixels.
pub fn icon(dst:&mut [u8],width:u32,height:u32,rgba:&[u8],iw:u32,ih:u32,left:i32,top:i32) {
    for y in 0..ih { for x in 0..iw {
        let dx=left+x as i32; let dy=top+y as i32;
        if dx<0||dy<0||dx>=width as i32||dy>=height as i32 { continue; }
        let si=((y*iw+x)*4) as usize; let di=((dy as u32*width+dx as u32)*4) as usize;
        let alpha=rgba[si+3] as u32;
        for c in 0..3 { dst[di+2-c]=((rgba[si+c] as u32*alpha+dst[di+2-c] as u32*(255-alpha)+127)/255) as u8; }
    }}
}
/// A compact trailing chevron for ribbon buttons that open a choices menu.
pub fn dropdown_caret(dst:&mut [u8],width:u32,height:u32,scale:f32,color:[u8;3]) {
    if width==0 || height==0 {return;}
    chevron(dst,width,height,width as f32-9.0*scale,height as f32*0.5,scale,color);
}
/// A large File ribbon dropdown keeps its chevron below the caption.
pub fn large_dropdown_caret(dst:&mut [u8],width:u32,height:u32,scale:f32,color:[u8;3]) {
    if width==0 || height==0 {return;}
    chevron(dst,width,height,width as f32*0.5,height as f32-6.0*scale,scale,color);
}
/// An antialiased down chevron centred at (cx, cy).
fn chevron(dst:&mut [u8],width:u32,height:u32,cx:f32,cy:f32,scale:f32,color:[u8;3]) {
    let points=[(cx-3.0*scale,cy-1.0*scale),(cx,cy+2.0*scale),(cx+3.0*scale,cy-1.0*scale)];
    stroke(dst,width,height,&[(points[0],points[1]),(points[1],points[2])],0.65*scale,color);
}
fn stroke(dst:&mut [u8],width:u32,height:u32,segments:&[((f32,f32),(f32,f32))],half:f32,color:[u8;3]) {
    let distance=|x:f32,y:f32,a:(f32,f32),b:(f32,f32)| {
        let dx=b.0-a.0;let dy=b.1-a.1;
        let t=(((x-a.0)*dx+(y-a.1)*dy)/(dx*dx+dy*dy).max(1e-6)).clamp(0.0,1.0);
        (x-a.0-t*dx).hypot(y-a.1-t*dy)
    };
    let (mut x0,mut y0,mut x1,mut y1)=(f32::MAX,f32::MAX,f32::MIN,f32::MIN);
    for (a,b) in segments { x0=x0.min(a.0).min(b.0); y0=y0.min(a.1).min(b.1); x1=x1.max(a.0).max(b.0); y1=y1.max(a.1).max(b.1); }
    let pad=half+1.0;
    let min_x=(x0-pad).floor().max(0.0) as u32; let max_x=((x1+pad).ceil().max(0.0) as u32).min(width);
    let min_y=(y0-pad).floor().max(0.0) as u32; let max_y=((y1+pad).ceil().max(0.0) as u32).min(height);
    for y in min_y..max_y {for x in min_x..max_x {
        let px=x as f32+0.5;let py=y as f32+0.5;
        let d=segments.iter().map(|(a,b)|distance(px,py,*a,*b)).fold(f32::MAX,f32::min);
        let alpha=((half+0.5-d).clamp(0.0,1.0)*255.0) as u32;
        if alpha==0 {continue;}
        let i=((y*width+x)*4) as usize;
        for c in 0..3 {dst[i+2-c]=((color[c] as u32*alpha+dst[i+2-c] as u32*(255-alpha)+127)/255) as u8;}
    }}
}
/// Word's gallery More mark: a short rule above a down chevron, in the lower
/// half of the gallery's edge strip.
pub fn gallery_more_mark(dst:&mut [u8],width:u32,height:u32,scale:f32,color:[u8;3]) {
    let cx=width as f32*0.5;
    let cy=height as f32*0.68;
    chevron(dst,width,height,cx,cy+1.5*scale,scale,color);
    stroke(dst,width,height,&[((cx-3.0*scale,cy-2.5*scale),(cx+3.0*scale,cy-2.5*scale))],0.6*scale,color);
}
