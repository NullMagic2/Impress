use super::*;
fn shape(text:&str,mode:LigatureMode)->ShapedRun {
    let font=ligature_fixture::font();
    let mut options=ShapeOptions::default();options.typography.ligatures=mode;
    shape_text(&font,0,text,&options).unwrap()
}
#[test] fn standard_ligatures_use_one_glyph_but_keep_original_cluster() {
    for (text,gid) in [("ff",13),("fi",9),("fl",10),("ffi",11),("ffl",14)] {
        let out=shape(text,LigatureMode::Standard);
        assert_eq!(out.glyphs.len(),1,"{text}");assert_eq!(out.glyphs[0].glyph_id,gid);
        assert_eq!(out.glyphs[0].cluster,0);
        assert_eq!(shape(text,LigatureMode::None).glyphs.len(),text.len());
    }
}
