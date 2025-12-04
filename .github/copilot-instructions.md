# Async NoStd - AI Agent Instructions

## Project Overview
Production-ready `#![no_std]` async runtime (2,203 LOC, 36KB binary). Bare-metal Linux syscalls on x86_64 with lock-free scheduler, multi-threaded executor, HTTP/WebSocket server. Edition 2024 workspace architecture with utils crate for code reuse.

## Critical Architecture Decisions

### 1. Config Centralization (`crates/runtime/config.rs`)
**All constants live here**: `MAX_TASK_SLOTS` (1024), `HEAP_SIZE` (16MB), `WORKER_STACK_SIZE` (64KB), `LISTEN_BACKLOG` (128), `LOG_FILE_PATH`, etc. Import from runtime, not hardcode values.

### 2. Acceptor Thread Pattern (Race-Free Accept)
```rust
// Main spawns dedicated blocking thread for accept()
async_runtime::spawn_acceptor_thread(sfd, handle_accepted_connection);

// Acceptor loop (runtime/lib.rs):
loop {
    let cfd = accept4(sfd, ...);  // Blocks
    fcntl(cfd, F_SETFL, O_NONBLOCK);  // Make async
    callback(cfd);  // Spawn task
}
```
**Why**: Eliminates accept/ppoll race. Acceptor never calls ppoll, workers never call accept.

### 3. Logging System
- **Server**: Opens `/tmp/async-nostd.log` with `O_TRUNC` (truncated on startup)
- **Format**: `[ACCEPT] fd=5`, `[HTTP] fd=5 route=/`, `[WS] fd=7 handshake complete`, `[ppoll] monitoring 3 fds`
- **Usage**: `async_runtime::log_write(b"[TAG] message\n");` then `sys::write_usize(LOG_FD.load(Ordering::Relaxed), value)`
- **Critical**: LOG_FD is AtomicI32, load with Relaxed ordering

### 4. Closed FD Cleanup (ppoll_and_schedule)
```rust
let is_closed = (pf.revents & 0x38) != 0;  // POLLERR|POLLHUP|POLLNVAL
if is_closed {
    log_write(b"[ppoll] removing closed fd=");
    reg.swap_remove(i);  // Prevents CPU spike
}
```
**Bug fixed**: Without this, closed FDs cause ppoll busy loop (200% CPU).

### 5. WebSocket Handshake Blocking Fix
```rust
// Temporarily block for handshake send (websocket/lib.rs)
fcntl(fd, F_SETFL, 0);  // Clear O_NONBLOCK
// ... blocking sendto loop ...
fcntl(fd, F_SETFL, O_NONBLOCK);  // Restore async
```
**Why**: Browser stays in CONNECTING state without full handshake delivery.

## Workspace Structure

```
src/main.rs (134 lines)          → open log + create socket + spawn acceptor
crates/
  syscall/ (426 lines)           → syscall1-6 + spawn_thread (CLONE_THREAD|SETTLS)
  runtime/ (376 lines)           → config.rs + allocator + scheduler + io_registry
  executor/ (83 lines)           → worker_loop: poll tasks → ppoll → repeat
  net/ (185 lines)               → RecvFuture/SendFuture (EAGAIN → register waker)
  http/ (111 lines)              → route: / → index, /ws → websocket upgrade
  websocket/ (252 lines)         → handshake + frame echo (uses utils)
  utils/ (237 lines)             → crypto.rs (SHA1/Base64) + parsing.rs (HTTP/WS frames)
```

**Note**: `async-pty` crate exists but unused. **Test**: `test.py` (28 tests with filtering).

## Build & Test

```bash
# Build (nightly required for build-std)
cargo +nightly build --release

# Run (args: workers, ip, port)
./target/x86_64-unknown-none/release/async-nostd 8 127.0.0.1 8080

# Test with filters
python3 test.py              # All 28 tests
python3 test.py browser      # Browser simulation (7 tests)
python3 test.py ws           # WebSocket only (9 tests)
python3 test.py http         # HTTP only (12 tests)
```

