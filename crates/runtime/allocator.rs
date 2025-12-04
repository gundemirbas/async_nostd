//! Bump allocator - simple, fast, no deallocation

use crate::config::HEAP_SIZE;
use crate::syscall;
use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
static HEAP_START: AtomicUsize = AtomicUsize::new(0);
static HEAP_CUR: AtomicUsize = AtomicUsize::new(0);
static HEAP_END: AtomicUsize = AtomicUsize::new(0);

pub struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if HEAP_START.load(Ordering::Relaxed) == 0 {
            let ptr = syscall::mmap(0, HEAP_SIZE, 3, 0x22);
            if !ptr.is_null() {
                let addr = ptr as usize;
                HEAP_START.store(addr, Ordering::Relaxed);
                HEAP_CUR.store(addr, Ordering::Relaxed);
                HEAP_END.store(addr + HEAP_SIZE, Ordering::Relaxed);
            }
        }

        let align = layout.align().max(1);
        let size = layout.size();
        if size == 0 {
            return align as *mut u8;
        }

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
            if HEAP_CUR
                .compare_exchange(cur, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return aligned as *mut u8;
            }
        }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: BumpAllocator = BumpAllocator;
