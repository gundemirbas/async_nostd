//! Runtime module - Tüm unsafe operasyonların izole edildiği yer
//! 
//! Bu modül program başlangıcı, future execution ve low-level işlemler için
//! güvenli wrapper'lar sağlar.

// no task waker utilities needed in runtime
use core::panic::PanicInfo;

// Simple mmap-backed global allocator for freestanding builds
mod mmap_alloc {
    use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::{AtomicUsize, Ordering};
    use core::ptr::null_mut;

    const HEAP_SIZE: usize = 16 * 1024 * 1024; // 16 MiB

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
            init_heap();
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

        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
            // no-op bump allocator
        }
    }

    #[global_allocator]
    static ALLOC: MmapAllocator = MmapAllocator;
}

/// Program entry point - naked function
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        // Stack'ten argc ve argv'yi al
        "xor rbp, rbp",
        "pop rdi",               // argc
        "mov rsi, rsp",          // argv (işaretçiler dizisinin başlangıcı)
        "and rsp, ~0xf",         // Stack'i 16-byte hizala
        "push 0",                // Dummy return address
        "jmp main",
    )
}

/// Main function trampoline - C ABI ile çağrılır
#[no_mangle]
pub extern "C" fn main(argc: isize, argv: *const *const u8) -> ! {
    crate::main(argc, argv)
}

/// Panic handler - only for freestanding builds
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use crate::syscall::exit;
    exit(1);
}

/// Pointer dizisinden güvenli bir şekilde eleman oku
/// 
/// # Safety
/// ptr geçerli bir pointer olmalı ve index sınırlar içinde olmalı
pub fn read_ptr_array(ptr: *const *const u8, index: isize) -> *const u8 {
    // SAFETY: Caller garantiliyor ki ptr geçerli ve index sınırlar içinde
    unsafe { *ptr.offset(index) }
}

/// Parse a null-terminated C string pointer as a decimal usize.
/// Returns `None` if the pointer is null or the string is empty/invalid.
pub fn parse_cstring_usize(s: *const u8) -> Option<usize> {
    if s.is_null() {
        return None;
    }
    // SAFETY: caller provides valid pointer; we bound to a reasonable length
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

// dummy raw waker removed — runtime does not expose a block_on waker.
// Executor moved to `executor_clean.rs` to keep runtime free of executor logic.
 