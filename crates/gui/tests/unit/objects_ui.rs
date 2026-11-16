use super::*;
#[test]fn resize_preserves_ratio_and_opposite_corner(){
    let r=Rect::new(10.0,20.0,80.0,40.0);let n=resized(r,Point::new(170.0,100.0),Handle::BottomRight,false);
    assert!((n.width/n.height-2.0).abs()<0.001);assert_eq!(n.x,r.x);assert_eq!(n.y,r.y);
    let n=resized(r,Point::new(-70.0,-20.0),Handle::TopLeft,false);assert!((n.x+n.width-90.0).abs()<0.001);assert!((n.y+n.height-60.0).abs()<0.001);
}
