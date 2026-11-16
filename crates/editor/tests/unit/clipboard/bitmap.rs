use super::*;
#[test]fn malformed_input_never_allocates_huge_image(){let b=Bitmap{width:1,height:1,rgba:vec![0;4]};let mut bytes=b.dib_v5().unwrap();bytes[4..8].copy_from_slice(&i32::MAX.to_le_bytes());assert!(decode_dib(&bytes).is_err());assert!(decode_dib(&bytes[..20]).is_err());}
