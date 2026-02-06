
/// Convert claw blob data to git blob object bytes.
/// Git blob format: "blob <size>\0<data>"
pub fn to_git_blob(data: &[u8]) -> Vec<u8> {
    let header = format!("blob {}\0", data.len());
    let mut result = Vec::with_capacity(header.len() + data.len());
    result.extend_from_slice(header.as_bytes());
    result.extend_from_slice(data);
    result
}

/// Compute the SHA-1 hash of git object bytes.
pub fn git_sha1(data: &[u8]) -> [u8; 20] {
    
    // Simple SHA-1 implementation using the sha1 algorithm
    // We implement a minimal SHA-1 here to avoid pulling in another dependency
    sha1_hash(data)
}

fn sha1_hash(data: &[u8]) -> [u8; 20] {
    // SHA-1 constants
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (data.len() as u64) * 8;

    // Pad message
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit block
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut result = [0u8; 20];
    result[0..4].copy_from_slice(&h0.to_be_bytes());
    result[4..8].copy_from_slice(&h1.to_be_bytes());
    result[8..12].copy_from_slice(&h2.to_be_bytes());
    result[12..16].copy_from_slice(&h3.to_be_bytes());
    result[16..20].copy_from_slice(&h4.to_be_bytes());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_empty_string() {
        // SHA-1 of empty string
        let hash = sha1_hash(b"");
        let hex = hex::encode(hash);
        assert_eq!(hex, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn git_blob_hash() {
        // "hello" as git blob: "blob 5\0hello" -> known SHA-1
        let git_data = to_git_blob(b"hello");
        let hash = git_sha1(&git_data);
        let hex_str = hex::encode(hash);
        // printf 'hello' | git hash-object --stdin
        assert_eq!(hex_str, "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0");
    }
}
