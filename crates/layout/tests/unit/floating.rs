use super::*;
fn picture(wrap:ImageWrap)->Floating{let rgba=[0,0,0,0,0,0,0,255,0,0,0,0];let raster=Arc::new(render::RasterImage::from_rgba(3,1,&rgba).unwrap());let mut f=Floating::image(NodeId::new(),model::ImageAnchor{paragraph:NodeId::new(),reference:AnchorReference::Page,x_mm:0.0,y_mm:0.0,gap_mm:0.0,locked:false},wrap,Rect::new(0.0,0.0,30.0,10.0));f.rows=Arc::new(alpha_rows(&raster));f.raster=Some(raster);f}
#[test]fn tight_follows_alpha_not_rectangle(){assert_eq!(picture(ImageWrap::Tight).blocked(0.0,5.0),Some((10.0,20.0)));}
