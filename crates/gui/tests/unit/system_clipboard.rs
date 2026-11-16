use super::*;
#[test] fn private_payload_ignores_allocator_padding_and_rejects_truncation() {
    let fragment = clipboard::html::import("<p><b>Ελληνικά</b></p>").unwrap();
    let mut bytes = encode(&fragment).unwrap(); let len = bytes.len();
    bytes.extend_from_slice(&[255, 17, 98, 0]); assert_eq!(decode(&bytes).unwrap().plain_text, fragment.plain_text);
    assert!(decode(&bytes[..len - 1]).is_none()); bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes()); assert!(decode(&bytes).is_none());
}
