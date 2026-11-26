use super::*;
#[test]
fn alpha_resampling_does_not_dark_fringe() {
    let rgba = resize_rgba(&[0, 0, 0, 0, 255, 0, 0, 255], 2, 1, 1, 1);
    assert_eq!(&rgba[..3], &[255, 0, 0]);
    assert!((i32::from(rgba[3]) - 128).abs() <= 1);
}

#[test]
fn live_color_bands_preserve_glyphs_and_support_custom_white_and_no_color() {
    for name in ["font-color", "highlight"] {
        let (source,w,h)=asset(name).unwrap();
        let blue=tint_indicator(source,w,h,ColourIndicator::Colour([18,100,210]));
        let white=tint_indicator(source,w,h,ColourIndicator::Colour([255,255,255]));
        let none=tint_indicator(source,w,h,ColourIndicator::NoColour);
        for (old,new) in source.chunks_exact(4).zip(blue.chunks_exact(4)) {
            if old[3]==0 || old[..3].iter().max().unwrap()-old[..3].iter().min().unwrap()<=20 { assert_eq!(old,new,"{name}: neutral glyph changed"); }
        }
        assert!(blue.chunks_exact(4).any(|p|p[2]>190&&p[0]<30&&p[1]>90&&p[3]==255));
        assert!(white.chunks_exact(4).any(|p|p[0]>245&&p[1]>245&&p[2]>245&&p[3]==255));
        assert!(white.chunks_exact(4).any(|p|p[..3]==[143,150,160]&&p[3]>0),"White needs a visible edge");
        let inside=((51*w+12)*4)as usize;
        assert_eq!(none[inside+3],0,"No Color must clear the old band");
        assert_ne!(none,source);
    }
}
#[test]
fn live_color_rasters_refresh_for_custom_colors_and_each_dpi() {
    for side in [18,23,36,54] {
        let blue=colour_raster("font-color",side,side,false,ColourIndicator::Colour([18,100,210])).unwrap();
        let gold=colour_raster("font-color",side,side,false,ColourIndicator::Colour([191,144,0])).unwrap();
        assert_eq!(blue.len(),(side*side*4)as usize);assert_ne!(blue,gold);
        let muted=colour_raster("highlight",side,side,true,ColourIndicator::Colour([18,100,210])).unwrap();
        assert!(muted.chunks_exact(4).all(|p|p[0]==p[1]&&p[1]==p[2]));
    }
}
