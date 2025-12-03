# Async NoStd Project - AI Agent Instructions

## Project Overview
**Workspace-based** freestanding Rust binary (`#![no_std]`, `#![no_main]`) with async/await via bare-metal Linux syscalls on x86_64. **Edition 2024** with workspace crate architecture (~2350 LOC, 32KB stripped binary). Production-ready HTTP + WebSocket server with multi-threaded async runtime.

## Architecture - Workspace Structure

```
Cargo.toml                       → Workspace root + binary target
src/main.rs (138 lines)          → Demo app: socket setup + acceptor thread spawning
crates/
  ├─ syscall/ (399 lines)        → Generic syscall wrappers (syscall1-6 pattern)
  ├─ runtime/ (712 lines)        → Entry point, allocator, scheduler, IO registry, acceptor wrapper
  │   ├─ lib.rs                  → Main module + acceptor thread helpers
  │   ├─ allocator.rs            → 16MB bump allocator
  │   ├─ scheduler.rs            → Lock-free Treiber stack task scheduler
  │   └─ io_registry.rs          → FD waker registration + eventfd signaling
  ├─ executor/ (76 lines)        → Worker thread pool coordinator
  ├─ net/ (185 lines)            → Network futures (Accept/Connect/Send/Recv)
  ├─ http/ (85 lines)            → HTTP request parser + response builder
  ├─ websocket/ (397 lines)      → WS handshake (SHA1+base64) + frame echo server
  └─ pty/ (13 lines)             → PTY stub (not implemented)
```

**Dependency Graph**: `syscall` → `runtime` → `executor`/`net` → `main`

**Critical Rule**: Main binary has NO unsafe code. Subcrates encapsulate all unsafe operations with safe public APIs.

### Crate Responsibilities

**async-syscall** (`crates/syscall/lib.rs`)
- Generic syscall helpers: `syscall1()` through `syscall6()` with inline asm
- Safe wrappers: `write()`, `exit()`, `mmap()`, `socket()`, `bind()`, `listen()`, etc.
- `spawn_thread()`: Clone-based with CLONE_VM|FS|FILES|SIGHAND|THREAD|SETTLS (real threads + TLS)
- Byte-order helpers: `htons()`, `ntohs()` for network conversions
- No dependencies, pure syscall layer

**async-runtime** (`crates/runtime/lib.rs` + submodules)
- `_start()`: Naked entry point with 16-byte stack alignment, calls `main_trampoline()`
- **Acceptor thread architecture**: `spawn_acceptor_thread(sfd, callback)` - spawns dedicated blocking thread that accepts connections and invokes callback per fd (eliminates accept/ppoll races)
- `AcceptorThreadArg`: C-compatible struct for passing sfd + callback to acceptor
- `acceptor_thread_wrapper()`: C-compatible entry point for clone syscall
- `run_acceptor_loop()`: Pure Rust blocking accept loop (sets socket blocking, calls accept4, sets accepted fd non-blocking, invokes callback)
- Bump allocator (`allocator.rs`): 16MB mmap-backed heap, CAS-based concurrent allocation, no deallocation
- Task scheduler (`scheduler.rs`): Lock-free Treiber stack for ready tasks, handle-based waker system, 1024 task slots with generation counters
- IO registry (`io_registry.rs`): `spin::Mutex<Vec<IoEntry>>` for fd → waker mappings, `ppoll()`-based readiness, eventfd signaling
- Utilities: `read_ptr_array()`, `parse_cstring_usize()`, `parse_cstring_ip()` for argv parsing
- **Depends on**: `async-syscall`, `spin`

**async-executor** (`crates/executor/lib.rs`)
- `Executor::enqueue_task()`: Registers task via `runtime::register_task()`
- `Executor::start_workers()`: Spawns N worker threads (64KB stack each) then becomes a worker itself
- `worker_loop()`: Polls scheduled tasks via `take_scheduled_task()` + `poll_task_safe()`, calls `ppoll_and_schedule()` when idle
- `TASKS_REMAINING`: Global atomic counter for completion tracking
- **Depends on**: `async-syscall`, `async-runtime`

