use super::*;
#[test] fn invalid_size_is_bounded_before_copying() {
    assert!(RasterImage::from_rgba(u32::MAX,u32::MAX,&[]).is_err());
    assert!(RasterImage::from_rgba(1,1,&[0]).is_err());
    assert!(decode_raster_cached(b"not an image").is_err());
}
