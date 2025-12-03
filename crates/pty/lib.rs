//! Minimal PTY helpers (stub)

#![no_std]

extern crate alloc;

// Provide a tiny stub for opening a pty pair. Real implementation would
// require ioctl/TIOCGPT etc; keep as a safe stub for now.

pub fn openpty() -> Result<(i32, i32), i32> {
    // Not implemented: return error
    Err(-1)
}
