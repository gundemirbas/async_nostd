//! WebSocket server implementation

#![no_std]

extern crate alloc;
use alloc::vec::Vec;
use async_net::{RecvFuture, SendFuture};
use async_syscall as sys;
use async_utils::{crypto, parsing};

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Public API: Accept WebSocket connection
pub async fn accept_connection(fd: i32, request: &[u8]) {
    accept_and_run(fd, request).await
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
    async_runtime::log_write(b"[WS] fd=");
    sys::write_usize(
        async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed),
        fd as usize,
    );
    async_runtime::log_write(b" handshake start\n");

    // find Sec-WebSocket-Key header
    if let Some(key_bytes) = parsing::find_header_value(request, "Sec-WebSocket-Key") {
        // trim whitespace
        let mut key_trim = key_bytes;
        while !key_trim.is_empty() && key_trim[0] == b' ' {
            key_trim = &key_trim[1..];
        }
        while !key_trim.is_empty() && (key_trim[key_trim.len() - 1] == b' ') {
            key_trim = &key_trim[..key_trim.len() - 1];
        }
        // compute accept using utils
        let mut combined = Vec::from(key_trim);
        combined.extend_from_slice(WS_GUID.as_bytes());
        let digest = crypto::sha1(&combined);
        let accept = crypto::base64_encode(&digest);

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
        let mut iteration = 0u32;
        while off_sync < resp.len() {
            iteration += 1;
            if iteration > 1000 {
                // Safety valve: avoid infinite loop, switch to async
                let _ = sys::fcntl(fd, sys::F_SETFL, sys::O_NONBLOCK);
                let _ = SendFuture::new(fd, &resp[off_sync..]).await;
                break;
            }
            let ptr = unsafe { resp.as_ptr().add(off_sync) };
            let rem = resp.len() - off_sync;
            let r = sys::sendto(fd, ptr, rem, 0, core::ptr::null(), 0);
            if r == -11 {
                // EAGAIN in blocking mode shouldn't happen
                continue;
            }
            if r < 0 {
                // Error: fall back to async send
                let _ = sys::fcntl(fd, sys::F_SETFL, sys::O_NONBLOCK);
                let _ = SendFuture::new(fd, &resp[off_sync..]).await;
                break;
            }
            let wrote = r as usize;
            if wrote == 0 {
                // Unexpected zero: switch to async
                let _ = sys::fcntl(fd, sys::F_SETFL, sys::O_NONBLOCK);
                let _ = SendFuture::new(fd, &resp[off_sync..]).await;
                break;
            }
            off_sync += wrote;
        }
        async_runtime::log_write(b"[WS] fd=");
        sys::write_usize(
            async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed),
            fd as usize,
        );
        async_runtime::log_write(b" handshake sent\n");

        // Restore non-blocking mode for normal async operation
        let _ = sys::fcntl(fd, sys::F_SETFL, sys::O_NONBLOCK);

        async_runtime::log_write(b"[WS] fd=");
        sys::write_usize(
            async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed),
            fd as usize,
        );
        async_runtime::log_write(b" handshake complete\n");

        // Send welcome message with ANSI colors
        let welcome = b"\r\n\x1b[1;32m=== Async NoStd Terminal ===\x1b[0m\r\n\r\n\
\x1b[1;36mWelcome to the async no_std WebSocket server!\x1b[0m\r\n\r\n\
Features:\r\n\
  \x1b[32m*\x1b[0m Lock-free task scheduler\r\n\
  \x1b[32m*\x1b[0m Multi-threaded async runtime\r\n\
  \x1b[32m*\x1b[0m WebSocket echo server\r\n\
  \x1b[32m*\x1b[0m 32KB binary size\r\n\r\n\
Type anything to see it echoed back!\r\n\r\n";
        send_ws_payload(fd, welcome).await;

        // enter buffered frame loop: accumulate recv bytes and parse frames incrementally.
        let mut buf_acc: Vec<u8> = Vec::new();
        // fragmentation state
        let mut frag_opcode: Option<u8> = None;
        let mut frag_payload: Vec<u8> = Vec::new();

        loop {
            let chunk = RecvFuture::new(fd, 4096).await;
            if chunk.is_empty() {
                async_runtime::log_write(b"[WS] fd=");
                sys::write_usize(
                    async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed),
                    fd as usize,
                );
                async_runtime::log_write(b" client disconnected\n");
                async_runtime::unregister_fd(fd);
                let _ = sys::close(fd);
                return;
            }
            async_runtime::log_write(b"[WS] fd=");
            sys::write_usize(
                async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed),
                fd as usize,
            );
            async_runtime::log_write(b" recv ");
            sys::write_usize(
                async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed),
                chunk.len(),
            );
            async_runtime::log_write(b" bytes\n");
            buf_acc.extend_from_slice(&chunk);

            // parse as many frames as available
            let mut parsed_any = false;
            while let Some((consumed, fin, opcode, payload)) =
                parsing::parse_websocket_frame(&buf_acc)
            {
                // remove consumed bytes
                let _ = buf_acc.drain(0..consumed);
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
                                    // Send ping to keep connection alive
                                    let ping = [0x80 | 0x9, 0x00]; // FIN + ping opcode, 0 length
                                    let _ = SendFuture::new(fd, &ping).await;
                                }
                            }
                            continue;
                        }

                        if opcode == 0x1 || opcode == 0x2 {
                            if fin {
                                // single-frame message — echo
                                send_ws_payload(fd, &payload).await;
                                // Send ping to keep connection alive
                                let ping = [0x80 | 0x9, 0x00]; // FIN + ping opcode, 0 length
                                let _ = SendFuture::new(fd, &ping).await;
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
                                async_runtime::unregister_fd(fd);
                                let _ = sys::close(fd);
                                return;
                            }
                            0x9 => {
                                // ping -> pong (opcode 0xA)
                                let pong = parsing::build_websocket_frame(0xA, &payload);
                                let _ = SendFuture::new(fd, &pong).await;
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
    } else {
        // Sec-WebSocket-Key not found - invalid WebSocket handshake
        async_runtime::log_write(b"[WS] fd=");
        sys::write_usize(
            async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed),
            fd as usize,
        );
        async_runtime::log_write(b" ERROR: No Sec-WebSocket-Key, closing\n");
        async_runtime::unregister_fd(fd);
        let _ = sys::close(fd);
    }
}
