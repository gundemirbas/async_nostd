//! IO event registry for async file descriptor operations

use crate::syscall;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use core::task::Waker;

pub struct IoEntry {
    pub fd: i32,
    pub events: i16,
    pub waiters: Vec<Waker>,
}

pub static IO_REG: spin::Mutex<Vec<IoEntry>> = spin::Mutex::new(Vec::new());
pub static EVENTFD: AtomicI32 = AtomicI32::new(-1);
pub static EVENT_PENDING: AtomicUsize = AtomicUsize::new(0);

pub fn ensure_eventfd() -> i32 {
    let cur = EVENTFD.load(Ordering::Relaxed);
    if cur >= 0 {
        return cur;
    }
    let fd = syscall::eventfd(0, 0);
    if fd >= 0 {
        if EVENTFD
            .compare_exchange(-1, fd, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            let _ = syscall::close(fd);
        }
        return EVENTFD.load(Ordering::Relaxed);
    }
    -1
}

pub fn signal_eventfd() {
    let fd = ensure_eventfd();
    if fd < 0 {
        return;
    }
    let v: u64 = 1;
    let _ = syscall::write(fd, unsafe {
        core::slice::from_raw_parts(&v as *const u64 as *const u8, 8)
    });
}

pub fn close_eventfd() {
    let fd = EVENTFD.swap(-1, Ordering::Relaxed);
    if fd >= 0 {
        let _ = syscall::close(fd);
    }
}

pub fn register_fd_waker(fd: i32, events: i16, waker: Waker) {
    let mut reg = IO_REG.lock();
    for e in reg.iter_mut() {
        if e.fd == fd {
            e.waiters.push(waker);
            return;
        }
    }
    let v = alloc::vec![waker];
    reg.push(IoEntry {
        fd,
        events,
        waiters: v,
    });
}

pub fn unregister_fd(fd: i32) {
    let mut reg = IO_REG.lock();
    reg.retain(|e| e.fd != fd);
    drop(reg); // Release lock before signal

    // Signal eventfd to wake up ppoll and refresh fd list
    signal_eventfd();
}
