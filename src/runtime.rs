//! Runtime module - Tüm unsafe operasyonların izole edildiği yer
//! 
//! Bu modül program başlangıcı, future execution ve low-level işlemler için
//! güvenli wrapper'lar sağlar.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, RawWaker, RawWakerVTable, Waker};
use core::panic::PanicInfo;

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

/// Panic handler - zorunlu no_std için
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

/// Dummy waker - no_std ortamında basit executor için
fn dummy_raw_waker() -> RawWaker {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        dummy_raw_waker()
    }
    
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
    RawWaker::new(core::ptr::null(), &VTABLE)
}

/// Future executor wrapper - unsafe operasyonları gizler
pub struct Executor;

impl Executor {
    /// Yeni bir executor oluştur
    pub fn new() -> Self {
        Self
    }
    
    /// Bir future'ı tek seferlik poll et
    /// 
    /// Bu basit bir executor implementasyonu, gerçek dünyada
    /// daha karmaşık bir executor kullanılmalı
    pub fn block_on<F>(&self, future: &mut F) -> F::Output
    where
        F: Future,
    {
        // SAFETY: dummy_raw_waker minimal geçerli bir RawWaker oluşturur
        let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
        let mut context = Context::from_waker(&waker);
        
        // SAFETY: future referansı geçerli ve poll süresince yaşayacak
        let mut pinned = unsafe { Pin::new_unchecked(future) };
        
        match pinned.as_mut().poll(&mut context) {
            core::task::Poll::Ready(val) => val,
            core::task::Poll::Pending => {
                // Bu basit executor pending durumunu handle etmiyor
                // Gerçek bir uygulamada bu tekrar poll edilmeli
                panic!("Future is pending");
            }
        }
    }
}
