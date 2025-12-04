//! Cryptographic utilities for WebSocket

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

fn rol(v: u32, s: u32) -> u32 {
    v.rotate_left(s)
}

/// SHA1 hash implementation for WebSocket handshake
pub fn sha1(input: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (input.len() as u64) * 8;

    let mut msg = Vec::from(input);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    for i in (0..8).rev() {
        msg.push(((bit_len >> (i * 8)) & 0xff) as u8);
    }

    let mut w = [0u32; 80];
    for chunk in msg.chunks(64) {
        for (i, item) in w.iter_mut().enumerate().take(16) {
            let j = i * 4;
            *item = ((chunk[j] as u32) << 24)
                | ((chunk[j + 1] as u32) << 16)
                | ((chunk[j + 2] as u32) << 8)
                | (chunk[j + 3] as u32);
        }
        for i in 16..80 {
            w[i] = rol(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = if i < 20 {
                ((b & c) | ((!b) & d), 0x5A827999)
            } else if i < 40 {
                (b ^ c ^ d, 0x6ED9EBA1)
            } else if i < 60 {
                ((b & c) | (b & d) | (c & d), 0x8F1BBCDC)
            } else {
                (b ^ c ^ d, 0xCA62C1D6)
            };
            let temp = rol(a, 5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = rol(b, 30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

/// Base64 encoding for WebSocket accept key
pub fn base64_encode(src: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= src.len() {
        let a = src[i] as u32;
        let b = src[i + 1] as u32;
        let c = src[i + 2] as u32;
        let triple = (a << 16) | (b << 8) | c;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        out.push(TABLE[(triple & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = src.len() - i;
    if rem == 1 {
        let a = src[i] as u32;
        let triple = a << 16;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let a = src[i] as u32;
        let b = src[i + 1] as u32;
        let triple = (a << 16) | (b << 8);
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}
