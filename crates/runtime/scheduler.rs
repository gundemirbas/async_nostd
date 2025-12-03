//! Task scheduler - Treiber stack for lock-free scheduling

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::future::Future;
use async_syscall as sys;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

struct Node {
    handle: usize,
    next: *mut Node,
}

static SCHEDULE_HEAD: AtomicPtr<Node> = AtomicPtr::new(core::ptr::null_mut());
static FREELIST_HEAD: AtomicPtr<Node> = AtomicPtr::new(core::ptr::null_mut());
static FREELIST_COUNT: AtomicUsize = AtomicUsize::new(0);
use crate::config::FREELIST_MAX;

// Per-slot task storage
use crate::config::MAX_TASK_SLOTS;

struct Slot {
    generation: AtomicUsize,
    inner: spin::Mutex<Option<Box<dyn Future<Output = ()> + Send>>>,
}

impl Slot {
    fn new() -> Self {
        Self {
            generation: AtomicUsize::new(0),
            inner: spin::Mutex::new(None),
        }
    }
}

static SLOTS: spin::Mutex<Option<Vec<Slot>>> = spin::Mutex::new(None);
static FREE_SLOTS: spin::Mutex<Vec<usize>> = spin::Mutex::new(Vec::new());

fn alloc_node(handle: usize) -> *mut Node {
    loop {
        let head = FREELIST_HEAD.load(Ordering::Acquire);
        if head.is_null() {
            return Box::into_raw(Box::new(Node {
                handle,
                next: core::ptr::null_mut(),
            }));
        }
        let next = unsafe { (*head).next };
        if FREELIST_HEAD
            .compare_exchange(head, next, Ordering::Release, Ordering::Acquire)
            .is_ok()
        {
            FREELIST_COUNT.fetch_sub(1, Ordering::Relaxed);
            unsafe {
                (*head).handle = handle;
                (*head).next = core::ptr::null_mut();
            }
            return head;
        }
    }
}

fn free_node(node: *mut Node) {
    if FREELIST_COUNT.load(Ordering::Relaxed) < FREELIST_MAX {
        loop {
            let head = FREELIST_HEAD.load(Ordering::Acquire);
            unsafe { (*node).next = head };
            if FREELIST_HEAD
                .compare_exchange(head, node, Ordering::Release, Ordering::Acquire)
                .is_ok()
            {
                FREELIST_COUNT.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    } else {
        let _ = unsafe { Box::from_raw(node) };
    }
}

pub fn register_task(task: Box<dyn Future<Output = ()> + Send>) -> usize {
    {
        let mut slots_guard = SLOTS.lock();
        if slots_guard.is_none() {
            let mut v = Vec::new();
            for _ in 0..MAX_TASK_SLOTS {
                v.push(Slot::new());
            }
            *slots_guard = Some(v);
        }
    }

    let slot_idx = {
        let mut free = FREE_SLOTS.lock();
        if let Some(idx) = free.pop() {
            idx
        } else {
            drop(free);
            let slots_guard = SLOTS.lock();
            let slots = slots_guard.as_ref().unwrap();
            let mut found_idx = None;
            for (i, slot) in slots.iter().enumerate() {
                if slot.inner.lock().is_none() {
                    found_idx = Some(i);
                    break;
                }
            }
            drop(slots_guard);
            found_idx.expect("No free task slots")
        }
    };

    let slots_guard = SLOTS.lock();
    let slots = slots_guard.as_ref().unwrap();
    let generation = slots[slot_idx].generation.fetch_add(1, Ordering::Relaxed);
    *slots[slot_idx].inner.lock() = Some(task);

    (slot_idx << 32) | (generation & 0xFFFFFFFF)
}

pub fn wake_handle(handle: usize) {
    let node = alloc_node(handle);
    loop {
        let head = SCHEDULE_HEAD.load(Ordering::Acquire);
        unsafe { (*node).next = head };
        if SCHEDULE_HEAD
            .compare_exchange(head, node, Ordering::Release, Ordering::Acquire)
            .is_ok()
        {
                // Wake the eventfd to notify pollers that work is available.
                crate::io_registry::signal_eventfd();
                return;
        }
    }
}

pub fn take_scheduled_task() -> Option<usize> {
    loop {
        let head = SCHEDULE_HEAD.load(Ordering::Acquire);
        if head.is_null() {
            return None;
        }
        let next = unsafe { (*head).next };
        if SCHEDULE_HEAD
            .compare_exchange(head, next, Ordering::Release, Ordering::Acquire)
            .is_ok()
        {
            let handle = unsafe { (*head).handle };
            // taken handle
            free_node(head);
            return Some(handle);
        }
    }
}

/// Check whether a handle is currently present in the scheduled Treiber stack.
pub fn is_handle_scheduled(target: usize) -> bool {
    let mut cur = SCHEDULE_HEAD.load(Ordering::Acquire);
    let mut seen = false;
    let mut steps = 0;
    while !cur.is_null() && steps < 4096 {
        unsafe {
            if (*cur).handle == target {
                seen = true;
                break;
            }
            cur = (*cur).next;
        }
        steps += 1;
    }
    seen
}

/// Dump up to `limit` handles from the scheduled stack (for diagnostics).
pub fn dump_scheduled(limit: usize) {
    let mut cur = SCHEDULE_HEAD.load(Ordering::Acquire);
    let mut i = 0;
    while !cur.is_null() && i < limit {
        unsafe {
            let h = (*cur).handle;
            let _ = sys::write(1, b"handle: ");
            sys::write_usize(1, h);
            let _ = sys::write(1, b"\n");
            cur = (*cur).next;
        }
        i += 1;
    }
}

pub fn poll_task_safe(handle: usize, cx: &mut Context<'_>) -> Poll<()> {
    // Poll the task associated with `handle`.
    let slot_idx = (handle >> 32) & 0x3FF;
    let generation = handle & 0xFFFFFFFF;

    let slots_guard = SLOTS.lock();
    let slots = slots_guard.as_ref().unwrap();
    if slot_idx >= slots.len() {
        return Poll::Ready(());
    }

    let cur_generation = slots[slot_idx].generation.load(Ordering::Relaxed);
    if cur_generation != generation && cur_generation != generation + 1 {
        return Poll::Ready(());
    }

    let mut guard = slots[slot_idx].inner.lock();
    if let Some(task) = guard.as_mut() {
        let pin = unsafe { core::pin::Pin::new_unchecked(task.as_mut()) };
        let result = pin.poll(cx);
        if matches!(result, Poll::Ready(_)) {
            *guard = None;
            drop(guard);
            FREE_SLOTS.lock().push(slot_idx);
        }
        // poll result handled by caller
        result
    } else {
        Poll::Ready(())
    }
}

unsafe fn clone_waker(data: *const ()) -> RawWaker {
    RawWaker::new(data, &VTABLE)
}

unsafe fn wake_by_ref(data: *const ()) {
    let handle = data as usize;
    wake_handle(handle);
}

unsafe fn wake_waker(data: *const ()) {
    unsafe {
        wake_by_ref(data);
    }
}

unsafe fn drop_waker(_data: *const ()) {}

static VTABLE: RawWakerVTable =
    RawWakerVTable::new(clone_waker, wake_waker, wake_by_ref, drop_waker);

pub fn create_waker(handle: usize) -> Waker {
    let raw = RawWaker::new(handle as *const (), &VTABLE);
    unsafe { Waker::from_raw(raw) }
}
