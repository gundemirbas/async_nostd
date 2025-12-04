//! Async runtime core - modular architecture

#![no_std]

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;
use async_syscall as syscall;
use core::sync::atomic::{AtomicI32, Ordering};

mod config;
pub use config::*;

// Global log file descriptor - opened at startup
pub static LOG_FD: AtomicI32 = AtomicI32::new(1); // default to stdout

// Helper to write to log
#[inline(always)]
pub fn log_write(s: &[u8]) {
    let fd = LOG_FD.load(Ordering::Relaxed);
    let _ = syscall::write(fd, s);
}

mod allocator;
mod io_registry;
mod scheduler;

// Re-export public API
pub use io_registry::{close_eventfd, register_fd_waker, unregister_fd};
pub use scheduler::{
    create_waker, dump_scheduled, is_handle_scheduled, poll_task_safe, spawn, take_scheduled_task,
    wake_handle,
};

/// Ergonomic spawn helper - automatically boxes the future and wakes it
#[inline]
pub fn spawn_task<F>(future: F) -> usize
where
    F: core::future::Future<Output = ()> + Send + 'static,
{
    use alloc::boxed::Box;
    let handle = spawn(Box::new(future));
    wake_handle(handle);
    handle
}

// SIGCHLD handler installation
fn install_sigchld_handler() {
    // Set SIGCHLD to SIG_IGN so the kernel reaps child processes automatically.
    // SIG_IGN is represented by a non-null handler (1) in the raw sigaction.
    let sa_handler: u64 = 1; // SIG_IGN
    let sa_flags: u64 = 0;
    let sa_restorer: u64 = 0;
    let sa_mask = [0u64; 16];

    let mut sigaction = [0u64; 32];
    sigaction[0] = sa_handler;
    sigaction[1] = sa_flags;
    sigaction[2] = sa_restorer;
    sigaction[3..(3 + sa_mask.len())].copy_from_slice(&sa_mask);

    let _ = syscall::rt_sigaction(
        17,
        sigaction.as_ptr() as *const u8,
        core::ptr::null_mut(),
        8,
    );
}

pub fn ppoll_and_schedule() {
    // Reap exited child processes
    loop {
        let mut status: i32 = 0;
        let r = syscall::waitpid(-1, &mut status as *mut i32, 1);
        if r <= 0 {
            break;
        }
    }

    let snapshot: Vec<(i32, i16)> = {
        let reg = io_registry::IO_REG.lock();
        reg.iter().map(|e| (e.fd, e.events)).collect()
    };

    let evt = io_registry::ensure_eventfd();
    let mut fds: Vec<syscall::PollFd> = Vec::new();

    // Always include eventfd first (for task wake notifications)
    if evt >= 0 {
        fds.push(syscall::PollFd {
            fd: evt,
            events: 0x0001,
            revents: 0,
        });
    } else {
        // No eventfd - can't wait for tasks
        // Sleep briefly to avoid busy-wait
        let _ = syscall::nanosleep_ns(10_000_000); // 10ms
        return;
    }

    for (fd, ev) in snapshot.iter() {
        fds.push(syscall::PollFd {
            fd: *fd,
            events: *ev,
            revents: 0,
        });
    }

    // Use infinite timeout ppoll - blocks until events are ready
    let ret = syscall::ppoll(fds.as_mut_ptr(), fds.len());

    if ret <= 0 {
        // ppoll error or no events
        return;
    }

    // Drain eventfd
    if evt >= 0 && fds[0].revents != 0 {
        let mut buf = [0u8; 8];
        let _ = syscall::read(evt, &mut buf);
        io_registry::EVENT_PENDING.store(0, Ordering::Relaxed);
    }

    let start = if evt >= 0 { 1 } else { 0 };
    let mut ready_count = 0;
    for pf in fds.iter().skip(start) {
        // Diagnostic per-fd revents
        if pf.revents != 0 {
            ready_count += 1;
            // POLLERR=0x08, POLLHUP=0x10, POLLNVAL=0x20
            let is_closed = (pf.revents & 0x38) != 0;

            // Log which fd and what events
            log_write(b"[ppoll] fd=");
            syscall::write_usize(LOG_FD.load(Ordering::Relaxed), pf.fd as usize);
            log_write(b" revents=0x");
            syscall::write_hex(LOG_FD.load(Ordering::Relaxed), pf.revents as usize);
            log_write(b" closed=");
            syscall::write_usize(
                LOG_FD.load(Ordering::Relaxed),
                if is_closed { 1 } else { 0 },
            );
            log_write(b"\n");

            let mut to_wake: Vec<core::task::Waker> = Vec::new();
            {
                let mut reg = io_registry::IO_REG.lock();
                for i in 0..reg.len() {
                    if reg[i].fd == pf.fd {
                        // Take wakers (will be re-registered on next poll if needed)
                        core::mem::swap(&mut to_wake, &mut reg[i].waiters);

                        // Always remove entry - task will re-register if it needs to wait again
                        if is_closed {
                            log_write(b"[ppoll] removing closed fd=");
                            syscall::write_usize(LOG_FD.load(Ordering::Relaxed), pf.fd as usize);
                            log_write(b"\n");
                        }
                        reg.swap_remove(i);
                        break;
                    }
                }
            }
            // Wake tasks - they will add new wakers on next poll
            for w in to_wake {
                w.wake();
            }
        }
    }

    if ready_count > 0 {
        log_write(b"[ppoll] ");
        syscall::write_usize(LOG_FD.load(Ordering::Relaxed), ready_count);
        log_write(b" fds ready\n");
    }
}

