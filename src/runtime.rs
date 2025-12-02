//! Runtime module

use core::panic::PanicInfo;

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

pub unsafe fn create_waker() -> core::task::Waker {
    use core::task::{RawWaker, RawWakerVTable, Waker};
    
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker { 
        RawWaker::new(core::ptr::null(), &VTABLE) 
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
    
    unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
}

pub unsafe fn poll_boxed_future(
    task: &mut alloc::boxed::Box<dyn core::future::Future<Output = ()> + Send + 'static>,
    cx: &mut core::task::Context<'_>
) -> core::task::Poll<()> {
    use core::pin::Pin;
    unsafe {
        let mut pinned = Pin::new_unchecked(task.as_mut());
        pinned.as_mut().poll(cx)
    }
}
 