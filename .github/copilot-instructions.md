# Async Futures Project - Copilot Instructions

## Project Overview
This is a **bare-metal Rust project** targeting `x86_64-unknown-linux-none` with `#![no_std]` and custom syscalls. It demonstrates async/await with futures in a freestanding environment without standard library support.

## Architecture

### Three-Layer Design (Strict Unsafe Isolation)
```
src/main.rs (89 lines)      → Application logic - #[forbid(unsafe_code)]
src/runtime.rs (92 lines)   → Program startup & executor - unsafe allowed
src/syscall.rs (76 lines)   → System calls - unsafe allowed
```

**Critical Rule**: `main.rs` uses `#[forbid(unsafe_code)]` - ALL unsafe operations must be in `runtime.rs` or `syscall.rs`.

### Key Components

**runtime.rs** - Program Bootstrap
- `_start()`: Naked function, sets up stack, jumps to main
- `#[no_mangle] main()`: C ABI trampoline that calls Rust's `main()`
- `#[panic_handler]`: Required for no_std
- `Executor`: Simple future executor (polls once, doesn't handle Pending)

**syscall.rs** - Direct Linux Syscalls
- `write()`, `exit()`, `print_cstring()`: Safe wrappers over raw syscalls
- Uses inline assembly (`core::arch::asm!`) for syscall interface
- All syscalls use `unsafe` internally, exposed as safe functions

**main.rs** - Pure Application Logic
- `main(argc, argv)`: Entry point (called from runtime trampoline)
- Uses only safe APIs from runtime/syscall modules
- Demonstrates async/await with `futures` crate (no_std mode)

## Build System

### Custom Target Specification
- `x86_64-unknown-linux-none.json`: Custom LLVM target (kernel mode, no red zone)
- `linker.ld`: Custom linker script (entry at 0x400000)
- `.cargo/config.toml`: Forces custom target + build-std

### Build Command
```bash
cargo +nightly build --release -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem
```

### Adding Unsafe Code
1. **Never** add unsafe to `main.rs` - it will fail compilation
## Copilot / AI agent guide — Async Futures Project

Quick orientation
- Purpose: a freestanding `no_std` Rust binary for x86_64 Linux demonstrating async/await with direct syscalls.
- Key files: `src/main.rs` (application logic — no unsafe), `src/runtime.rs` (startup, allocator, helpers), `src/syscall.rs` (inline asm syscalls), `src/executor.rs` (executor), `x86_64-unknown-linux-none.json`, `linker.ld`, `.cargo/config.toml`.

Hard rules for agents
- Never add `unsafe` to `src/main.rs` — it has `#[forbid(unsafe_code)]`.
- Put raw `asm!` and syscall boundary code in `src/syscall.rs` and expose safe wrappers.
- Put startup and allocator `unsafe` in `src/runtime.rs` only.

Build & run (exact)
```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
cargo +nightly build --release -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem
./target/x86_64-unknown-linux-none/release/async_futures_project <worker_count?>
```

Project-specific patterns (do this here)
- Printing: use `crate::syscall::write(b"...\n")` or `print_cstring(ptr)` for argv strings.
- Reading argv: `runtime::read_ptr_array(argv, idx)` returns `*const u8`.
- Worker count: `main` parses `argv[1]` as decimal; default is `16`.
- Spawn tasks: `executor.enqueue_task(Box::new(async move { /* ... */ }))` and `executor.start_workers(n)`.

Executor constraints
- The executor is minimal: workers poll each boxed future once and expect it to complete (no `Waker`/`Pending` support). A future returning `Poll::Pending` will panic.
- Task storage uses a `spin::Mutex<Option<Vec<Option<Box<dyn Future<Output=()>>>>>>` global — acceptable for this demo but avoid heavy contention.

Syscall & thread notes
- `src/syscall.rs:spawn_thread` uses raw `clone` syscall. Do not introduce `CLONE_THREAD` without full thread runtime; current flags use SIGCHLD-like behavior.
- To add syscalls: place inline `asm!` in `syscall.rs` and wrap with a safe `pub fn`.

Agent-first steps
1. Run the exact nightly build above to validate toolchain.
2. Inspect `src/runtime.rs` and `src/syscall.rs` before editing any unsafe code.
3. Add small feature by editing `main.rs` (safe code) and using syscall/runtime APIs.

Gotchas & tips
- Output interleaving: tasks call `write()` multiple times (e.g., separate writes for "Task ", id, newline). To avoid interleaving build a single buffer and call `write()` once.
- `_start()` must align the stack to 16 bytes — incorrect alignment causes segfaults.

If you want more detail (how to implement wake support, structured tests, or a safer task queue), tell me which area to expand and I will update this doc.
