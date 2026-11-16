//! RFC1320 MD4 identifiers used by OfficeArt embedded raster records.
pub(super) fn md4(bytes: &[u8]) -> [u8; 16] {
    let mut padded = bytes.to_vec();
    let bits = (bytes.len() as u64) * 8;
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bits.to_le_bytes());
    let mut state = [0x67452301u32, 0xEFCDAB89, 0x98BADCFE, 0x10325476];
    for block in padded.chunks_exact(64) {
        let words: Vec<u32> = block
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect();
        let mut work = state;
        for round in 0..3 {
            for step in 0..16 {
                let [a, b, c, d] = work;
                let (function, index, shift, constant) = match round {
                    0 => ((b & c) | (!b & d), step, [3, 7, 11, 19][step % 4], 0),
                    1 => (
                        (b & c) | (b & d) | (c & d),
                        (step % 4) * 4 + step / 4,
                        [3, 5, 9, 13][step % 4],
                        0x5A827999,
                    ),
                    _ => (
                        b ^ c ^ d,
                        [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15][step],
                        [3, 9, 11, 15][step % 4],
                        0x6ED9EBA1,
                    ),
                };
                let value = a
                    .wrapping_add(function)
                    .wrapping_add(words[index])
                    .wrapping_add(constant)
                    .rotate_left(shift);
                work = [d, value, b, c];
            }
        }
        for (index, value) in work.into_iter().enumerate() {
            state[index] = state[index].wrapping_add(value);
        }
    }
    let mut digest = [0; 16];
    for (index, value) in state.into_iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    digest
}
