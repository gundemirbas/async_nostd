//! Minimal WebSocket implementation: SHA1 + base64, handshake and echo framing.

#![no_std]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use async_net::{RecvFuture, SendFuture};
use async_syscall as sys;

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

fn rol(v: u32, s: u32) -> u32 {
    v.rotate_left(s)
}

fn sha1(input: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (input.len() as u64) * 8;

    // build padded message
    let mut msg = Vec::from(input);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    // append 64-bit big-endian length
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

fn base64_encode(src: &[u8]) -> String {
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

fn find_header_val<'a>(req: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let needle = name.as_bytes();
    let mut i = 0;
    while i + needle.len() < req.len() {
        if req[i..i + needle.len()].eq(needle) {
            // skip name and possible colon/space
            let mut j = i + needle.len();
            while j < req.len() && (req[j] == b':' || req[j] == b' ') {
                j += 1;
            }
            // read until CRLF
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

async fn send_ws_payload(fd: i32, payload: &[u8]) {
    // Build a single unmasked text frame (server -> client)
    let mut out = Vec::new();
    out.push(0x80 | 0x2); // FIN + binary opcode (client expects ArrayBuffer)
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

    // Ensure we send the entire frame, handling partial writes
    let mut off = 0usize;
    while off < out.len() {
        let slice = &out[off..];
        let r = SendFuture::new(fd, slice).await;
        if r < 0 {
            break;
        }
        let wrote = r as usize;
        off += wrote;
        if wrote == 0 {
            // avoid spin in case of unexpected 0
            break;
        }
    }
}

pub async fn accept_and_run(fd: i32, request: &[u8]) {
    // find Sec-WebSocket-Key header
    if let Some(key_bytes) = find_header_val(request, "Sec-WebSocket-Key") {
        // trim whitespace
        let mut key_trim = key_bytes;
        while !key_trim.is_empty() && key_trim[0] == b' ' {
            key_trim = &key_trim[1..];
        }
        while !key_trim.is_empty() && (key_trim[key_trim.len() - 1] == b' ') {
            key_trim = &key_trim[..key_trim.len() - 1];
        }
        // compute accept
        let mut combined = Vec::from(key_trim);
        combined.extend_from_slice(WS_GUID.as_bytes());
        let digest = sha1(&combined);
        let accept = base64_encode(&digest);

        let mut resp = Vec::new();
        resp.extend_from_slice(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ");
        resp.extend_from_slice(accept.as_bytes());
        resp.extend_from_slice(b"\r\n\r\n");
        // Log the request line (first CRLF) to aid debugging
        // (debug logging removed)

        // To ensure the browser receives the HTTP 101 immediately and
        // doesn't stay in CONNECTING, temporarily clear O_NONBLOCK on the
        // accepted socket and perform a blocking write loop for the
        // handshake response. After the handshake we restore
        // non-blocking mode and continue with async I/O for frames.
        let _ = sys::fcntl(fd, sys::F_SETFL, 0); // clear O_NONBLOCK
        let mut off_sync = 0usize;
        while off_sync < resp.len() {
            let ptr = unsafe { resp.as_ptr().add(off_sync) };
            let rem = resp.len() - off_sync;
            let r = sys::sendto(fd, ptr, rem, 0, core::ptr::null(), 0);
            if r < 0 {
                // Something went wrong with the blocking send; fall back
                // to async send for the remainder.
                let _ = sys::fcntl(fd, sys::F_SETFL, sys::O_NONBLOCK);
                let _ = SendFuture::new(fd, &resp[off_sync..]).await;
                break;
            }
            let wrote = r as usize;
            if wrote == 0 {
                // Unexpected zero — break to avoid spinning and use async
                let _ = sys::fcntl(fd, sys::F_SETFL, sys::O_NONBLOCK);
                let _ = SendFuture::new(fd, &resp[off_sync..]).await;
                break;
            }
            off_sync += wrote;
        }
        // Restore non-blocking mode for normal async operation
        let _ = sys::fcntl(fd, sys::F_SETFL, sys::O_NONBLOCK);

        // Send welcome message after successful handshake synchronously
        let welcome = b"\r\n\x1b[1;32m=== Async NoStd Terminal ===\x1b[0m\r\n\r\n\x1b[1;36mWelcome to the no_std async runtime!\x1b[0m\r\n\r\nFeatures:\r\n  \x1b[33m*\x1b[0m Lock-free task scheduler (Treiber stack)\r\n  \x1b[33m*\x1b[0m Multi-threaded workers with TLS\r\n  \x1b[33m*\x1b[0m ppoll-based async I/O\r\n  \x1b[33m*\x1b[0m 31KB binary (stripped)\r\n\r\n\x1b[90mType anything and it will be echoed back...\x1b[0m\r\n\r\n$ ";
        let _ = send_ws_payload(fd, welcome).await;

        // Notify local console that a websocket connection has been established.
        // Prefer a detailed line with remote IP:port so tests and logs can
        // attribute the connection. Fall back to a simple marker if
        // peer lookup fails.

            // (no connection stdout logging in production)

        // enter buffered frame loop: accumulate recv bytes and parse frames incrementally.
        let mut buf_acc: Vec<u8> = Vec::new();
        // fragmentation state
        let mut frag_opcode: Option<u8> = None;
        let mut frag_payload: Vec<u8> = Vec::new();

        loop {
            let chunk = RecvFuture::new(fd, 4096).await;
            if chunk.is_empty() {
                let _ = sys::close(fd);
                return;
            }
            buf_acc.extend_from_slice(&chunk);

            // parse as many frames as available
            let mut parsed_any = false;
            loop {
                if buf_acc.len() < 2 {
                    break;
                }
                let b1 = buf_acc[0];
                let b2 = buf_acc[1];
                let fin = (b1 & 0x80) != 0;
                let opcode = b1 & 0x0f;
                let masked = (b2 & 0x80) != 0;
                let mut payload_len = (b2 & 0x7f) as usize;
                let mut pos = 2usize;
                if payload_len == 126 {
                    if buf_acc.len() < pos + 2 {
                        break;
                    }
                    payload_len = ((buf_acc[pos] as usize) << 8) | (buf_acc[pos + 1] as usize);
                    pos += 2;
                } else if payload_len == 127 {
                    if buf_acc.len() < pos + 8 {
                        break;
                    }
                    payload_len = 0usize;
                    for _ in 0..8 {
                        payload_len = (payload_len << 8) | (buf_acc[pos] as usize);
                        pos += 1;
                    }
                }
                let mask_key_pos = pos;
                if masked {
                    if buf_acc.len() < pos + 4 {
                        break;
                    }
                    pos += 4;
                }
                let frame_total = pos + payload_len;
                if buf_acc.len() < frame_total {
                    break;
                }

                // extract payload
                let mut payload = Vec::new();
                if payload_len > 0 {
                    payload.extend_from_slice(&buf_acc[pos..pos + payload_len]);
                }
                if masked {
                    let key = &buf_acc[mask_key_pos..mask_key_pos + 4];
                    for i in 0..payload_len {
                        payload[i] ^= key[i & 3];
                    }
                }

                // remove consumed bytes
                let _ = buf_acc.drain(0..frame_total);
                parsed_any = true;

                // handle fragmentation
                if opcode == 0x0 {
                    // continuation
                    if frag_opcode.is_none() {
                        // unexpected continuation, ignore
                        continue;
                    }
                    frag_payload.extend_from_slice(&payload);
                    if fin {
                        // finalize
                        let op = frag_opcode.take().unwrap();
                        let full = core::mem::take(&mut frag_payload);
                        // echo as text/binary based on op
                        if op == 0x1 || op == 0x2 {
                            // echo back
                            send_ws_payload(fd, &full).await;
                        }
                    }
                    continue;
                }

                if opcode == 0x1 || opcode == 0x2 {
                    if fin {
                        // single-frame message — echo
                        send_ws_payload(fd, &payload).await;
                    } else {
                        // start fragmentation
                        frag_opcode = Some(opcode);
                        frag_payload.clear();
                        frag_payload.extend_from_slice(&payload);
                    }
                    continue;
                }

                match opcode {
                    0x8 => {
                        // close
                        let _ = sys::close(fd);
                        return;
                    }
                    0x9 => {
                        // ping -> pong
                        let mut out = Vec::new();
                        out.push(0x80 | 0xA);
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
                        out.extend_from_slice(&payload);
                        let _ = SendFuture::new(fd, &out).await;
                    }
                    _ => {
                        // ignore other opcodes
                    }
                }
            }

            if !parsed_any {
                // need more data; continue recv
            }
        }
    }
}
