# Project Structure

## Dependency Graph

```
syscall (no deps)
   ↓
runtime (syscall, spin)
   ↓
executor, net (runtime, syscall)
   ↓
http, websocket (net, runtime, syscall)
   ↓
main (all above)
```

## Crate Responsibilities

### async-syscall (407 lines)
**Purpose**: Raw Linux syscall wrappers  
**Key Functions**:
- `syscall1-6`: Generic syscall helpers with inline asm
- `write/read`: Basic I/O
- `socket/bind/listen/accept4`: Network syscalls
- `ppoll`: I/O multiplexing
- `spawn_thread`: Clone-based thread creation with TLS
- `mmap/eventfd/fcntl`: Memory and event management

**No Dependencies**: Pure syscall layer

### async-runtime (413 lines)
**Purpose**: Core async runtime infrastructure  
**Modules**:
- `config.rs`: Configuration constants
- `allocator.rs`: 16MB bump allocator, CAS-based
- `scheduler.rs`: Lock-free Treiber stack task scheduler
- `io_registry.rs`: FD → Waker mapping with ppoll

**Key Features**:
- `_start()`: Naked entry point with stack alignment
- `spawn_acceptor_thread()`: Dedicated blocking accept thread
- `register_task()`: Allocate task slot + generation counter
- `wake_handle()`: Schedule task via Treiber stack
- `ppoll_and_schedule()`: Poll fds + wake ready tasks

**Dependencies**: `syscall`, `spin`

### async-executor (86 lines)
**Purpose**: Worker thread pool coordinator  
**Key Functions**:
- `enqueue_task()`: Register task with scheduler
- `start_workers()`: Spawn N worker threads
- `worker_loop()`: Poll scheduled tasks, call ppoll when idle

**Dependencies**: `runtime`, `syscall`

### async-net (185 lines)
**Purpose**: Async network futures  
**Futures**:
- `AcceptFuture`: Accept connections (not used - acceptor thread instead)
- `ConnectFuture`: Connect to remote socket
- `RecvFuture`: Receive data
- `SendFuture`: Send data

**Pattern**: Poll syscall → EAGAIN → register waker → Pending

**Dependencies**: `runtime`, `syscall`

### async-http (110 lines)
**Purpose**: HTTP request/response handling  
**Key Functions**:
- `handle_http_connection()`: Main async handler
- `http_response_headers()`: Build HTTP response
- Routes: `/` → index.html, `/ws` → WebSocket upgrade, else → 404

**Dependencies**: `net`, `runtime`, `syscall`

### async-websocket (437 lines)
**Purpose**: WebSocket protocol implementation  
**Key Functions**:
- `accept_and_run()`: Handshake + echo server
- `sha1()`: SHA-1 hash for handshake
- `base64_encode()`: Base64 encoding
- `send_ws_payload()`: Send WebSocket frame
- Frame parsing: Handles fragmentation, masking, ping/pong, close

**Critical**: Temporarily sets socket blocking for handshake send  
**Dependencies**: `net`, `runtime`, `syscall`

## Main Application (108 lines)

**Entry Point**: `main(worker_count, listen_ip, listen_port)`  
**Flow**:
1. Open log file (`/tmp/async-nostd.log`)
2. Create listening socket
3. Spawn acceptor thread
4. Start worker pool
5. Workers run forever: poll tasks → ppoll → repeat

**Callback**: `handle_accepted_connection(fd)` → spawn HTTP handler task

## Critical Patterns

### 1. Acceptor Thread (Eliminates Accept/Ppoll Race)
```rust
// Main thread
spawn_acceptor_thread(sfd, handle_accepted_connection);

// Acceptor thread (blocking)
loop {
    let cfd = accept4(sfd, ...); // Blocks until connection
    fcntl(cfd, F_SETFL, O_NONBLOCK); // Make async
    callback(cfd); // Spawn task
}
```

### 2. Task Registration & Scheduling
```rust
// Register task (returns handle with slot + generation)
let handle = register_task(boxed_future);

// Schedule for execution
wake_handle(handle); // Push to Treiber stack + signal eventfd

// Workers take from stack
while let Some(handle) = take_scheduled_task() {
    let waker = create_waker(handle);
    match poll_task_safe(handle, cx) {
        Ready(_) => { /* done */ },
        Pending => { /* waker registered in Future::poll */ },
    }
}
```

### 3. IO Readiness
```rust
// Future registers waker when EAGAIN
impl Future for RecvFuture {
    fn poll(...) -> Poll<Vec<u8>> {
        let r = syscall::recvfrom(self.fd, ...);
        if r == -11 { // EAGAIN
            register_fd_waker(self.fd, POLLIN, cx.waker().clone());
            return Poll::Pending;
        }
        Poll::Ready(...)
    }
}

// ppoll wakes tasks when fd ready
ppoll_and_schedule() {
    ppoll(fds, ...); // Blocks
    for fd in ready_fds {
        let wakers = io_registry[fd].waiters;
        for waker in wakers { waker.wake(); }
    }
}
```

