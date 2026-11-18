//! Anchored pictures, text boxes and floating tables with line-by-line
//! exclusion layout. Tight wrap follows the alpha silhouette's outside edges
//! (not transparent holes). Page/anchor placement and text flow settle
//! together, with a hard cap.
use super::*;
use model::{AnchorReference,HorizontalFrom,HorizontalPlacement,ImageAnchor,ImageWrap,TablePosition,TableWrap,TextBox,VerticalFrom,VerticalPlacement};
use std::sync::Arc;
/// How a drawing is placed: a picture/text-box anchor, or a table position.
#[derive(Clone)]
enum Place { Image(ImageAnchor), Table(TablePosition) }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Wrap { Square, Tight, TopAndBottom, Behind, Front }
impl From<ImageWrap> for Wrap {
    fn from(w:ImageWrap)->Self{match w{ImageWrap::Tight=>Wrap::Tight,ImageWrap::BehindText=>Wrap::Behind,ImageWrap::InFrontOfText=>Wrap::Front,ImageWrap::TopAndBottom=>Wrap::TopAndBottom,_=>Wrap::Square}}
}
#[derive(Clone)]
struct Floating {
    id:NodeId,
    /// The paragraph whose position decides the page (and the vertical
    /// origin for paragraph-relative placement).
    paragraph:NodeId,place:Place,wrap:Wrap,
    /// Distance from text: left, right, top, bottom (points).
    gaps:[f32;4],
    raster:Option<Arc<render::RasterImage>>,rows:Arc<Vec<Option<(u32,u32)>>>,
    commands:Vec<DrawCommand>,hit_regions:Vec<TextHitRegion>,
    /// Table cell regions (a table float has no region of its own).
    object_regions:Vec<ObjectHitRegion>,
    page:usize,rect:Rect,
}
type Starts=BTreeMap<NodeId,(usize,f32,f32)>;
pub(super) fn has_floats(section:&Section)->bool{
    section.blocks.iter().any(|b|match b{
        Block::Image(i)=>i.anchor.is_some()&&i.wrap!=ImageWrap::Inline,
        Block::Paragraph(p)=>anchored_boxes(&p.content).next().is_some(),
        Block::Table(t)=>floating_table(section,t),
        _=>false,
    })
}
/// Floating tables need a single-column section; elsewhere they stay inline.
pub(super) fn floating_table(section:&Section,table:&Table)->bool{
    table.properties.position.is_some()&&section.properties.column_count<=1
}
fn anchored_boxes(items:&[Inline])->impl Iterator<Item=&TextBox>{
    items.iter().filter_map(|item|match item{
        Inline::TextBox(b) if b.anchor.is_some()&&b.wrap!=ImageWrap::Inline=>Some(b),
        _=>None,
    })
}
fn error(message:impl Into<String>)->LayoutError{LayoutError::RenderPreparation(message.into())}
fn alpha_rows(image:&render::RasterImage)->Vec<Option<(u32,u32)>>{
    (0..image.height).map(|y|{let mut left=None;let mut right=0;for x in 0..image.width{
        if image.pixels[((y*image.width+x)*4+3)as usize]>0{left.get_or_insert(x);right=x+1;}
    }left.map(|x|(x,right))}).collect()
}
impl Floating{
    fn blocked(&self,y:f32,height:f32)->Option<(f32,f32)>{
        if !matches!(self.wrap,Wrap::Square|Wrap::Tight|Wrap::TopAndBottom){return None;}
        let [gl,gr,gt,gb]=self.gaps;
        if y+height<=self.rect.y-gt||y>=self.rect.y+self.rect.height+gb{return None;}
        if self.wrap==Wrap::TopAndBottom{return Some((-1.0e6,1.0e6));}
        if self.wrap==Wrap::Square||self.raster.is_none(){return Some((self.rect.x-gl,self.rect.x+self.rect.width+gr));}
        let raster=self.raster.as_ref()?;
        let a=(((y-gt-self.rect.y)/self.rect.height*raster.height as f32).floor().max(0.0)as usize).min(self.rows.len().saturating_sub(1));
        let b=(((y+height+gb-self.rect.y)/self.rect.height*raster.height as f32).ceil().max(1.0)as usize).min(self.rows.len());
        let mut left=raster.width;let mut right=0;
        for (a,b) in self.rows[a..b.max(a+1).min(self.rows.len())].iter().flatten(){left=left.min(*a);right=right.max(*b);}
        if right==0{return None;}
        Some((self.rect.x+left as f32/raster.width as f32*self.rect.width-gl,self.rect.x+right as f32/raster.width as f32*self.rect.width+gr))
    }
    fn image(id:NodeId,anchor:ImageAnchor,wrap:ImageWrap,rect:Rect)->Self{
        let gap=anchor.gap_mm*MM_TO_PT;
        Floating{id,paragraph:anchor.paragraph.clone(),place:Place::Image(anchor),wrap:wrap.into(),gaps:[gap;4],
            raster:None,rows:Arc::new(Vec::new()),commands:Vec::new(),hit_regions:Vec::new(),object_regions:Vec::new(),page:0,rect}
    }
}
fn free_span(floats:&[Floating],page:usize,y:f32,height:f32,left:f32,right:f32)->Option<(f32,f32)>{
    let mut spans=vec![(left,right)];
    for picture in floats.iter().filter(|f|f.page==page){if let Some((a,b))=picture.blocked(y,height){
        let mut next=Vec::new();for (l,r) in spans{if b<=l||a>=r{next.push((l,r));}else{if a>l{next.push((l,a.min(r)));}if b<r{next.push((b.max(l),r));}}}spans=next;
    }}
    spans.into_iter().filter(|(a,b)|b-a>=8.0).max_by(|a,b|(a.1-a.0).total_cmp(&(b.1-b.0)))
}
fn clear_y(floats:&[Floating],page:usize,y:f32,h:f32)->f32{
    floats.iter().filter(|f|f.page==page&&f.blocked(y,h).is_some()).map(|f|f.rect.y+f.rect.height+f.gaps[3]).fold(y+h,f32::max)
}
fn position(floats:&mut[Floating],starts:&Starts,geometry:&SectionGeometry)->Result<(),LayoutError>{
    for f in floats{
        let (page,x,y)=starts.get(&f.paragraph).copied().ok_or_else(||error("Floating drawing anchor must be a surviving top-level body paragraph"))?;
        f.page=page;
        match &f.place{
            Place::Image(a)=>{
                a.validate().map_err(error)?;
                f.rect.x=a.x_mm*MM_TO_PT+if a.reference==AnchorReference::Paragraph{x}else{0.0};
                f.rect.y=a.y_mm*MM_TO_PT+if a.reference==AnchorReference::Paragraph{y}else{0.0};
                if f.rect.x<0.0||f.rect.y<0.0||f.rect.x+f.rect.width>geometry.page_width+0.1||f.rect.y+f.rect.height>geometry.page_height+0.1{return Err(error("Floating picture is outside its anchored page. Adjust Position & wrapping."));}
                if matches!(f.wrap,Wrap::Square|Wrap::Tight)&&f.rect.y+f.rect.height>geometry.bottom(page)+0.1{return Err(error("Wrapped drawing intrudes into the footer/footnote area. Move or resize it."));}
            }
            Place::Table(p)=>{
                p.validate().map_err(error)?;
                let rect=table_rect(p,page,y,f.rect.width,f.rect.height,geometry);
                if f.rect.height>geometry.height_at(page)+0.1{return Err(error("A floating table must fit on one page; make it inline to let it break across pages."));}
                f.rect=rect;
            }
        }
    }Ok(())
}
/// Places a floating table of the given size, then keeps it on the page and,
/// because text wraps around it, above the footer/footnote area.
fn table_rect(p:&TablePosition,page:usize,paragraph_y:f32,width:f32,height:f32,g:&SectionGeometry)->Rect{
    let (left,span)=match p.horizontal_from{
        HorizontalFrom::Page=>(0.0,g.page_width),
        HorizontalFrom::Margin|HorizontalFrom::Column=>(g.content_left,g.text_width()),
    };
    // Page numbers are 1-based: the first page is odd, so inside is its left.
    let odd=page%2==0;
    let x=match p.horizontal{
        HorizontalPlacement::Offset(mm)=>left+mm*MM_TO_PT,
        HorizontalPlacement::Left=>left,
        HorizontalPlacement::Center=>left+(span-width)*0.5,
        HorizontalPlacement::Right=>left+span-width,
        HorizontalPlacement::Inside=>if odd{left}else{left+span-width},
        HorizontalPlacement::Outside=>if odd{left+span-width}else{left},
    };
    let (top,extent)=match p.vertical_from{
        VerticalFrom::Page=>(0.0,g.page_height),
        VerticalFrom::Margin=>(g.content_top,g.height_at(page)),
        VerticalFrom::Paragraph=>(paragraph_y,g.bottom(page)-paragraph_y),
    };
    let y=match p.vertical{
        VerticalPlacement::Offset(mm)=>top+mm*MM_TO_PT,
        VerticalPlacement::Top=>top,
        VerticalPlacement::Center=>top+(extent-height)*0.5,
        VerticalPlacement::Bottom=>top+extent-height,
    };
    let x=x.min(g.page_width-width).max(0.0);
    let y=y.min(g.bottom(page)-height).max(0.0);
    Rect::new(x,y,width,height)
}
/// A prepared table drawn at the origin, as one unbreakable floating block.
fn table_float(id:&NodeId,paragraph:NodeId,position:&TablePosition,table:&PreparedTable,g:&SectionGeometry)->Floating{
    let mut page=new_page(g.page_width,g.page_height,false);
    page.commands.clear();
    let mut y=0.0;let mut width=0.0f32;
    for row in &table.rows{
        render_table_row(&mut page,row,0.0,y);
        width=row.cells.iter().fold(width,|w,c|w.max(c.x+c.width));
        y+=row.height;
    }
    let wrap=match position.wrap{TableWrap::Around=>Wrap::Square,TableWrap::TopAndBottom=>Wrap::TopAndBottom};
    Floating{id:id.clone(),paragraph,place:Place::Table(position.clone()),wrap,
        gaps:[position.distance_left_mm*MM_TO_PT,position.distance_right_mm*MM_TO_PT,position.distance_top_mm*MM_TO_PT,position.distance_bottom_mm*MM_TO_PT],
        raster:None,rows:Arc::new(Vec::new()),commands:page.commands,hit_regions:page.hit_regions,object_regions:page.object_regions,
        page:0,rect:Rect::new(0.0,0.0,width.max(1.0),y.max(1.0))}
}
/// The normal prepared pipeline supplies initial anchor positions. Floats are
/// omitted from flow, then paragraphs recompose against page-specific exclusions.
pub(super) fn paginate<T:AsRef<PreparedBlock>>(section:&Section,document:&Document,prepared:&[T],geometry:&SectionGeometry,initial:Option<(usize,f32)>,background:bool,fields:&FieldContext,cancel:Option<(&AtomicU64,u64)>)->Result<(Vec<VectorPage>,FlowCursor),LayoutError>{
    if geometry.column_count!=1{return Err(error("Floating-picture wrapping currently requires a single-column section"));}
    let mut floats=Vec::new();let mut flow=Vec::new();let mut flow_prepared=Vec::new();let mut raster_budget=0usize;let mut silhouettes=BTreeMap::<ResourceId,Arc<Vec<Option<(u32,u32)>>>>::new();
    // Floating pictures are not prepared; every other block is, in order.
    let mut prepared_blocks=prepared.iter();
    for (index,b) in section.blocks.iter().enumerate(){if let Block::Image(i)=b{if i.anchor.is_some()&&i.wrap!=ImageWrap::Inline{
        if floats.len()>=128{return Err(error("A section is limited to 128 floating pictures"));}
        let resource=document.resources.get(&i.resource).ok_or_else(||error("Floating image resource is missing"))?;
        let raster=render::decode_raster_cached(&resource.data).map_err(|e|error(e.to_string()))?;
        let width=i.width_mm*MM_TO_PT;let height=i.height_mm*MM_TO_PT;
        if !width.is_finite()||!height.is_finite()||width<=0.0||height<=0.0{return Err(error("Invalid floating image dimensions"));}
        let rows=if let Some(rows)=silhouettes.get(&i.resource){Arc::clone(rows)}else{
            let bytes=raster.pixels.len();
            raster_budget=raster_budget.checked_add(bytes).ok_or_else(||error("Floating raster budget overflow"))?;
            if raster_budget>256*1024*1024{return Err(error("Floating pictures exceed the section's 256 MiB raster budget"));}
            let rows=Arc::new(alpha_rows(&raster));silhouettes.insert(i.resource.clone(),Arc::clone(&rows));rows
        };let mut f=Floating::image(i.id.clone(),i.anchor.clone().expect("checked anchor"),i.wrap,Rect::new(0.0,0.0,width,height));
        f.raster=Some(Arc::clone(&raster));f.rows=rows;f.commands=vec![DrawCommand::Image{rect:Rect::new(0.0,0.0,width,height),image:raster}];
        floats.push(f);continue;
    }}
    let p=prepared_blocks.next().ok_or_else(||error("Floating flow/preparation order mismatch"))?;
    if let (Block::Table(t),PreparedBlock::Table(prepared_table))=(b,p.as_ref()){
        if let (true,Some(position))=(floating_table(section,t),t.properties.position.as_ref()){
            // Anchored to the paragraph after the table (Word's rule), else the one before.
            let paragraph=section.blocks[index+1..].iter().find_map(|b|match b{Block::Paragraph(p)=>Some(p.id.clone()),_=>None})
                .or_else(||section.blocks[..index].iter().rev().find_map(|b|match b{Block::Paragraph(p)=>Some(p.id.clone()),_=>None}));
            if let Some(paragraph)=paragraph{
                if floats.len()>=128{return Err(error("A section is limited to 128 floating drawings and tables"));}
                floats.push(table_float(&t.id,paragraph,position,prepared_table,geometry));continue;
            }
        }
    }
    flow.push(b);flow_prepared.push(p);}
    let mut box_context=WorkerContext{fonts:FontCatalog::for_resources(&document.resources),typst:TypstTypesetter::new()};
    for block in &section.blocks { if let Block::Paragraph(paragraph)=block {
        for boxed in anchored_boxes(&paragraph.content) {
            if floats.len()>=128{return Err(error("A section is limited to 128 floating drawings"));}
            let width=boxed.width_mm*MM_TO_PT;let height=boxed.height_mm*MM_TO_PT;
            if !width.is_finite()||!height.is_finite()||width<=0.0||height<=0.0{return Err(error("Invalid floating text-box dimensions"));}
            let (commands,hit_regions)=prepare_text_box(boxed,&document.styles,&mut box_context,&document.resources,fields)?;
            let mut f=Floating::image(boxed.id.clone(),boxed.anchor.clone().expect("checked anchor"),boxed.wrap,Rect::new(0.0,0.0,width,height));
            f.commands=commands;f.hit_regions=hit_regions;floats.push(f);
        }
    }}
    let mut starts=Starts::new();
    // First iteration has no exclusions; following iterations converge anchors
    // and line wrapping. Each pass runs on the existing background layout worker.
    let (mut pages,mut cursor,first)=flow_pass(&flow,&flow_prepared,document,geometry,initial,background,fields,&[],cancel)?;starts.extend(first);
    let mut stable=false;
    for _ in 0..12{
        check_cancel(cancel)?;position(&mut floats,&starts,geometry)?;
        let prior=floats.iter().map(|f|(f.page,f.rect)).collect::<Vec<_>>();
        let result=flow_pass(&flow,&flow_prepared,document,geometry,initial,background,fields,&floats,cancel)?;
        pages=result.0;cursor=result.1;starts=result.2;position(&mut floats,&starts,geometry)?;
        stable=prior.iter().zip(&floats).all(|((p,r),f)|*p==f.page&&(r.x-f.rect.x).abs()<0.05&&(r.y-f.rect.y).abs()<0.05);
        if stable{break;}
    }
    if !stable{return Err(error("Drawing anchors and text wrapping did not settle in twelve passes; adjust anchor position or drawing size"));}
    for f in floats{
        let page=pages.get_mut(f.page).ok_or_else(||error("Floating drawing page is missing"))?;
        if page.has_paint {
            let commands=f.commands.into_iter().map(|c|c.translated(f.rect.x,f.rect.y));
            if f.wrap==Wrap::Behind{
                let at=usize::from(background&&!page.commands.is_empty());page.commands.splice(at..at,commands);
            }else{page.commands.extend(commands);}
        }
        page.hit_regions.extend(f.hit_regions.into_iter().map(|r|r.translated(f.rect.x,f.rect.y)));
        if matches!(f.place,Place::Table(_)){
            page.object_regions.extend(f.object_regions.into_iter().map(|r|r.translated(f.rect.x,f.rect.y)));
        }else{
            page.object_regions.push(ObjectHitRegion{node_id:f.id.0,rect:f.rect,
                kind:if f.raster.is_some(){ObjectKind::Picture{source:None}}else{ObjectKind::TextBox{source:None}}});
        }
    }
    Ok((pages,cursor))
}
#[allow(clippy::too_many_arguments)]
fn flow_pass<T:AsRef<PreparedBlock>>(flow:&[&Block],prepared:&[T],document:&Document,g:&SectionGeometry,initial:Option<(usize,f32)>,background:bool,fields:&FieldContext,floats:&[Floating],cancel:Option<(&AtomicU64,u64)>)->Result<(Vec<VectorPage>,FlowCursor,Starts),LayoutError>{
    let mut pages=vec![g.page(0, background)];let (column,y)=initial.unwrap_or((0,g.content_top));
    let mut cursor=FlowCursor{page_index:0,column_index:column,y};let mut starts=Starts::new();let mut ctx=WorkerContext{fonts:FontCatalog::for_resources(&document.resources),typst:TypstTypesetter::new()};
    if flow.len()!=prepared.len(){return Err(error("Floating flow/preparation order mismatch"));}
    for (block,p) in flow.iter().zip(prepared){
        check_cancel(cancel)?;
        if pages.len()>10000{return Err(error("Floating layout exceeds 10,000 pages"));}
        match (block,p.as_ref()){
            (Block::Paragraph(paragraph),PreparedBlock::Paragraph{lines,space_before,space_after,left_indent,marker,decoration,keep_lines_together,page_break_before,widow_control,..})=>{
                if *page_break_before&&cursor.y>g.content_top{new_flow_page(&mut pages,&mut cursor,g,background);}
                if let Some(line)=lines.first(){ensure_vertical_space(&mut pages,&mut cursor,g,line.height+*space_before,background);}
                starts.insert(paragraph.id.clone(),(cursor.page_index,g.content_left,cursor.y+*space_before));
                let span_height=lines.iter().map(|l|l.height).sum::<f32>()+*space_before+*space_after;
                let touched=floats.iter().any(|f|f.page==cursor.page_index&&f.blocked(cursor.y,span_height).is_some())||cursor.y+span_height>g.bottom(cursor.page_index)&&floats.iter().any(|f|f.page>cursor.page_index);
                if !touched{paginate_paragraph(lines,*space_before,*space_after,*left_indent,marker.as_ref(),decoration.as_ref(),*keep_lines_together,*widow_control,&mut pages,&mut cursor,g,background)?;}
                else{wrapped_paragraph(paragraph,document,&mut ctx,g,fields,marker.as_ref(),floats,&mut pages,&mut cursor,background,cancel)?;}
            }
            (_,PreparedBlock::PageBreak)=>new_flow_page(&mut pages,&mut cursor,g,background),
            (_,PreparedBlock::ColumnBreak)=>advance_column(&mut pages,&mut cursor,g,background),
            (_,PreparedBlock::Image(i))=>{
                clear_fixed(floats,i.height,&mut pages,&mut cursor,g,background)?;
                let x=g.content_left+match i.alignment{ImageAlignment::Left=>0.0,ImageAlignment::Center=>(g.column_width-i.width)*0.5,ImageAlignment::Right=>g.column_width-i.width};
                let page=&mut pages[cursor.page_index];
                if page.has_paint { page.commands.extend(i.commands.iter().map(|c|c.translated(x,cursor.y))); }
                page.object_regions.push(ObjectHitRegion{node_id:i.id.clone(),rect:Rect::new(x,cursor.y,i.width,i.height),kind:ObjectKind::Picture{source:None}});cursor.y+=i.height+4.0;
            }
            (_,PreparedBlock::Table(t))=>{
                let height=t.rows.iter().map(|r|r.height).sum::<f32>();clear_fixed(floats,height.min(g.column_height()),&mut pages,&mut cursor,g,background)?;
                // A split table cannot share a later page with an exclusion until
                // table fragments themselves support line-level wrap geometry.
                if height>g.bottom(cursor.page_index)-cursor.y&&floats.iter().any(|f|f.page>cursor.page_index&&matches!(f.wrap,Wrap::Square|Wrap::Tight|Wrap::TopAndBottom)){return Err(error("A table spanning a later floating-drawing page is not supported; move its anchor or keep the table on a separate page"));}
                paginate_table(t,&mut pages,&mut cursor,g,background)?;
            }
            (_,PreparedBlock::Equation{id,fragment,before,after})=>{
                cursor.y+=*before;clear_fixed(floats,fragment.size.height,&mut pages,&mut cursor,g,background)?;let x=g.content_left+((g.column_width-fragment.size.width)*0.5).max(0.0);
                let page = &mut pages[cursor.page_index];
                if page.has_paint { page.commands.extend(fragment.commands.iter().map(|c|c.translated(x,cursor.y))); }
                page.object_regions.push(ObjectHitRegion{node_id:id.clone(),rect:Rect::new(x,cursor.y,fragment.size.width,fragment.size.height),kind:ObjectKind::Equation{source:None}});cursor.y+=fragment.size.height+*after;
            }
            (_,PreparedBlock::Foreign{height})=>{clear_fixed(floats,*height,&mut pages,&mut cursor,g,background)?;cursor.y+=*height;}
            _=>return Err(error("Unexpected floating-flow block preparation")),
        }
    }Ok((pages,cursor,starts))
}
fn clear_fixed(floats:&[Floating],height:f32,pages:&mut Vec<VectorPage>,cursor:&mut FlowCursor,g:&SectionGeometry,background:bool)->Result<(),LayoutError>{
    for _ in 0..130{
        ensure_vertical_space(pages,cursor,g,height,background);
        if !floats.iter().any(|f|f.page==cursor.page_index&&f.blocked(cursor.y,height).is_some()){return Ok(());}
        cursor.y=clear_y(floats,cursor.page_index,cursor.y,height);
    }Err(error("No progress placing a block around floating pictures"))
}
#[allow(clippy::too_many_arguments)]
fn wrapped_paragraph(p:&Paragraph,document:&Document,ctx:&mut WorkerContext,g:&SectionGeometry,fields:&FieldContext,marker:Option<&PreparedMarker>,floats:&[Floating],pages:&mut Vec<VectorPage>,cursor:&mut FlowCursor,background:bool,cancel:Option<(&AtomicU64,u64)>)->Result<(),LayoutError>{
    let props=document.styles.resolve_paragraph_with_direct(p.style.as_ref(),&p.properties);let base=document.styles.resolve_run(p.style.as_ref(),None,&CharacterProperties::default());
    let size=base.font_size_pt.unwrap_or(DEFAULT_FONT_SIZE_PT);let line_height=size*props.line_spacing.unwrap_or(DEFAULT_LINE_HEIGHT_EM);
    let mut hard=collect_inline_tokens(p,&document.styles,ctx,&document.resources,&base,fields)?;let text=paragraph_plain_text(p);apply_bidi_levels(&mut hard,&text,&ctx.fonts)?;
    let left_indent=(props.left_indent_mm.unwrap_or(0.0)*MM_TO_PT).max(if marker.is_some(){18.0}else{0.0});let right_indent=props.right_indent_mm.unwrap_or(0.0)*MM_TO_PT;
    cursor.y+=props.space_before_pt.unwrap_or(0.0);let mut visual=0;let mut attempts=0;
    for mut line in hard{
        let full_width=(g.column_width-left_indent-right_indent-props.first_line_indent_mm.unwrap_or(0.0).max(0.0)*MM_TO_PT).max(1.0);
        line.tokens=line_fitting::split_overwide_text(line.tokens,full_width);
        if line.tokens.is_empty(){ensure_vertical_space(pages,cursor,g,line_height,background);let l=empty_line(&p.id.0,line.start_offset,visual,line_height,size);emit(&l,g.content_left+left_indent,g.column_width-left_indent-right_indent,g,pages,cursor);visual+=1;continue;}
        let mut start=0;
        while start<line.tokens.len(){
            check_cancel(cancel)?;attempts+=1;if attempts>100000{return Err(error("Floating paragraph exceeded 100,000 line-placement attempts"));}
            let first_indent=if visual==0{props.first_line_indent_mm.unwrap_or(0.0)*MM_TO_PT}else{0.0};
            let minimum=line_height.max(line.tokens[start].ascent+line.tokens[start].descent);
            if minimum>g.height_at(cursor.page_index){return Err(error("Inline object cannot fit on a floating-layout page"));}
            ensure_vertical_space(pages,cursor,g,minimum,background);
            let left=g.content_left+left_indent+first_indent;let right=g.content_left+g.column_width-right_indent;
            let Some((x,r))=free_span(floats,cursor.page_index,cursor.y,minimum,left,right)else{cursor.y=clear_y(floats,cursor.page_index,cursor.y,minimum);continue;};
            let width=r-x;
            // Do not strand an ordinary word beside a float when it will fit
            // below it; long words have already been split at safe boundaries.
            if line.tokens[start].width>width+0.01 && width<right-left-0.01 {
                cursor.y=clear_y(floats,cursor.page_index,cursor.y,minimum);continue;
            }
            let (end,tokens)=line_fitting::next_line(&line.tokens,start,&text,&props,width,0.0,None,&ctx.fonts)?;
            let height=tokens.iter().fold(minimum,|h,t|h.max(t.ascent+t.descent));
            if height>minimum+0.01 {
                if cursor.y+height>g.bottom(cursor.page_index){advance_column(pages,cursor,g,background);continue;}
                if free_span(floats,cursor.page_index,cursor.y,height,left,right).is_none_or(|(a,b)|a>x+0.1||b<r-0.1){cursor.y=clear_y(floats,cursor.page_index,cursor.y,height);continue;}
            }
            let format=document.styles.resolve_formatting(p.style.as_ref());
            let result=vectorize_mixed_line(&tokens,&p.id.0,visual,width,0.0,1.0,props.alignment.unwrap_or(ParagraphAlignment::Left),line_height,0.0,props.hanging_punctuation.unwrap_or(true),&format,&ctx.fonts)?;
            if visual==0 && pages[cursor.page_index].has_paint {if let Some(m)=marker{let mx=x-m.width-m.gap;pages[cursor.page_index].commands.extend(m.commands.iter().map(|c|c.translated(mx,cursor.y+result.ascent)));}}
            emit(&result,x,width,g,pages,cursor);visual+=1;start=end;
        }
    }
    cursor.y+=props.space_after_pt.unwrap_or(size*0.3);Ok(())
}
fn emit(line:&PreparedLine,x:f32,width:f32,g:&SectionGeometry,pages:&mut[VectorPage],cursor:&mut FlowCursor){
    let y=cursor.y+line.ascent;
    let clip=Rect::new(x,g.content_top,width.max(0.0),g.height_at(cursor.page_index));
    let page=&mut pages[cursor.page_index];
    if page.has_paint { page.commands.extend(line.commands.iter().map(|c|c.translated(x,y).clipped(clip))); }
    page.hit_regions.extend(line.hit_regions.iter().filter_map(|r|r.translated(x,y).clipped(clip)));
    cursor.y+=line.height;
}
#[cfg(test)] #[path = "../tests/unit/floating.rs"] mod tests;
