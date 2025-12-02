//! Async runtime core

#![no_std]

extern crate alloc;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::future::Future;
use core::sync::atomic::{AtomicUsize, AtomicPtr, AtomicI32, Ordering};
use core::task::Waker;
use async_syscall as syscall;

// Allocator
mod allocator {
    use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::{AtomicUsize, Ordering};
    use async_syscall::mmap;

    const HEAP_SIZE: usize = 16 * 1024 * 1024;
    static HEAP_START: AtomicUsize = AtomicUsize::new(0);
    static HEAP_CUR: AtomicUsize = AtomicUsize::new(0);
    static HEAP_END: AtomicUsize = AtomicUsize::new(0);

    pub struct BumpAllocator;

    unsafe impl GlobalAlloc for BumpAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if HEAP_START.load(Ordering::Relaxed) == 0 {
                let ptr = mmap(0, HEAP_SIZE, 3, 0x22);
                if !ptr.is_null() {
                    let addr = ptr as usize;
                    HEAP_START.store(addr, Ordering::Relaxed);
                    HEAP_CUR.store(addr, Ordering::Relaxed);
                    HEAP_END.store(addr + HEAP_SIZE, Ordering::Relaxed);
                }
            }

            let align = layout.align().max(1);
            let size = layout.size();
            if size == 0 { return align as *mut u8; }

            loop {
                let cur = HEAP_CUR.load(Ordering::Relaxed);
                let aligned = (cur + align - 1) & !(align - 1);
                let next = match aligned.checked_add(size) {
                    Some(n) => n,
                    None => return core::ptr::null_mut(),
                };
                if next > HEAP_END.load(Ordering::Relaxed) {
                    return core::ptr::null_mut();
                }
                if HEAP_CUR.compare_exchange(cur, next, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                    return aligned as *mut u8;
                }
            }
        }
        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
    }

    #[global_allocator]
    static ALLOC: BumpAllocator = BumpAllocator;
}

// Task scheduler
struct Node { handle: usize, next: *mut Node }
static SCHEDULE_HEAD: AtomicPtr<Node> = AtomicPtr::new(core::ptr::null_mut());
static FREELIST_HEAD: AtomicPtr<Node> = AtomicPtr::new(core::ptr::null_mut());
static FREELIST_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_HANDLE: AtomicUsize = AtomicUsize::new(1);
const FREELIST_MAX: usize = 256;

static TASK_TABLE: spin::Mutex<Option<Vec<Option<Box<dyn Future<Output = ()> + Send>>>>> =
    spin::Mutex::new(None);

struct IoEntry {
    fd: i32,
    events: i16,
    waiters: Vec<Waker>,
}
static IO_REG: spin::Mutex<Vec<IoEntry>> = spin::Mutex::new(Vec::new());
static EVENTFD: AtomicI32 = AtomicI32::new(-1);
static EVENT_PENDING: AtomicUsize = AtomicUsize::new(0);

fn ensure_eventfd() -> i32 {
    let cur = EVENTFD.load(Ordering::Relaxed);
    if cur >= 0 { return cur; }
    let fd = syscall::eventfd(0, 0);
    if fd >= 0 {
        if EVENTFD.compare_exchange(-1, fd, Ordering::Relaxed, Ordering::Relaxed).is_err() {
            let _ = syscall::close(fd);
        }
        return EVENTFD.load(Ordering::Relaxed);
    }
    -1
}

fn signal_eventfd() {
    let fd = ensure_eventfd();
    if fd < 0 { return; }
    let v: u64 = 1;
    let _ = syscall::write(fd, unsafe {
        core::slice::from_raw_parts(&v as *const u64 as *const u8, 8)
    });
}

pub fn close_eventfd() {
    let fd = EVENTFD.swap(-1, Ordering::Relaxed);
    if fd >= 0 { let _ = syscall::close(fd); }
}

pub fn register_fd_waker(fd: i32, events: i16, waker: Waker) {
    let mut reg = IO_REG.lock();
    for e in reg.iter_mut() {
        if e.fd == fd {
            e.waiters.push(waker);
            return;
        }
    }
    let mut v = Vec::new();
    v.push(waker);
    reg.push(IoEntry { fd, events, waiters: v });
}

pub fn ppoll_and_schedule() {
    let snapshot: Vec<(i32, i16)> = {
        let reg = IO_REG.lock();
        reg.iter().map(|e| (e.fd, e.events)).collect()
    };

    let evt = ensure_eventfd();
    let mut fds: Vec<syscall::PollFd> = Vec::new();
    if evt >= 0 {
        fds.push(syscall::PollFd { fd: evt, events: 0x0001, revents: 0 });
    }
    for (fd, ev) in snapshot.iter() {
        fds.push(syscall::PollFd { fd: *fd, events: *ev, revents: 0 });
    }

    if fds.is_empty() { return; }

    let ret = syscall::ppoll(fds.as_mut_ptr(), fds.len());
    if ret <= 0 { return; }

    let start = if evt >= 0 { 1 } else { 0 };
    for pf in fds.iter().skip(start) {
        if pf.revents != 0 {
            let mut to_wake: Vec<Waker> = Vec::new();
            {
                let mut reg = IO_REG.lock();
                for i in 0..reg.len() {
                    if reg[i].fd == pf.fd {
                        core::mem::swap(&mut to_wake, &mut reg[i].waiters);
                        break;
                    }
                }
            }
            for w in to_wake { w.wake(); }
        }
    }
}

