use super::*;
use render::{Size, VectorTextRun};

fn typing_page(length: usize) -> Arc<VectorDocument> { typing_doc(length, 1) }

fn typing_doc(length: usize, pages: usize) -> Arc<VectorDocument> {
    let outline = Arc::new(VectorPath { verbs: vec![
        PathVerb::MoveTo(render::Point::new(0.0, 0.0)),
        PathVerb::LineTo(render::Point::new(4.0, 1.0)),
        PathVerb::LineTo(render::Point::new(2.0, 10.0)), PathVerb::Close,
    ] });
    let mut all = Vec::new();
    for _ in 0..pages {
        let mut page = VectorPage::new(Size::new(600.0, 800.0));
        for line in 0..35 {
            let count = if line == 17 { length } else { 70 };
            let glyphs = (0..count).map(|i| render::GlyphVector {
                glyph_id: 1, outline: Arc::clone(&outline), cluster: Some(i),
                transform: Affine2D::translation(40.0 + i as f32 * 7.0, 40.0 + line as f32 * 18.0),
            }).collect();
            page.commands.push(DrawCommand::TextRun(Arc::new(VectorTextRun {
                actual_text: None, source: None, font_family: "Test".into(), font_size_pt: 11.0,
                fill: ScenePaint::Solid(Color::BLACK), glyphs,
                bounds: Rect::new(40.0, 40.0 + line as f32 * 18.0, count as f32 * 7.0, 11.0),
                advance_pt: count as f32 * 7.0,
            })));
        }
        all.push(page);
    }
    Arc::new(VectorDocument { pages: all, ..Default::default() })
}

fn frame(renderer: &mut WxCanvasRenderer, doc: &Arc<VectorDocument>, viewport: &ViewportState,
    overlay: &CanvasOverlay) -> Vec<u8> {
    let (w, h) = (viewport.viewport_px.width as u32, viewport.viewport_px.height as u32);
    renderer.paint(doc, viewport, overlay, w, h).unwrap().data().to_vec()
}

#[test]
fn incremental_typing_pixels_equal_full_repaint() {
    let mut renderer = WxCanvasRenderer::default();
    let mut viewport = ViewportState::default();
    viewport.viewport_px = Size::new(1200.0, 1000.0);
    let overlay = CanvasOverlay::default();
    for dpi in [96.0, 144.0, 192.0] {
        viewport.dpi = dpi;
        for count in [20, 21, 50, 19, 0, 60] {
            let doc = typing_page(count);
            let incremental = frame(&mut renderer, &doc, &viewport, &overlay);
            let cold = frame(&mut WxCanvasRenderer::default(), &doc, &viewport, &overlay);
            assert!(incremental == cold, "typing/deletion at {dpi} DPI, {count} glyphs");
        }
    }
}

#[test]
fn presented_patches_reproduce_every_frame() {
    let mut renderer = WxCanvasRenderer::default();
    let mut viewport = ViewportState::default();
    viewport.viewport_px = Size::new(1000.0, 900.0);
    let mut presented = vec![0; 1000 * 900 * 4];
    for (step, count) in [20, 21, 30, 0, 10, 10, 11].into_iter().enumerate() {
        let document = typing_page(count);
        let overlay = CanvasOverlay {
            caret: (step % 2 == 0).then_some(render::OverlayRect {
                page_index: 0, rect: Rect::new(55.0 + step as f32 * 9.0, 50.0, 1.0, 13.0),
            }),
            selections: if step == 1 { vec![render::OverlayRect {
                page_index: 0, rect: Rect::new(40.0, 75.0, 30.0, 12.0),
            }] } else { Vec::new() },
            ..Default::default()
        };
        if step >= 5 { viewport.scroll_by(0.0, 40.0, &document); }
        let pixels = frame(&mut renderer, &document, &viewport, &overlay);
        match &renderer.present {
            Present::Full => presented.copy_from_slice(&pixels),
            Present::Patches { shift: (dx, dy), rects } => {
                // Move the previous content like the native bitmap does.
                let previous = presented.clone();
                for y in 0..900i32 { for x in 0..1000i32 {
                    let (sx, sy) = (x - dx, y - dy);
                    if (0..1000).contains(&sx) && (0..900).contains(&sy) {
                        let (t, s) = (((y * 1000 + x) * 4) as usize, ((sy * 1000 + sx) * 4) as usize);
                        presented[t..t + 4].copy_from_slice(&previous[s..s + 4]);
                    }
                }}
                for rect in rects { for row in rect.y as usize..(rect.max_y() as usize).min(900) {
                    let start = (row * 1000 + rect.x as usize) * 4;
                    let end = start + (rect.width as usize).min(1000 - rect.x as usize) * 4;
                    presented[start..end].copy_from_slice(&pixels[start..end]);
                }}
            }
        }
        assert!(presented == pixels, "presented patch missed pixels at step {step}");
    }
}