// Utilities
/// Read a C-style argv array pointer at `index`.
///
/// # Safety
/// `ptr` must be a valid pointer to an array of `*const u8` values with at least
/// `index + 1` entries. The function performs a raw pointer offset and dereference.
pub unsafe fn read_ptr_array(ptr: *const *const u8, index: isize) -> *const u8 {
    unsafe { *ptr.offset(index) }
}

/// Parse a NUL-terminated ASCII decimal string into a `usize`.
///
/// # Safety
/// `s` must be a valid pointer to a NUL-terminated ASCII string accessible for
/// up to 64 bytes. The function reads memory from `s` until a NUL or non-digit
/// byte is found.
pub unsafe fn parse_cstring_usize(s: *const u8) -> Option<usize> {
    if s.is_null() {
        return None;
    }
    unsafe {
        let mut i: isize = 0;
        let mut acc: usize = 0;
        let mut any = false;
        while i < 64 {
            let c = *s.offset(i);
            if c == 0 || !c.is_ascii_digit() {
                break;
            }
            any = true;
            acc = acc * 10 + ((c - b'0') as usize);
            i += 1;
        }
        if any { Some(acc) } else { None }
    }
}

/// Create a Vec<u8> by copying `len` bytes from `ptr`.
///
/// # Safety
/// `ptr` must be valid for reads of `len` bytes. The function performs a raw
/// memory copy into a freshly allocated Vec.
pub unsafe fn vec_from_ptr(ptr: *const u8, len: usize) -> Vec<u8> {
    let mut v = vec![0u8; len];
    unsafe {
        core::ptr::copy_nonoverlapping(ptr, v.as_mut_ptr(), len);
    }
    v
}

pub fn vec_with_len(len: usize) -> Vec<u8> {
    vec![0u8; len]
}

pub fn set_vec_len(v: &mut Vec<u8>, new_len: usize) {
    v.truncate(new_len);
}

/// Parse a NUL-terminated IPv4 address (dotted decimal) into a u32 (little-endian).
///
/// # Safety
/// `s` must be a valid pointer to a NUL-terminated ASCII string containing a
/// dotted-decimal IPv4 address (e.g. "127.0.0.1"). The function reads up to
/// 128 bytes from `s` when parsing.
pub unsafe fn parse_cstring_ip(s: *const u8) -> Option<u32> {
    if s.is_null() {
        return None;
    }
    let mut i: isize = 0;
    let mut part: usize = 0;
    let mut acc: usize = 0;
    let mut parts = [0u8; 4];
    while i < 128 {
        let c = unsafe { *s.offset(i) };
        if c == 0 {
            if part != 3 {
                return None;
            }
            parts[part] = acc as u8;
            // Little-endian for x86_64 memory layout
            let v = (parts[0] as u32)
                | ((parts[1] as u32) << 8)
                | ((parts[2] as u32) << 16)
                | ((parts[3] as u32) << 24);
            return Some(v);
        }
        if c == b'.' {
            if part >= 3 {
                return None;
            }
            parts[part] = acc as u8;
            part += 1;
            acc = 0;
        } else if c.is_ascii_digit() {
            acc = acc * 10 + ((c - b'0') as usize);
            if acc > 255 {
                return None;
            }
        } else {
            return None;
        }
        i += 1;
    }
    None
}

// Entry point assembly - must be naked, no prologue
core::arch::global_asm!(
    ".section .text._start,\"ax\",@progbits",
    ".globl _start",
    ".type _start, @function",
    "_start:",
    "pop rdi",              // argc
    "mov rsi, rsp",         // argv
    "and rsp, ~15",         // align stack
    "xor rbp, rbp",         // clear frame pointer
    "call {main_trampoline}",
    "ud2",                  // should never return
    main_trampoline = sym main_trampoline
);

// OLD VERSION - remove this
/*
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text._start")]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::asm!(
        "pop rdi",              // argc
        "mov rsi, rsp",         // argv
        "and rsp, ~15",         // align
        "call {main}",
        main = sym main_trampoline,
        options(noreturn)
    )
}
*/

extern "C" fn main_trampoline(argc: isize, argv: *const *const u8) -> ! {
    install_sigchld_handler();

    let mut worker_count: usize = 16; // Default 16 workers
    let mut listen_ip: u32 = 0; // Default 0.0.0.0
    let mut listen_port: usize = 8000; // Default port 8000

    if argc > 1 {
        let arg = unsafe { read_ptr_array(argv, 1) };
        if let Some(n) = unsafe { parse_cstring_usize(arg) } {
            worker_count = n;
        }
    }
    if argc > 2 {
        let ip_arg = unsafe { read_ptr_array(argv, 2) };
        if let Some(v) = unsafe { parse_cstring_ip(ip_arg) } {
            listen_ip = v;
        }
    }
    if argc > 3 {
        let port_arg = unsafe { read_ptr_array(argv, 3) };
        if let Some(p) = unsafe { parse_cstring_usize(port_arg) } {
            listen_port = p;
        }
    }

    unsafe { crate::main(worker_count, listen_ip, listen_port) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    syscall::exit(1);
}

unsafe extern "C" {
    fn main(worker_count: usize, listen_ip: u32, listen_port: usize) -> !;
}