**Logs**: Server `/tmp/async-nostd.log`, Test `/tmp/test_all_output.log`

## Hard Rules

### Unsafe Code
1. **Main binary**: ZERO unsafe allowed
2. **syscall crate**: All `asm!` blocks (private syscall1-6)
3. **runtime crate**: Waker creation, task polling
4. Always expose safe public API

### Adding Syscalls
```rust
// crates/syscall/lib.rs
pub fn getpid() -> i32 {
    let ret = unsafe { syscall1(39, 0) };  // __NR_getpid
    if ret < 0 { -1 } else { ret as i32 }
}
```

### Adding Constants
```rust
// crates/runtime/config.rs (NOT inline)
pub const NEW_LIMIT: usize = 512;

// Usage elsewhere
use async_runtime::NEW_LIMIT;
```

### Spawning Tasks
```rust
// Ergonomic API: spawn_task (auto-boxing + wake)
let handle = async_runtime::spawn_task(my_async_function(arg));

// Low-level API: spawn (manual boxing, no auto-wake)
let handle = async_runtime::spawn(Box::new(my_future));
async_runtime::wake_handle(handle);
```

### Async Future Pattern
```rust
impl Future for MyFuture {
    type Output = Vec<u8>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let r = syscall::my_syscall(self.fd, ...);
        if r >= 0 { return Poll::Ready(...); }
        if r == -11 {  // EAGAIN
            if !self.registered {
                async_runtime::register_fd_waker(self.fd, POLLIN, cx.waker().clone());
                self.registered = true;
            }
            return Poll::Pending;
        }
        Poll::Ready(Err(...))
    }
}
```

## Common Issues

**Segfault**: Stack misalignment in `_start()` (16-byte required) or wrong clone stack pointer (must point to TOP).

**Hang**: FD not registered with `register_fd_waker()` or socket not set `O_NONBLOCK`.

**CPU Spike**: Closed FDs not removed from IO registry (check `ppoll_and_schedule` cleanup).

**Edition 2024**: Requires `unsafe { asm!(...) }`, `#[unsafe(no_mangle)]`, `unsafe extern "C"`.

## Key Files

- **Config**: `crates/runtime/config.rs` - All tunable constants
- **Entry**: `crates/runtime/lib.rs` - `_start()`, acceptor thread, ppoll, spawn APIs
- **Scheduler**: `crates/runtime/scheduler.rs` - Treiber stack, 1024 slots
- **Utils**: `crates/utils/` - Shared crypto (SHA1/Base64) and parsing (HTTP/WebSocket)
- **Main**: `src/main.rs` - Socket setup, log open, acceptor spawn
- **Test**: `test.py` - 28 tests with filtering (not `test_all.py`)

## Utils Crate Pattern

Shared utilities to eliminate code duplication:

```rust
// crates/utils/crypto.rs - Cryptographic functions
pub fn sha1(input: &[u8]) -> [u8; 20];
pub fn base64_encode(src: &[u8]) -> String;

// crates/utils/parsing.rs - Protocol parsing
pub fn find_header_value<'a>(req: &'a [u8], name: &str) -> Option<&'a [u8]>;
pub fn parse_websocket_frame(buf: &[u8]) -> Option<(usize, bool, u8, Vec<u8>)>;
pub fn build_websocket_frame(opcode: u8, payload: &[u8]) -> Vec<u8>;
```

**Usage**: `use async_utils::{crypto, parsing};` in websocket/http crates.

## Documentation

- **README.md**: Quick start, features, usage
- **PROJECT_STRUCTURE.md**: Deep dive, patterns, memory layout
- **CHANGELOG.md**: Version history (v0.2.0 = optimization release)

Performance: 36KB binary, 10K+ req/s, sub-ms latency, 28/28 tests passing.
