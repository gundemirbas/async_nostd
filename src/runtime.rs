//! Runtime module

use core::panic::PanicInfo;
extern crate alloc;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicUsize, AtomicPtr, Ordering, AtomicI32};
use core::task::Waker;

// Task table and scheduling primitives
static TASK_TABLE: spin::Mutex<Option<alloc::vec::Vec<Option<alloc::boxed::Box<dyn core::future::Future<Output = ()> + Send + 'static>>>>> =
    spin::Mutex::new(None);

const TASK_TABLE_TRIM_THRESHOLD: usize = 8;
const NODE_FREELIST_CAP: usize = 256;

static NEXT_HANDLE: AtomicUsize = AtomicUsize::new(1);

struct Node { handle: usize, next: *mut Node }
static SCHEDULE_HEAD: AtomicPtr<Node> = AtomicPtr::new(core::ptr::null_mut());
static FREELIST_HEAD: AtomicPtr<Node> = AtomicPtr::new(core::ptr::null_mut());
static NODE_FREELIST_COUNT: AtomicUsize = AtomicUsize::new(0);

// IO registry entry for poll-based readiness notifications.
struct IoEntry {
    fd: i32,
    events: i16,
    waiters: alloc::vec::Vec<Waker>,
}

static IO_REG: spin::Mutex<alloc::vec::Vec<IoEntry>> = spin::Mutex::new(alloc::vec::Vec::new());

// Eventfd used to block worker threads efficiently when idle. -1 means uninitialized.
static EVENTFD: AtomicI32 = AtomicI32::new(-1);
static EVENT_PENDING: AtomicUsize = AtomicUsize::new(0);
static EXTRA_WAIT_FD: AtomicI32 = AtomicI32::new(-1);

fn ensure_eventfd() -> i32 {
    let cur = EVENTFD.load(Ordering::SeqCst);
    if cur >= 0 {
        return cur;
    }
    // Try to create an eventfd (eventfd2 with flags=0). On error, return -1.
    let fd = crate::syscall::eventfd(0, 0);
    if fd >= 0 {
        // Try to store for other threads; if we lose the race, close our fd to avoid leaks.
        if EVENTFD.compare_exchange(-1, fd, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            // another thread set it; close our fd
            let _ = crate::syscall::close(fd);
        }
        return EVENTFD.load(Ordering::SeqCst);
    }
    -1
}

/// Return the runtime eventfd, creating it if necessary.
pub fn get_eventfd() -> i32 { ensure_eventfd() }

/// Set an extra fd that workers should wait on (e.g., a pipe read end for demos).
pub fn set_extra_wait_fd(fd: i32) { EXTRA_WAIT_FD.store(fd, Ordering::SeqCst); }

/// Get the extra wait fd, or -1 if none.
#[allow(dead_code)]
pub fn get_extra_wait_fd() -> i32 { EXTRA_WAIT_FD.load(Ordering::SeqCst) }

fn signal_eventfd() {
    let fd = ensure_eventfd();
    if fd < 0 { return; }
    let v: u64 = 1;
    // write 8 bytes
    let bytes = unsafe { core::slice::from_raw_parts((&v as *const u64) as *const u8, 8) };
    let _ = crate::syscall::write_fd(fd, bytes);
}

#[allow(dead_code)]
pub fn wait_eventfd() {
    let fd = ensure_eventfd();
    if fd < 0 { return; }
    let mut v: u64 = 0;
    let buf = unsafe { core::slice::from_raw_parts_mut((&mut v as *mut u64) as *mut u8, 8) };
    // Block until value available (read may return >0)
    let r = crate::syscall::read_fd(fd, buf);
    if r <= 0 {
        return;
    }
    let read_cnt = v as usize;
    // Subtract the read count from pending; if there are remaining pending
    // signals (wakes that happened while we were processing), write again
    // to ensure workers wake for them.
    let prev = EVENT_PENDING.fetch_sub(read_cnt, Ordering::SeqCst);
    if prev > read_cnt {
        // there are still pending signals; ensure eventfd has a notification
        signal_eventfd();
    }
}

/// Close the runtime eventfd (if any) and mark it closed.
pub fn close_eventfd() {
    // Swap to -1 so other threads know it's closed.
    let fd = EVENTFD.swap(-1, Ordering::SeqCst);
    if fd >= 0 {
        let _ = crate::syscall::close(fd);
    }
}

mod mmap_alloc {
    use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::{AtomicUsize, Ordering};
    use core::ptr::null_mut;

    const HEAP_SIZE: usize = 16 * 1024 * 1024;

    static HEAP_START: AtomicUsize = AtomicUsize::new(0);
    static HEAP_CUR: AtomicUsize = AtomicUsize::new(0);
    static HEAP_END: AtomicUsize = AtomicUsize::new(0);