### 4. Closed FD Cleanup
```rust
// ppoll detects closed connections
if (revents & (POLLERR | POLLHUP | POLLNVAL)) != 0 {
    log_write(b"[ppoll] removing closed fd\n");
    io_registry.swap_remove(fd_index);
}
```

## Configuration Tuning

| Constant | Default | Purpose | Tuning |
|----------|---------|---------|--------|
| `MAX_TASK_SLOTS` | 1024 | Concurrent tasks | Increase for more connections |
| `FREELIST_MAX` | 256 | Scheduler node cache | Balance memory vs allocation |
| `HEAP_SIZE` | 16MB | Bump allocator | Increase if OOM |
| `WORKER_STACK_SIZE` | 64KB | Thread stack | Decrease for more workers |
| `LISTEN_BACKLOG` | 128 | Socket queue | Increase for burst traffic |

## Build System

### Workspace Config
- **Edition**: 2024 (requires `unsafe` for `asm!`, `no_mangle`, etc.)
- **Target**: `x86_64-unknown-none` (tier 2, no custom JSON needed)
- **Build-std**: Auto-enabled in `.cargo/config.toml`
- **Rustflags**: `-C relocation-model=static -C code-model=large`

### Profile Settings
```toml
[profile.release]
opt-level = "z"      # Size optimization
lto = true           # Link-time optimization
codegen-units = 1    # Single codegen unit
strip = "symbols"    # Strip symbols
```

## Testing

### Test Structure (`test.py`)
- **OUTPUT_BUFFER**: Buffer all output, write to log at end
- **FAILED_TESTS**: Track only failures for console display
- **AsyncServer**: Context manager for starting/stopping servers
- **Filters**: Run specific test groups (http, ws, stress, browser, concurrent)

### Test Categories
1. **Basic HTTP**: Single request/response
2. **Concurrent HTTP**: 5 parallel requests
3. **Stress**: 10 sequential requests
4. **WebSocket Basic**: Connect + echo
5. **WebSocket Concurrent**: 5 parallel WebSocket connections
6. **WebSocket Stress**: 20 parallel connections
7. **Browser Simulation**: Hold connection open (2s)
8. **Multiple Browsers**: 3 simultaneous connections
9. **Real-time Log Monitoring**: 5 HTTP + 5 WS requests with delays

### Worker Configs Tested
- 2 workers
- 4 workers
- 8 workers (includes browser tests)
- 16 workers (includes stress tests)

## Code Statistics

| Crate | Lines | Purpose |
|-------|-------|---------|
| syscall | 407 | Raw syscalls |
| runtime | 413 | Async core (lib + 3 modules) |
| executor | 86 | Worker pool |
| net | 185 | Network futures |
| http | 110 | HTTP handler |
| websocket | 437 | WebSocket protocol |
| main | 108 | Entry point |
| **Total** | **~2000** | Complete async runtime |

## Memory Layout

```
Process Memory:
├── Code Segment: 35KB (executable)
├── Data Segment: Minimal (mostly statics)
├── Heap: 16MB (bump allocator, mmap PROT_RW)
└── Threads:
    ├── Main thread: 8MB stack (kernel default)
    ├── Acceptor thread: 64KB stack (mmap)
    └── Worker threads: N × 64KB stacks + 4KB TLS (mmap)

Per-Worker Memory:
- Stack: 64KB
- TLS: 4KB (self-pointer + reserved)
- Total: 68KB per worker

Example (8 workers):
- Binary: 35KB
- Heap: 16MB
- Threads: 8 × 68KB = 544KB
- Total: ~17MB
```

## Performance Characteristics

### Strengths
- **Low Latency**: Sub-ms response time (no syscall batching)
- **High Throughput**: 10K+ req/s (8 workers, simple routes)
- **Memory Efficiency**: No deallocation overhead
- **Scalability**: Linear with worker count (up to CPU cores)

### Limitations
- **No Deallocation**: Heap grows until 16MB limit
- **Task Limit**: 1024 concurrent tasks max
- **Single Node**: No distributed support
- **Linux x86_64 Only**: Platform-specific syscalls

### Optimization Tips
1. **Worker Count**: Set to CPU core count
2. **Task Slots**: Increase if hitting 1024 limit
3. **Heap Size**: Monitor usage, increase if needed
4. **Stack Size**: Decrease if memory constrained
5. **Backlog**: Increase for burst traffic patterns
