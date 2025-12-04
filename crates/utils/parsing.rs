//! Parsing utilities for HTTP and WebSocket

extern crate alloc;
use alloc::vec::Vec;

/// Find HTTP header value in request buffer
pub fn find_header_value<'a>(req: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let needle = name.as_bytes();
    let mut i = 0;
    while i + needle.len() < req.len() {
        if req[i..i + needle.len()].eq(needle) {
            let mut j = i + needle.len();
            while j < req.len() && (req[j] == b':' || req[j] == b' ') {
                j += 1;
            }
            let mut k = j;
            while k + 1 < req.len() {
                if req[k] == b'\r' && req[k + 1] == b'\n' {
                    break;
                }
                k += 1;
            }
            return Some(&req[j..k]);
        }
        i += 1;
    }
    None
}

/// Parse WebSocket frame, returns (bytes_consumed, fin, opcode, payload)
pub fn parse_websocket_frame(buf: &[u8]) -> Option<(usize, bool, u8, Vec<u8>)> {
    if buf.len() < 2 {
        return None;
    }
    
    let b1 = buf[0];
    let b2 = buf[1];
    let fin = (b1 & 0x80) != 0;
    let opcode = b1 & 0x0f;
    let masked = (b2 & 0x80) != 0;
    let mut payload_len = (b2 & 0x7f) as usize;
    let mut pos = 2usize;
    
    if payload_len == 126 {
        if buf.len() < pos + 2 {
            return None;
        }
        payload_len = ((buf[pos] as usize) << 8) | (buf[pos + 1] as usize);
        pos += 2;
    } else if payload_len == 127 {
        if buf.len() < pos + 8 {
            return None;
        }
        payload_len = 0usize;
        for _ in 0..8 {
            payload_len = (payload_len << 8) | (buf[pos] as usize);
            pos += 1;
        }
    }
    
    let mask_key_pos = pos;
    if masked {
        if buf.len() < pos + 4 {
            return None;
        }
        pos += 4;
    }
    
    let frame_total = pos + payload_len;
    if buf.len() < frame_total {
        return None;
    }
    
    let mut payload = Vec::new();
    if payload_len > 0 {
        payload.extend_from_slice(&buf[pos..pos + payload_len]);
    }
    if masked {
        let key = &buf[mask_key_pos..mask_key_pos + 4];
        for i in 0..payload_len {
            payload[i] ^= key[i & 3];
        }
    }
    
    Some((frame_total, fin, opcode, payload))
}

/// Build WebSocket frame (unmasked, server->client)
pub fn build_websocket_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x80 | opcode);
    
    let l = payload.len();
    if l < 126 {
        out.push(l as u8);
    } else if l < 65536 {
        out.push(126);
        out.push(((l >> 8) & 0xff) as u8);
        out.push((l & 0xff) as u8);
    } else {
        out.push(127);
        for i in (0..8).rev() {
            out.push(((l >> (i * 8)) & 0xff) as u8);
        }
    }
    out.extend_from_slice(payload);
    out
}