    unsafe fn init_heap() {
        if HEAP_START.load(Ordering::SeqCst) == 0 {
            let ptr = crate::syscall::mmap_alloc(HEAP_SIZE);
            if !ptr.is_null() {
                let s = ptr as usize;
                HEAP_START.store(s, Ordering::SeqCst);
                HEAP_CUR.store(s, Ordering::SeqCst);
                HEAP_END.store(s + HEAP_SIZE, Ordering::SeqCst);
            }
        }
    }

    #[inline]
    fn align_up(x: usize, align: usize) -> usize {
        (x + align - 1) & !(align - 1)
    }

    pub struct MmapAllocator;

    unsafe impl GlobalAlloc for MmapAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            unsafe { init_heap(); }
            let align = layout.align().max(1);
            let size = layout.size();
            if size == 0 {
                return align as *mut u8;
            }

            loop {
                let cur = HEAP_CUR.load(Ordering::SeqCst);
                let aligned = align_up(cur, align);
                let next = aligned.checked_add(size).unwrap_or(usize::MAX);
                let end = HEAP_END.load(Ordering::SeqCst);
                if next > end {
                    return null_mut();
                }
                if HEAP_CUR.compare_exchange(cur, next, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                    return aligned as *mut u8;
                }
            }
        }

        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
    }

    #[global_allocator]
    static ALLOC: MmapAllocator = MmapAllocator;
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "xor rbp, rbp",
        "pop rdi",
        "mov rsi, rsp",
        "and rsp, ~0xf",
        "push 0",
        "call {main}",
        main = sym main,
    )
}
#[unsafe(no_mangle)]
pub extern "C" fn main(argc: isize, argv: *const *const u8) -> ! {
    crate::main(argc, argv)
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    crate::syscall::exit(1);
}

pub fn read_ptr_array(ptr: *const *const u8, index: isize) -> *const u8 {
    unsafe { *ptr.offset(index) }
}

pub fn parse_cstring_usize(s: *const u8) -> Option<usize> {
    if s.is_null() {
        return None;
    }
    unsafe {
        let mut i: isize = 0;
        let mut acc: usize = 0;
        let mut any = false;
        while i < 64 {
            let c = *s.offset(i);
            if c == 0 { break; }
            if c < b'0' || c > b'9' { break; }
            any = true;
            acc = acc * 10 + ((c - b'0') as usize);
            i += 1;
        }
        if any { Some(acc) } else { None }
    }
}

// Backwards-compatible wrapper: notify implementation moved to `crate::notify`.
// `notify` primitive removed — no public shim provided anymore.

/// Register a `Waker` to be notified when `fd` has `events` ready (POLLIN/POLLOUT).
pub fn register_fd_waker(fd: i32, events: i16, waker: core::task::Waker) {
    let mut reg = IO_REG.lock();
    for e in reg.iter_mut() {
        if e.fd == fd {
            e.waiters.push(waker);
            return;
        }
    }
    // not found -> insert new entry
    let mut v = alloc::vec::Vec::new();
    v.push(waker);
    reg.push(IoEntry { fd, events, waiters: v });
}

/// Build pollfd array from IO_REG + eventfd, call `ppoll`, and schedule/ wake registered wakers for ready fds.
pub fn ppoll_and_schedule() {
    use crate::syscall::PollFd;
    use crate::syscall::ppoll;

    // Snapshot IO_REG fds
    let snapshot: alloc::vec::Vec<(i32, i16)> = {
        let reg = IO_REG.lock();
        let mut s = alloc::vec::Vec::new();
        for e in reg.iter() { s.push((e.fd, e.events)); }
        s
    };

    // Prepare pollfds: first eventfd to be signalled by wake_handle
    let evt = ensure_eventfd();
    let mut fds: alloc::vec::Vec<PollFd> = alloc::vec::Vec::new();
    if evt >= 0 {
        fds.push(PollFd { fd: evt, events: 0x0001, revents: 0 });
    }
    for (fd, ev) in snapshot.iter() {
        fds.push(PollFd { fd: *fd, events: *ev, revents: 0 });
    }

    if fds.is_empty() {
        // nothing to wait on, just return
        return;
    }

    // Call ppoll with NULL timeout
    let ret = ppoll(fds.as_mut_ptr(), fds.len(), core::ptr::null(), core::ptr::null(), 0);
    if ret <= 0 { return; }

    // Handle ready fds (skip index 0 if eventfd present)
    let start = if evt >= 0 { 1 } else { 0 };
    for (_idx, pf) in fds.iter().enumerate().skip(start) {
        if pf.revents != 0 {
            // drain waiters for this fd
            let mut to_wake: alloc::vec::Vec<Waker> = alloc::vec::Vec::new();
            {
                let mut reg = IO_REG.lock();
                for i in 0..reg.len() {
                    if reg[i].fd == pf.fd {
                        core::mem::swap(&mut to_wake, &mut reg[i].waiters);
                        break;
                    }
                }
            }
            for w in to_wake.into_iter() {
                w.wake();
            }
        }
    }
}

