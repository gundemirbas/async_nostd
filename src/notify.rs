#![allow(dead_code)]
//! Lock-free FIFO notify primitive (Michael-Scott queue) used by the runtime.

use core::task::Waker;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use core::ptr;
extern crate alloc;
use alloc::boxed::Box;

struct WaiterNode {
    waker: Option<Box<Waker>>,
    next: AtomicPtr<WaiterNode>,
}

static HEAD: AtomicPtr<WaiterNode> = AtomicPtr::new(ptr::null_mut());
static TAIL: AtomicPtr<WaiterNode> = AtomicPtr::new(ptr::null_mut());
static COUNT: AtomicUsize = AtomicUsize::new(0);

fn init_queue_if_needed() {
    if HEAD.load(Ordering::Acquire).is_null() {
        let dummy = Box::into_raw(Box::new(WaiterNode { waker: None, next: AtomicPtr::new(ptr::null_mut()) }));
        HEAD.store(dummy, Ordering::Release);
        TAIL.store(dummy, Ordering::Release);
    }
}

pub struct Notify;

impl Notify {
    pub const fn new() -> Self { Notify }

    /// Register a waker and, if `total` registrations reached, wake all
    /// registered waiters in FIFO order.
    pub fn register_and_maybe_wake_all(&self, _id: usize, total: usize, waker: Waker) {
        init_queue_if_needed();

        // create node
        let node = Box::into_raw(Box::new(WaiterNode { waker: Some(Box::new(waker)), next: AtomicPtr::new(ptr::null_mut()) }));

        // Enqueue: swap tail and set prev.next -> node
        let prev = TAIL.swap(node, Ordering::AcqRel);
        unsafe { (*prev).next.store(node, Ordering::Release); }

        let reg = COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        if reg >= total {
            // Dequeue all nodes (FIFO) and wake them
            let dummy = HEAD.swap(ptr::null_mut(), Ordering::AcqRel);
            // reset tail to dummy
            TAIL.store(dummy, Ordering::Release);
            COUNT.store(0, Ordering::SeqCst);

            // iterate from dummy.next
            let mut cur = unsafe { (*dummy).next.load(Ordering::Acquire) };
            while !cur.is_null() {
                let next = unsafe { (*cur).next.load(Ordering::Acquire) };
                // take boxed waker and wake
                let boxed = unsafe { Box::from_raw(cur) };
                if let Some(w) = boxed.waker { w.wake_by_ref(); }
                cur = next;
            }
            // drop dummy
            unsafe { let _ = Box::from_raw(dummy); }
        }
    }

    /// Drain and drop any queued waiters and reset the queue.
    pub fn reset(&self) {
        let head = HEAD.swap(ptr::null_mut(), Ordering::AcqRel);
        COUNT.store(0, Ordering::SeqCst);
        if head.is_null() { return; }
        // drain list
        let mut cur = unsafe { (*head).next.load(Ordering::Acquire) };
        while !cur.is_null() {
            let next = unsafe { (*cur).next.load(Ordering::Acquire) };
            unsafe { let _ = Box::from_raw(cur); }
            cur = next;
        }
        unsafe { let _ = Box::from_raw(head); }
        TAIL.store(ptr::null_mut(), Ordering::Release);
    }
}

pub static NOTIFY: Notify = Notify::new();
