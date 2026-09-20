//! SHA-256 and BLAKE3, by hand so no new dependency is pulled in. roc's `Crypto`
//! module hands back a `Digest` of 32 bytes; the streaming `Hasher` here simply
//! accumulates the bytes and hashes them at `finish`, which is pure (so `finish` twice
//! gives the same digest) and produces the same result as a true streaming hash.

// ---- SHA-256 ------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(message: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    // Padding: 0x80, then zeros, then the 64-bit big-endian bit length.
    let mut data = message.to_vec();
    let bit_len = (message.len() as u64) * 8;
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());

    for block in data.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA256_K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6]; v[6] = v[5]; v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2]; v[2] = v[1]; v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ---- BLAKE3 -------------------------------------------------------------------------
//
// The reference algorithm (no SIMD, no multithreading): the input is split into 1 KiB
// chunks, each chunk compresses its 64-byte blocks in a chain, and the chunk chaining
// values are combined by a binary tree of parent nodes.

const BLAKE3_IV: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];
const MSG_PERMUTATION: [usize; 16] = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];
const CHUNK_START: u32 = 1 << 0;
const CHUNK_END: u32 = 1 << 1;
const PARENT: u32 = 1 << 2;
const ROOT: u32 = 1 << 3;

fn g(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, mx: u32, my: u32) {
    state[a] = state[a].wrapping_add(state[b]).wrapping_add(mx);
    state[d] = (state[d] ^ state[a]).rotate_right(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(12);
    state[a] = state[a].wrapping_add(state[b]).wrapping_add(my);
    state[d] = (state[d] ^ state[a]).rotate_right(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(7);
}

fn round(state: &mut [u32; 16], m: &[u32; 16]) {
    g(state, 0, 4, 8, 12, m[0], m[1]);
    g(state, 1, 5, 9, 13, m[2], m[3]);
    g(state, 2, 6, 10, 14, m[4], m[5]);
    g(state, 3, 7, 11, 15, m[6], m[7]);
    g(state, 0, 5, 10, 15, m[8], m[9]);
    g(state, 1, 6, 11, 12, m[10], m[11]);
    g(state, 2, 7, 8, 13, m[12], m[13]);
    g(state, 3, 4, 9, 14, m[14], m[15]);
}

fn compress(cv: &[u32; 8], block: &[u32; 16], counter: u64, block_len: u32, flags: u32) -> [u32; 16] {
    let mut state = [
        cv[0], cv[1], cv[2], cv[3], cv[4], cv[5], cv[6], cv[7],
        BLAKE3_IV[0], BLAKE3_IV[1], BLAKE3_IV[2], BLAKE3_IV[3],
        counter as u32, (counter >> 32) as u32, block_len, flags,
    ];
    let mut m = *block;
    for r in 0..7 {
        round(&mut state, &m);
        if r < 6 {
            let mut permuted = [0u32; 16];
            for i in 0..16 {
                permuted[i] = m[MSG_PERMUTATION[i]];
            }
            m = permuted;
        }
    }
    for i in 0..8 {
        state[i] ^= state[i + 8];
        state[i + 8] ^= cv[i];
    }
    state
}

fn words_from_block(block: &[u8]) -> [u32; 16] {
    let mut m = [0u32; 16];
    for i in 0..16 {
        let mut b = [0u8; 4];
        let start = i * 4;
        for j in 0..4 {
            b[j] = block.get(start + j).copied().unwrap_or(0);
        }
        m[i] = u32::from_le_bytes(b);
    }
    m
}

/// The chaining value of one chunk (up to 1024 bytes), given its chunk index.
fn chunk_cv(chunk: &[u8], chunk_counter: u64) -> [u32; 8] {
    let mut cv = BLAKE3_IV;
    let blocks: Vec<&[u8]> = if chunk.is_empty() { vec![&[][..]] } else { chunk.chunks(64).collect() };
    let n = blocks.len();
    for (i, block) in blocks.iter().enumerate() {
        let mut flags = 0;
        if i == 0 { flags |= CHUNK_START; }
        if i == n - 1 { flags |= CHUNK_END; }
        let m = words_from_block(block);
        let out = compress(&cv, &m, chunk_counter, block.len() as u32, flags);
        cv = [out[0], out[1], out[2], out[3], out[4], out[5], out[6], out[7]];
    }
    cv
}

fn parent_cv(left: &[u32; 8], right: &[u32; 8]) -> [u32; 8] {
    let mut block = [0u32; 16];
    block[..8].copy_from_slice(left);
    block[8..].copy_from_slice(right);
    let out = compress(&BLAKE3_IV, &block, 0, 64, PARENT);
    [out[0], out[1], out[2], out[3], out[4], out[5], out[6], out[7]]
}

pub fn blake3(message: &[u8]) -> [u8; 32] {
    // Split into 1024-byte chunks, each with its own counter, then combine left to
    // right as a binary tree. The single-chunk case compresses the last block with the
    // ROOT flag; the multi-chunk case sets ROOT on the final parent compression.
    let chunks: Vec<&[u8]> = if message.is_empty() { vec![&[][..]] } else { message.chunks(1024).collect() };

    if chunks.len() == 1 {
        // Recompute the root: same as chunk_cv but the last block carries ROOT.
        let chunk = chunks[0];
        let mut cv = BLAKE3_IV;
        let blocks: Vec<&[u8]> = if chunk.is_empty() { vec![&[][..]] } else { chunk.chunks(64).collect() };
        let n = blocks.len();
        let mut root_words = [0u32; 16];
        for (i, block) in blocks.iter().enumerate() {
            let mut flags = 0;
            if i == 0 { flags |= CHUNK_START; }
            if i == n - 1 { flags |= CHUNK_END | ROOT; }
            let m = words_from_block(block);
            let out = compress(&cv, &m, 0, block.len() as u32, flags);
            if i == n - 1 {
                root_words = out;
            } else {
                cv = [out[0], out[1], out[2], out[3], out[4], out[5], out[6], out[7]];
            }
        }
        return words_to_bytes(&root_words[..8]);
    }

    // Chunk chaining values, then a left-to-right tree. The root parent uses a ROOT
    // compression to produce the output.
    let cvs: Vec<[u32; 8]> = chunks.iter().enumerate().map(|(i, c)| chunk_cv(c, i as u64)).collect();
    let root = root_from_cvs(&cvs);
    root
}

/// Combine chunk CVs into the root output bytes. Builds the tree left to right,
/// compressing the topmost parent with the ROOT flag.
fn root_from_cvs(cvs: &[[u32; 8]]) -> [u8; 32] {
    fn combine(cvs: &[[u32; 8]], is_root: bool) -> ([u32; 8], [u32; 16]) {
        if cvs.len() == 1 {
            // A single CV that is the root would have been produced with ROOT already
            // by the caller; this path is only reached for internal subtrees.
            return (cvs[0], [0u32; 16]);
        }
        // Largest power of two strictly less than len is the left subtree size.
        let mut left_len = 1;
        while left_len * 2 < cvs.len() {
            left_len *= 2;
        }
        let left = subtree_cv(&cvs[..left_len]);
        let right = subtree_cv(&cvs[left_len..]);
        if is_root {
            let mut block = [0u32; 16];
            block[..8].copy_from_slice(&left);
            block[8..].copy_from_slice(&right);
            let out = compress(&BLAKE3_IV, &block, 0, 64, PARENT | ROOT);
            (parent_cv(&left, &right), out)
        } else {
            (parent_cv(&left, &right), [0u32; 16])
        }
    }
    let (_, root_words) = combine(cvs, true);
    words_to_bytes(&root_words[..8])
}

/// The chaining value of a subtree of chunk CVs (never the root).
fn subtree_cv(cvs: &[[u32; 8]]) -> [u32; 8] {
    if cvs.len() == 1 {
        return cvs[0];
    }
    let mut left_len = 1;
    while left_len * 2 < cvs.len() {
        left_len *= 2;
    }
    let left = subtree_cv(&cvs[..left_len]);
    let right = subtree_cv(&cvs[left_len..]);
    parent_cv(&left, &right)
}

fn words_to_bytes(words: &[u32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, w) in words.iter().enumerate().take(8) {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

pub fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sha256_abc() {
        assert_eq!(to_hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(to_hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }
    #[test]
    fn blake3_abc() {
        assert_eq!(to_hex(&blake3(b"abc")), "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85");
        assert_eq!(to_hex(&blake3(b"")), "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262");
    }
}