fn alloc_node(handle: usize) -> *mut Node {
    loop {
        let head = FREELIST_HEAD.load(Ordering::Acquire);
        if head.is_null() {
            return Box::into_raw(Box::new(Node { handle, next: core::ptr::null_mut() }));
        }
        let next = unsafe { (*head).next };
        if FREELIST_HEAD.compare_exchange(head, next, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            FREELIST_COUNT.fetch_sub(1, Ordering::Relaxed);
            unsafe {
                (*head).handle = handle;
                (*head).next = core::ptr::null_mut();
            }
            return head;
        }
    }
}

fn free_node(ptr: *mut Node) {
    if FREELIST_COUNT.load(Ordering::Relaxed) >= FREELIST_MAX {
        unsafe { let _ = Box::from_raw(ptr); }
        return;
    }
    FREELIST_COUNT.fetch_add(1, Ordering::Relaxed);
    loop {
        let head = FREELIST_HEAD.load(Ordering::Acquire);
        unsafe { (*ptr).next = head; }
        if FREELIST_HEAD.compare_exchange(head, ptr, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            break;
        }
    }
}

pub fn wake_handle(handle: usize) {
    let node = alloc_node(handle);
    loop {
        let head = SCHEDULE_HEAD.load(Ordering::Acquire);
        unsafe { (*node).next = head; }
        if SCHEDULE_HEAD.compare_exchange(head, node, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            break;
        }
    }
    let prev = EVENT_PENDING.fetch_add(1, Ordering::Relaxed);
    if prev == 0 { signal_eventfd(); }
}

pub fn take_scheduled_task() -> Option<usize> {
    loop {
        let head = SCHEDULE_HEAD.load(Ordering::Acquire);
        if head.is_null() { return None; }
        let next = unsafe { (*head).next };
        if SCHEDULE_HEAD.compare_exchange(head, next, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            let handle = unsafe { (*head).handle };
            free_node(head);
            return Some(handle);
        }
    }
}

pub fn register_task(task: Box<dyn Future<Output = ()> + Send + 'static>) -> usize {
    let mut table = TASK_TABLE.lock();
    if table.is_none() { *table = Some(Vec::new()); }
    let t = table.as_mut().unwrap();

    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let idx = handle - 1;
    while idx >= t.len() { t.push(None); }
    t[idx] = Some(task);
    drop(table);
    wake_handle(handle);
    handle
}

pub unsafe fn poll_task(handle: usize, cx: &mut core::task::Context<'_>) -> core::task::Poll<()> {
    let mut table = TASK_TABLE.lock();
    if table.is_none() { return core::task::Poll::Ready(()); }
    let t = table.as_mut().unwrap();
    let idx = handle - 1;
    if idx >= t.len() || t[idx].is_none() { return core::task::Poll::Ready(()); }

    let mut task = t[idx].take().unwrap();
    drop(table);

    let res = unsafe {
        use core::pin::Pin;
        Pin::new_unchecked(task.as_mut()).poll(cx)
    };

    match res {
        core::task::Poll::Ready(()) => {
            drop(task);
            core::task::Poll::Ready(())
        }
        core::task::Poll::Pending => {
            let mut table = TASK_TABLE.lock();
            if let Some(t) = table.as_mut() {
                if idx < t.len() {
                    t[idx] = Some(task);
                }
            }
            core::task::Poll::Pending
        }
    }
}

pub unsafe fn create_waker_for_handle(handle: usize) -> Waker {
    use core::task::{RawWaker, RawWakerVTable, Waker};

    unsafe fn clone(data: *const ()) -> RawWaker { RawWaker::new(data, &VTABLE) }
    unsafe fn wake(data: *const ()) { wake_handle(data as usize); }
    unsafe fn wake_by_ref(data: *const ()) { wake_handle(data as usize); }
    unsafe fn drop(_: *const ()) {}

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    unsafe { Waker::from_raw(RawWaker::new(handle as *const (), &VTABLE)) }
}

// Entry point
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
        main = sym main_trampoline,
    )
}

#[unsafe(no_mangle)]
extern "C" fn main_trampoline(argc: isize, argv: *const *const u8) -> ! {
    unsafe { crate::main(argc, argv) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    syscall::exit(1);
}

// Utilities
pub fn read_ptr_array(ptr: *const *const u8, index: isize) -> *const u8 {
    unsafe { *ptr.offset(index) }
}

pub fn parse_cstring_usize(s: *const u8) -> Option<usize> {
    if s.is_null() { return None; }
    unsafe {
        let mut i: isize = 0;
        let mut acc: usize = 0;
        let mut any = false;
        while i < 64 {
            let c = *s.offset(i);
            if c == 0 || c < b'0' || c > b'9' { break; }
            any = true;
            acc = acc * 10 + ((c - b'0') as usize);
            i += 1;
        }
        if any { Some(acc) } else { None }
    }
}

unsafe extern "C" {
    fn main(argc: isize, argv: *const *const u8) -> !;
}
