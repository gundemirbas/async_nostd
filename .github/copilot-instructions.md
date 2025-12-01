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

**Why nightly**: Requires unstable features:
- `build-std`: Build core/alloc from source for custom target
- `#[unsafe(naked)]` attribute for `_start()`
- `naked_asm!` macro for naked functions

### Dependencies
- `futures = { version = "0.3", default-features = false, features = ["async-await"] }`
- Only `async-await` feature enabled (no executor or std features)

## Development Patterns

### Adding Unsafe Code
1. **Never** add unsafe to `main.rs` - it will fail compilation
2. Low-level syscalls → `syscall.rs`
3. Runtime/startup code → `runtime.rs`
4. Wrap unsafe operations in safe public APIs

Example:
```rust
// syscall.rs
unsafe fn syscall_write(fd: i32, buf: &[u8]) { /* asm */ }

pub fn write(buf: &[u8]) {
    unsafe { syscall_write(1, buf) }  // OK here
}

// main.rs
write(b"Hello");  // Safe API, no unsafe needed
```

### Adding Application Logic
- Keep it in `main.rs` with `#[forbid(unsafe_code)]`
- Use syscall wrappers: `write()`, `exit()`, `print_cstring()`
- Use runtime helpers: `Executor`, `read_ptr_array()`

### Async Code
- Use `futures` primitives: `ready()`, `Future` trait
- Poll via `Executor::block_on()` (simple single-poll executor)
- Note: Current executor panics on `Pending` - futures must complete immediately

## Testing
```bash
# Run with arguments
./target/x86_64-unknown-linux-none/release/async_futures_project arg1 arg2

# Binary is ~14KB, runs directly on Linux (uses syscalls)
```

## Common Issues

**"unsafe_code is denied"**: You tried to use unsafe in `main.rs` - move code to `runtime.rs` or `syscall.rs`

**"target not found"**: Need nightly toolchain + rust-src component:
```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
```

**Segfault on startup**: Check `_start()` assembly - stack alignment must be 16-byte

## Why This Architecture?
- **Safety**: Enforces unsafe isolation at compile time via `#[forbid(unsafe_code)]`
- **Clarity**: Physical module boundaries match safety boundaries
- **Bare-metal**: Demonstrates async/await without OS abstractions
- **Educational**: Shows exact syscall interface and program startup mechanics
