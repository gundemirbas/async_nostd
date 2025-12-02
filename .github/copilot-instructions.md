# Async Futures Project - Copilot Instructions

## Project Overview
Freestanding Rust binary (`#![no_std]`, `#![no_main]`) demonstrating async/await with bare-metal syscalls on x86_64 Linux. **Edition 2024** with strict unsafe isolation.

## Architecture (4 modules, ~450 LOC)

```
src/main.rs (91 lines)      → Application logic (no unsafe code)
src/executor.rs (88 lines)  → Task executor (no unsafe code) 
src/runtime.rs (134 lines)  → Program startup, allocator, waker (unsafe allowed)
src/syscall.rs (136 lines)  → Raw Linux syscalls (unsafe allowed)
```

**Critical Rule**: `main.rs` and `executor.rs` contain ZERO unsafe code. ALL unsafe operations must be in `runtime.rs` or `syscall.rs` with safe wrapper APIs.

### Key Components

**runtime.rs** - Unsafe Isolation Layer
- `_start()`: Naked entry point, 16-byte stack alignment, calls `main()` 
- `mmap_alloc` module: Bump allocator using mmap syscall (16MB heap)
- `create_waker()`: Creates dummy waker for executor (no actual wake support)
- `poll_boxed_future()`: Pins and polls futures (called by executor)
- `parse_cstring_usize()`, `read_ptr_array()`: Argv parsing helpers

**syscall.rs** - Direct Linux Syscalls  
- All functions use inline `asm!` for syscall instruction
- `write()`, `exit()`, `print_cstring()`, `mmap_alloc()`: Exposed as safe APIs
- `spawn_thread()`: Uses `clone` syscall with CLONE_VM|CLONE_FS|CLONE_FILES|CLONE_SIGHAND
- **No CLONE_THREAD**: Uses SIGCHLD process model (not pthread-style threads)

**executor.rs** - Minimal Task Executor
- Global `TASK_STORAGE`: `spin::Mutex<Vec<Option<Box<dyn Future>>>>` 
- `worker_trampoline()`: Polls tasks until none remain, then exits
- **No Pending support**: Tasks MUST complete on first poll or panic
- Workers spawned via `syscall::spawn_thread()` with 64KB stacks

**main.rs** - Application Entry
- `main(argc, argv)` never returns (calls `exit()`)
- Parses `argv[1]` for worker count (default: 16)
- Spawns 32 async tasks, starts workers, waits for completion

## Build System

### Target Configuration (NO custom JSON/linker needed)
- **Target**: `x86_64-unknown-none` (standard tier 2 target)
- **Relocation**: Static linking (`-C relocation-model=static -C code-model=large`)
- **Binary**: ELF 64-bit LSB executable, statically linked, NOT PIE
- `.cargo/config.toml` sets target and enables `build-std`

### Build Command
```bash
cargo +nightly build --release
# Auto-uses: -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem
```

### Run
```bash
./target/x86_64-unknown-none/release/async_futures_project [worker_count]
# Example: ./async_futures_project 4
```
## Edition 2024 Requirements

### Unsafe Annotations (MANDATORY)
- `#[no_mangle]` → `#[unsafe(no_mangle)]` (lines 65, 77 in runtime.rs)
- `#[naked]` → `#[unsafe(naked)]` (line 65 in runtime.rs)
- Inline `asm!` in unsafe fn → wrap in `unsafe { asm!(...) }` block
- Example: `syscall.rs` lines 5-16, 19-26, 119-131

## Hard Rules for AI Agents

### Unsafe Code Placement
1. **NEVER** add `unsafe` to `src/main.rs` or `src/executor.rs`
2. All `asm!` blocks MUST be in `syscall.rs`, wrapped as safe public functions
3. Waker/Future polling unsafe code goes in `runtime.rs` (see `create_waker()`, `poll_boxed_future()`)
4. When adding syscalls: write raw `asm!` in `syscall.rs`, expose safe wrapper

### Common Patterns

**Printing from safe code:**
```rust
use crate::syscall::write;
write(b"Hello\n");  // Safe wrapper over syscall 1 (write)
```

**Argv parsing:**
```rust
let arg_ptr = crate::runtime::read_ptr_array(argv, 1);
if let Some(n) = crate::runtime::parse_cstring_usize(arg_ptr) {
    // Use n
}
```

**Spawning async tasks:**
```rust
let executor = Executor::new();
executor.enqueue_task(Box::new(async move {
    write(b"Task running\n");
}))?;
executor.start_workers(4)?;
executor.wait_all();
```

## Critical Constraints

### Executor Limitations
- **No `Poll::Pending` support**: Tasks must complete on first poll or will panic
- Workers poll once per task, then discard completed tasks
- Use `async fn` or `async {}` blocks that complete immediately
- Task storage uses global spinlock - avoid heavy contention

### Thread Model
- `spawn_thread()` uses `clone` syscall with process-like semantics (SIGCHLD)
- Each worker gets 64KB stack via `mmap`
- Workers call `exit(0)` on completion (not pthread_exit)
- **Do NOT use CLONE_THREAD** - requires full TLS/pthread setup

### Memory Model  
- Bump allocator: 16MB heap, no deallocation
- All allocations permanent until process exit
- `GlobalAlloc::dealloc` is no-op

## Debugging Tips

**Segfault causes:**
- Stack misalignment in `_start()` (must be 16-byte aligned)
- Missing `unsafe { }` wrapper around `asm!` in edition 2024
- Incorrect `clone` syscall stack pointer

**Output interleaving:**
- Multiple `write()` calls from concurrent tasks interleave
- Solution: Build complete message in buffer, single `write()` call

**Task hangs:**
- Check if futures return `Poll::Pending` (will panic)
- Verify `TASKS_REMAINING` counter decrements correctly

## Adding Features

**New syscall example:**
```rust
// In syscall.rs
unsafe fn syscall_getpid() -> i32 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 39u64,  // __NR_getpid
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret as i32
}

pub fn getpid() -> i32 {
    unsafe { syscall_getpid() }
}
```

**New helper in runtime.rs:**
```rust
pub fn some_helper(x: usize) -> usize {
    unsafe {
        // Any unsafe operations needed
    }
}
```