#[test]
fn overlay_changes_restore_the_page_exactly() {
    // Only the overlay changes between these frames, so each restores just the
    // previous overlay's area from the base; every frame must still match a
    // renderer drawing it from scratch.
    let mut viewport = ViewportState::default();
    viewport.viewport_px = Size::new(1000.0, 900.0);
    let doc = typing_page(20);
    let caret = |x: f32| Some(render::OverlayRect { page_index: 0, rect: Rect::new(x, 50.0, 1.0, 13.0) });
    let selection = |w: f32| vec![render::OverlayRect { page_index: 0, rect: Rect::new(40.0, 75.0, w, 12.0) }];
    let overlays = [
        CanvasOverlay { caret: caret(100.0), ..Default::default() },
        CanvasOverlay::default(),
        CanvasOverlay { caret: caret(160.0), ..Default::default() },
        CanvasOverlay { selections: selection(80.0), ..Default::default() },
        CanvasOverlay { selections: selection(30.0), caret: caret(70.0), ..Default::default() },
        CanvasOverlay::default(),
    ];
    let mut renderer = WxCanvasRenderer::default();
    for overlay in &overlays {
        let reused = frame(&mut renderer, &doc, &viewport, overlay);
        let fresh = frame(&mut WxCanvasRenderer::default(), &doc, &viewport, overlay);
        assert!(reused == fresh, "a restored frame differs from a fresh one");
    }
}

#[test]
fn an_expired_typing_frame_budget_keeps_old_text_visible_until_replacements_are_ready() {
    let before=typing_page(70);
    let mut after=(*before).clone();
    for command in &mut after.pages[0].commands {
        if let DrawCommand::TextRun(run)=command {
            for glyph in &mut Arc::make_mut(run).glyphs { glyph.transform= glyph.transform.then(Affine2D::translation(2.0,0.0)); }
        }
    }
    let after=Arc::new(after);
    let mut view=ViewportState::default();view.viewport_px=Size::new(1000.0,900.0);
    let overlay=CanvasOverlay::default();
    let mut renderer=WxCanvasRenderer::default();renderer.pool=None;
    let previous=frame(&mut renderer,&before,&view,&overlay);
    renderer.budget=Some(Duration::ZERO);
    let pending=frame(&mut renderer,&after,&view,&overlay);
    assert!(renderer.incomplete,"test must exhaust its rasterization budget");
    let ink=|pixels:&[u8]|pixels.chunks_exact(4).filter(|p|p[0]<120&&p[1]<120&&p[2]<120).count();
    assert!(ink(&pending)>ink(&previous)*9/10,"typing must not flash blank paper while tiles catch up");
    renderer.budget=None;
    let complete=frame(&mut renderer,&after,&view,&overlay);
    let expected=frame(&mut WxCanvasRenderer::default(),&after,&view,&overlay);
    assert!(!renderer.incomplete);assert_eq!(complete,expected);
    assert!(renderer.pages.iter().all(|p|p.stale.is_empty()));
}

#[test]
fn a_glyph_crossing_its_cell_clip_cannot_paint_into_the_next_cell() {
    let outline=Arc::new(VectorPath{verbs:vec![PathVerb::MoveTo(render::Point::new(0.0,0.0)),
        PathVerb::LineTo(render::Point::new(40.0,0.0)),PathVerb::LineTo(render::Point::new(40.0,10.0)),
        PathVerb::LineTo(render::Point::new(0.0,10.0)),PathVerb::Close]});
    let glyph=render::GlyphVector{glyph_id:1,outline,cluster:Some(0),transform:Affine2D::translation(10.0,10.0)};
    let command=DrawCommand::TextRun(Arc::new(VectorTextRun{actual_text:Some("W".into()),source:None,font_family:"Test".into(),
        font_size_pt:11.0,fill:ScenePaint::Solid(Color::BLACK),glyphs:vec![glyph],bounds:Rect::new(10.0,10.0,40.0,10.0),advance_pt:40.0}))
        .clipped(Rect::new(10.0,10.0,20.0,10.0));
    let mut renderer=WxCanvasRenderer::default();let bounds=renderer.command_bounds(&command);
    let mut target=Pixmap::new(64,32).unwrap();target.fill(tiny_skia::Color::WHITE);
    let raster=Raster{glyphs:&renderer.glyphs,images:&renderer.images,scale:1.0,tile:256};
    raster.draw_command(&mut target,&mut HashMap::new(),&command,bounds,Affine2D::IDENTITY,Rect::new(0.0,0.0,64.0,32.0));
    assert_eq!(target.pixel(15,15).unwrap().red(),0);
    assert_eq!(target.pixel(35,15).unwrap().red(),255,"clipped glyph ink must not bleed into adjacent cells");
}
