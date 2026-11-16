//! ECMA-376 font embedding: reverse the GUID's textual bytes and XOR the
//! first 32 bytes twice with that 16-byte key. The operation is symmetric.
pub(crate) fn obfuscate(data: &mut [u8], key: &str) -> Result<(), String> {
    if data.len() < 32 {
        return Err("Embedded font is shorter than 32 bytes".into());
    }
    let key = key.trim_matches(['{', '}']);
    if key.len() != 36 || [8, 13, 18, 23].iter().any(|i| key.as_bytes()[*i] != b'-') {
        return Err("Invalid embedded font key".into());
    }
    let hex: Vec<u8> = key.bytes().filter(|b| *b != b'-').collect();
    if hex.len() != 32 {
        return Err("Invalid embedded font key".into());
    }
    let mut bytes = [0u8; 16];
    for (i, pair) in hex.chunks_exact(2).enumerate() {
        let digit = |b: u8| {
            (b as char)
                .to_digit(16)
                .ok_or_else(|| "Invalid embedded font key".to_owned())
        };
        bytes[15 - i] = (digit(pair[0])? * 16 + digit(pair[1])?) as u8;
    }
    for i in 0..32 {
        data[i] ^= bytes[i % 16];
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_guid_byte_order_and_symmetry() {
        let key = "{00112233-4455-6677-8899-AABBCCDDEEFF}";
        let mut data = vec![0; 40];
        obfuscate(&mut data, key).unwrap();
        assert_eq!(
            &data[..16],
            &[
                255, 238, 221, 204, 187, 170, 153, 136, 119, 102, 85, 68, 51, 34, 17, 0
            ]
        );
        assert_eq!(&data[..16], &data[16..32]);
        assert_eq!(&data[32..], &[0; 8]);
        obfuscate(&mut data, key).unwrap();
        assert_eq!(data, vec![0; 40]);
    }
    #[test]
    fn rejects_bad_key_and_short_data() {
        assert!(obfuscate(&mut [0; 31], "00112233-4455-6677-8899-AABBCCDDEEFF").is_err());
        assert!(obfuscate(&mut [0; 32], "not-a-guid").is_err());
    }
}