// Helper: ensure table exists and return the guard
fn lock_table() -> spin::MutexGuard<'static, Option<alloc::vec::Vec<Option<alloc::boxed::Box<dyn core::future::Future<Output = ()> + Send + 'static>>>>> {
    TASK_TABLE.lock()
}

/// Register a task into the runtime table and schedule it for polling.
pub fn register_task(task: alloc::boxed::Box<dyn core::future::Future<Output = ()> + Send + 'static>) -> usize {
    let mut table_guard = lock_table();
    if table_guard.is_none() {
        *table_guard = Some(alloc::vec::Vec::new());
    }
    let table = table_guard.as_mut().unwrap();

    // Try to reuse a handle from the lock-free freelist first.
    let handle = if let Some(h) = pop_handle() {
        h
    } else {
        NEXT_HANDLE.fetch_add(1, Ordering::SeqCst)
    };

    let idx = handle - 1;
    while (idx as usize) >= table.len() {
        table.push(None);
    }
    table[idx as usize] = Some(task);
    drop(table_guard);
    // schedule: push handle onto the lock-free stack
    wake_handle(handle);
    handle
}

/// Take next scheduled task handle, if any.
pub fn take_scheduled_task() -> Option<usize> {
    // Pop a node from the Treiber stack
    loop {
        let head = SCHEDULE_HEAD.load(Ordering::Acquire);
        if head.is_null() {
            return None;
        }
        let next = unsafe { (*head).next };
        if SCHEDULE_HEAD.compare_exchange(head, next, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            // We have exclusive ownership of `head` pointer now.
            // Read the handle, recycle the node into the freelist, and return the handle.
            let handle = unsafe { (*head).handle };
            free_node(head);
            return Some(handle);
        }
    }
}

/// Wake a task by handle (schedule it for polling).
pub fn wake_handle(handle: usize) {
    // Allocate or reuse a node, then push it onto the lock-free Treiber stack
    let node = alloc_node(handle);
    loop {
        let head = SCHEDULE_HEAD.load(Ordering::Acquire);
        unsafe { (*node).next = head; }
        if SCHEDULE_HEAD.compare_exchange(head, node, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            break;
        }
    }
    // Coalesce eventfd writes: increment pending and only signal when transitioning 0->1.
    let prev = EVENT_PENDING.fetch_add(1, Ordering::SeqCst);
    if prev == 0 {
        signal_eventfd();
    }
}

/// Pop a node from the freelist or allocate a new one if empty.
fn alloc_node(handle: usize) -> *mut Node {
    loop {
        let head = FREELIST_HEAD.load(Ordering::Acquire);
        if head.is_null() {
            let b = Box::new(Node { handle, next: core::ptr::null_mut() });
            return Box::into_raw(b);
        }
        let next = unsafe { (*head).next };
        if FREELIST_HEAD.compare_exchange(head, next, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            // decrement freelist count since we're taking one out
            NODE_FREELIST_COUNT.fetch_sub(1, Ordering::AcqRel);
            unsafe {
                (*head).handle = handle;
                (*head).next = core::ptr::null_mut();
            }
            return head;
        }
    }
}

/// Return a node pointer to the freelist for reuse.
fn free_node(ptr: *mut Node) {
    // Try to push into freelist up to NODE_FREELIST_CAP entries. If full,
    // free the node instead of caching it.
    let prev = NODE_FREELIST_COUNT.fetch_add(1, Ordering::AcqRel);
    if prev >= NODE_FREELIST_CAP {
        // freelist full, drop the node and undo the count
        NODE_FREELIST_COUNT.fetch_sub(1, Ordering::AcqRel);
        unsafe { let _ = Box::from_raw(ptr); }
        return;
    }

    loop {
        let head = FREELIST_HEAD.load(Ordering::Acquire);
        unsafe { (*ptr).next = head; }
        if FREELIST_HEAD.compare_exchange(head, ptr, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            break;
        }
    }
}