**async-net** (`crates/net/lib.rs`)
- Futures: `AcceptFuture`, `ConnectFuture`, `RecvFuture`, `SendFuture`
- Polls syscalls, returns `Poll::Pending` on EAGAIN (-11), registers fd waker via `runtime::register_fd_waker()`
- **Note**: `AcceptFuture` exists but production uses acceptor thread instead to avoid races

**async-http** (`crates/http/lib.rs`)
- `handle_http_connection(fd)`: Async handler for HTTP/WebSocket routing
- Routes: `GET /` → serves embedded `html/index.html`, `GET /term` or `/ws` → WebSocket upgrade
- `http_response_headers()`: Builds HTTP response with status, content-type, content-length
- **Depends on**: `async-syscall`, `async-runtime`, `async-net`, `async-websocket`

**async-websocket** (`crates/websocket/lib.rs`)
- `accept_and_run(fd, request)`: WebSocket handshake + echo server
- Handshake: SHA-1 hash + base64 encoding of Sec-WebSocket-Key + GUID
- **Critical**: Temporarily sets socket blocking for handshake send (eliminates browser CONNECTING state bug), restores non-blocking for frame loop
- Frame parsing: Handles fragmentation, masking, ping/pong (0x9/0xA), close (0x8), text/binary (0x1/0x2)
- Sends frames as binary opcode (0x2) since client uses ArrayBuffer
- **Depends on**: `async-syscall`, `async-runtime`, `async-net`
## Build System

### Workspace Configuration
- **Workspace**: 7 member crates under `crates/`, main binary at root
- **Target**: `x86_64-unknown-none` (standard tier 2, NO custom JSON needed)
- **Build-std**: `.cargo/config.toml` auto-enables for all builds
- **Rustflags**: `-C relocation-model=static -C code-model=large` (in config.toml)
- **Binary**: 32KB stripped, statically linked ELF, NOT PIE

### Build & Run
```bash
# Build (nightly required for build-std)
cargo +nightly build --release

# Run server (accepts: worker_count, listen_ip, listen_port)
./target/x86_64-unknown-none/release/async-nostd 8              # 8 workers
./target/x86_64-unknown-none/release/async-nostd 4 127.0.0.1 8080  # custom port
./target/x86_64-unknown-none/release/async-nostd 0              # single-threaded mode

# Test suite (requires websocket-client: pip install websocket-client)
python3 test_all.py        # Runs all tests (single + multi-threaded)
python3 test_all.py single # Single-threaded tests only
python3 test_all.py multi  # Multi-threaded tests only
python3 test_all.py ws     # WebSocket tests only
```

### Profile Settings
```toml
[profile.release]
opt-level = "z"      # Size optimization
lto = true           # Link-time optimization
codegen-units = 1    # Single codegen for better optimization
strip = "symbols"    # Strip symbols for smaller binary
## Hard Rules for AI Agents

### Unsafe Code Placement
1. **Main binary (`src/main.rs`)**: ZERO unsafe code allowed
2. **Subcrates**: Unsafe only where necessary, always expose safe public APIs
3. All inline `asm!` blocks go in `crates/syscall/lib.rs` (private `syscallN` helpers)
4. Waker/task polling unsafe code in `crates/runtime/lib.rs` (`poll_task()`, `create_waker_for_handle()`)
5. When adding syscalls: Use generic `syscall1-6` helpers, expose safe typed wrapper

### Modularization Pattern (Acceptor Thread Example)
The acceptor thread demonstrates proper code organization:
- **Rust logic**: `run_acceptor_loop(sfd, callback)` - pure Rust, easy to test
- **C wrapper**: `acceptor_thread_wrapper(arg: *mut u8)` - minimal unsafe bridging code for clone syscall
- **Public API**: `spawn_acceptor_thread(sfd, callback)` - safe interface that hides implementation
- **Main usage**: Single line: `async_runtime::spawn_acceptor_thread(sfd, handle_accepted_connection)`
This pattern eliminated 158 lines of duplicated code from main.rs (53% reduction)
### Common Patterns

**Adding a new syscall:**
```rust
// In crates/syscall/lib.rs
pub fn getpid() -> i32 {
    let ret = unsafe { syscall1(39, 0) };  // __NR_getpid = 39
    if ret < 0 { -1 } else { ret as i32 }
}
```

**Using helper functions to reduce duplication:**
```rust
// Extract repeated setup into helpers (see src/main.rs)
fn create_listening_socket(ip: u32, port: usize) -> Result<i32, ()> {
    let sfd = sys::socket(AF_INET, SOCK_STREAM, 0);
    // ... bind, listen, error handling ...
    Ok(sfd)
}

