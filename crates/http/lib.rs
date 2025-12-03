//! Minimal HTTP server/client using the runtime and net crates

#![no_std]

extern crate alloc;
use alloc::vec::Vec;
use async_net::RecvFuture;
use async_net::SendFuture;
use async_syscall as sys;

// Embed the HTML into the binary so HTTP handler can serve it directly.
static INDEX_HTML: &[u8] = include_bytes!("../../html/index.html");

pub fn http_response_headers(status: &str, content_type: &str, content_len: usize) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"HTTP/1.1 ");
    v.extend_from_slice(status.as_bytes());
    v.extend_from_slice(b"\r\nContent-Type: ");
    v.extend_from_slice(content_type.as_bytes());
    v.extend_from_slice(b"\r\nContent-Length: ");
    // simple number formatting
    let mut num = content_len;
    let mut buf = [0u8; 20];
    let mut i = 0;
    if num == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        while num > 0 {
            buf[i] = b'0' + (num % 10) as u8;
            num /= 10;
            i += 1;
        }
        // reverse
        let mut j = 0;
        while j < i / 2 {
            buf.swap(j, i - 1 - j);
            j += 1;
        }
    }
    v.extend_from_slice(&buf[..i]);
    v.extend_from_slice(b"\r\n\r\n");
    v
}

pub async fn handle_http_connection(fd: i32) {
    async_runtime::log_write(b"[HTTP] fd=");
    sys::write_usize(async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed), fd as usize);
    async_runtime::log_write(b" recv request\n");
    
    // read request - may need multiple reads for WebSocket handshake
    let mut buf = RecvFuture::new(fd, 2048).await;
    
    if buf.is_empty() {
        async_runtime::log_write(b"[HTTP] fd=");
        sys::write_usize(async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed), fd as usize);
        async_runtime::log_write(b" empty recv, closing\n");
        let _ = sys::close(fd);
        return;
    }
    
    // Check if we have complete HTTP request (ends with \r\n\r\n)
    let has_complete_headers = buf.windows(4).any(|w| w == b"\r\n\r\n");
    if !has_complete_headers && buf.len() == 2048 {
        // Try reading more
        let extra = RecvFuture::new(fd, 1024).await;
        if !extra.is_empty() {
            buf.extend_from_slice(&extra);
        }
    }
    // parse very simply
    if buf.starts_with(b"GET / ") || buf.starts_with(b"GET /HTTP") || buf.starts_with(b"GET / HTTP")
    {
        async_runtime::log_write(b"[HTTP] fd=");
        sys::write_usize(async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed), fd as usize);
        async_runtime::log_write(b" route=/ (index)\n");
        
        let mut resp =
            http_response_headers("200 OK", "text/html; charset=utf-8", INDEX_HTML.len());
        resp.extend_from_slice(INDEX_HTML);
        let _ = SendFuture::new(fd, &resp).await;
    } else if buf.starts_with(b"GET /term ")
        || buf.starts_with(b"GET /term")
        || buf.starts_with(b"GET /ws ")
        || buf.starts_with(b"GET /ws")
    {
        async_runtime::log_write(b"[HTTP] fd=");
        sys::write_usize(async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed), fd as usize);
        async_runtime::log_write(b" route=/ws (websocket upgrade)\n");
        
        // WebSocket endpoint for terminal
        // NOTE: WebSocket handler manages fd lifetime, doesn't return until connection closes
        async_websocket::accept_and_run(fd, &buf).await;
        return;
    } else {
        async_runtime::log_write(b"[HTTP] fd=");
        sys::write_usize(async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed), fd as usize);
        async_runtime::log_write(b" route=unknown (404)\n");
        
        let body = b"Not Found\n";
        let mut resp = http_response_headers("404 Not Found", "text/plain", body.len());
        resp.extend_from_slice(body);
        let _ = SendFuture::new(fd, &resp).await;
    }
    async_runtime::log_write(b"[HTTP] fd=");
    sys::write_usize(async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed), fd as usize);
    async_runtime::log_write(b" done, closing\n");
    let _ = sys::close(fd);
}