// ---- Handle recycling using Node freelist (zero-allocation when possible) ----
fn pop_handle() -> Option<usize> {
    // Try to pop a cached Node from the FREELIST_HEAD and return its stored handle.
    loop {
        let head = FREELIST_HEAD.load(Ordering::Acquire);
        if head.is_null() {
            return None;
        }
        let next = unsafe { (*head).next };
        if FREELIST_HEAD.compare_exchange(head, next, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            // We own `head` now. Decrement cached count and extract handle.
            NODE_FREELIST_COUNT.fetch_sub(1, Ordering::AcqRel);
            let h = unsafe { (*head).handle };
            // drop the Node memory (we're only returning the handle value)
            unsafe { let _ = Box::from_raw(head); }
            return Some(h);
        }
    }
}

fn push_handle(handle: usize) {
    // To avoid allocating, try to reuse an existing cached Node object from FREELIST_HEAD.
    // If none available, skip recycling (NEXT_HANDLE will grow instead).
    loop {
        let head = FREELIST_HEAD.load(Ordering::Acquire);
        if head.is_null() {
            // no cached nodes to reuse; skip recycling to avoid allocation
            return;
        }
        let next = unsafe { (*head).next };
        if FREELIST_HEAD.compare_exchange(head, next, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            // Reuse the node we popped: fill it with the handle and push it back.
            NODE_FREELIST_COUNT.fetch_sub(1, Ordering::AcqRel);
            unsafe {
                (*head).handle = handle;
                (*head).next = core::ptr::null_mut();
            }
            // push back
            loop {
                let h = FREELIST_HEAD.load(Ordering::Acquire);
                unsafe { (*head).next = h; }
                if FREELIST_HEAD.compare_exchange(h, head, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    NODE_FREELIST_COUNT.fetch_add(1, Ordering::AcqRel);
                    break;
                }
            }
            return;
        }
    }
}

/// Create a `Waker` that will wake the given handle when called.
pub unsafe fn create_waker_for_handle(handle: usize) -> core::task::Waker {
    use core::task::{RawWaker, RawWakerVTable, Waker};

    unsafe fn clone(data: *const ()) -> RawWaker { RawWaker::new(data, &VTABLE) }
    unsafe fn wake(data: *const ()) {
        let h = data as usize;
        wake_handle(h);
    }
    unsafe fn wake_by_ref(data: *const ()) {
        let h = data as usize;
        wake_handle(h);
    }
    unsafe fn drop(_: *const ()) {}

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

    let data = handle as *const ();
    unsafe { Waker::from_raw(RawWaker::new(data, &VTABLE)) }
}

/// Poll the task for the given handle. If it returns `Pending`, the task is left
/// in the table; if `Ready` the task is dropped. This function performs the
/// unsafe `Pin::new_unchecked` and must live in `runtime.rs`.
pub unsafe fn poll_task(handle: usize, cx: &mut core::task::Context<'_>) -> core::task::Poll<()> {
    let mut table_guard = lock_table();
    if table_guard.is_none() {
        return core::task::Poll::Ready(());
    }
    let table = table_guard.as_mut().unwrap();
    let idx = handle - 1;
    if (idx as usize) >= table.len() {
        return core::task::Poll::Ready(());
    }
    if table[idx as usize].is_none() {
        return core::task::Poll::Ready(());
    }
    // take the Box out for polling
    let mut task = table[idx as usize].take().unwrap();
    // Drop the table lock while polling to avoid deadlocks if the polled
    // future calls back into runtime APIs (like registering tasks).
    drop(table_guard);
    use core::pin::Pin;
    let res = {
        // Pin::new_unchecked is unsafe, keep explicit unsafe block
        unsafe {
            let mut pinned = Pin::new_unchecked(task.as_mut());
            pinned.as_mut().poll(cx)
        }
    };

    match res {
        core::task::Poll::Ready(()) => {
            // Task completed — drop it and then re-acquire table lock to update state.
            drop(task);
            let mut table_guard = lock_table();
            if table_guard.is_none() {
                return core::task::Poll::Ready(());
            }
            let table = table_guard.as_mut().unwrap();

            // Try to compact the task table by removing trailing `None` slots.
            let mut trailing = 0usize;
            for i in (0..table.len()).rev() {
                if table[i].is_none() {
                    trailing += 1;
                } else {
                    break;
                }
            }
            if trailing >= TASK_TABLE_TRIM_THRESHOLD {
                for _ in 0..trailing {
                    table.pop();
                }
            }
            // recycle handle id for reuse (lock-free)
            push_handle(handle);
            core::task::Poll::Ready(())
        }
        core::task::Poll::Pending => {
            // re-acquire lock and put task back into table
            let mut table_guard = lock_table();
            if table_guard.is_none() {
                return core::task::Poll::Ready(());
            }
            let table = table_guard.as_mut().unwrap();
            table[idx as usize] = Some(task);
            core::task::Poll::Pending
        }
    }
}
 