fn print_socket_info(sfd: i32) {
    // ... getsockname, format output ...
}

// Main becomes concise
let sfd = create_listening_socket(listen_ip, listen_port)?;
print_socket_info(sfd);
async_runtime::spawn_acceptor_thread(sfd, handle_accepted_connection);
```

**Spawning async tasks:**
```rust
let executor = Executor::new();
let task = async move {
    sys::write(1, b"Hello\n");
};
executor.enqueue_task(Box::new(task))?;
executor.start_workers(4)?;
executor.wait_all();
``` crate::syscall::write;
write(b"Hello\n");  // Safe wrapper over syscall 1 (write)
## Critical Constraints

### Async Model
- **Poll::Pending IS supported**: Futures can return Pending and will be re-polled
- Waker system: Task handles stored in lock-free schedule queue, woken via `runtime::wake_handle()`
- IO readiness: `ppoll()` monitors registered fds, wakes tasks on POLLIN/POLLOUT events
- Futures in `async-net` crate use this pattern (see `AcceptFuture::poll()`)

### Thread Model
- `spawn_thread()` uses `clone` with flags: CLONE_VM|FS|FILES|SIGHAND|THREAD|SETTLS
- **Real threads with TLS**: CLONE_THREAD for shared PID, CLONE_SETTLS for thread-local storage
- Each worker: 64KB mmap stack + 4KB TLS block with self-pointer (x86_64 TLS ABI)
- Workers run forever in `worker_loop()`, polling scheduled tasks → `ppoll_and_schedule()` when idle
- **Acceptor thread**: Separate blocking thread that calls `accept4()` in loop, schedules per-connection tasks directly via `register_task()` + `wake_handle()` (avoids accept/ppoll races)

### Memory Model  
- Bump allocator: 16MB mmap heap, CAS-based concurrent allocation
- No deallocation: All allocations permanent until process exit
- Lock-free node freelist (256 cap) for task scheduler nodes
```
## Debugging Tips

**Segfault causes:**
- Stack misalignment in `_start()` (must be 16-byte aligned before call)
- Wrong `clone` stack pointer (must point to TOP of mmap'd stack)
- Missing `unsafe { }` wrapper around `asm!` in Edition 2024 (mandatory)

**Build issues:**
- "extern blocks must be unsafe" → wrap `extern "C"` with `unsafe extern "C"`
- `#[no_mangle]` error → use `#[unsafe(no_mangle)]` in Edition 2024
- Missing nightly → all builds require `cargo +nightly` for build-std

**Runtime hangs:**
- Task waiting forever → check if fd registered with `runtime::register_fd_waker()`
- Recv/Send stuck → ensure socket set non-blocking via `fcntl(fd, F_SETFL, O_NONBLOCK)`
## Adding Features

**New subcrate:**
```bash
mkdir -p crates/newcrate
cargo init --lib crates/newcrate --name async-newcrate --edition 2024
# Add to workspace members in root Cargo.toml
# Add dependencies to crates/newcrate/Cargo.toml
```

**New async future:**
```rust
// In crates/net/lib.rs (or new crate)
pub struct MyFuture { fd: i32, registered: bool }

impl core::future::Future for MyFuture {
    type Output = isize;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let r = async_syscall::my_syscall(self.fd);
        if r >= 0 { return Poll::Ready(r); }
        if r == -11 {  // EAGAIN
            if !self.registered {
                async_runtime::register_fd_waker(self.fd, 0x0001, cx.waker().clone());
                self.registered = true;
            }
            return Poll::Pending;
        }
        Poll::Ready(r)
    }
}
```

**Optimization tips:**
- Profile with `opt-level = "z"` for size, `"3"` for speed
- Check binary size: `ls -lh target/x86_64-unknown-none/release/async-nostd`
- Use `cargo bloat --release` to find large code sections (requires `cargo-bloat` install)
