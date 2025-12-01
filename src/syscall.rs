//! Low-level system call wrappers
//! 
//! Bu modül unsafe sistem çağrılarını içerir ve güvenli wrapper'lar sağlar.

/// Linux syscall - write (low-level)
/// 
/// # Safety
/// fd geçerli bir file descriptor olmalı ve buf geçerli bir slice olmalı
unsafe fn syscall_write(fd: i32, buf: &[u8]) {
    core::arch::asm!(
        "syscall",
        in("rax") 1,  // syscall number for write
        in("rdi") fd,
        in("rsi") buf.as_ptr(),
        in("rdx") buf.len(),
        lateout("rax") _,
        lateout("rcx") _,
        lateout("r11") _,
    );
}

/// Linux syscall - exit (low-level)
/// 
/// # Safety
/// Bu fonksiyon process'i sonlandırır, geri dönmez
unsafe fn syscall_exit(code: i32) -> ! {
    core::arch::asm!(
        "syscall",
        in("rax") 60,  // syscall number for exit
        in("rdi") code,
        options(noreturn)
    );
}

/// stdout'a güvenli yazma
pub fn write(buf: &[u8]) {
    // SAFETY: syscall write güvenli bir işlem, buf geçerli slice
    unsafe { syscall_write(1, buf) };
}

/// Program çıkışı
pub fn exit(code: i32) -> ! {
    // SAFETY: exit syscall güvenli bir işlem
    unsafe { syscall_exit(code) };
}

/// C string yazdır (null-terminated pointer)
/// 
/// # Safety
/// Pointer null olabilir (kontrol ediliyor) veya geçerli null-terminated string
pub fn print_cstring(s: *const u8) {
    if s.is_null() {
        write(b"(null)");
        return;
    }
    
    // SAFETY: pointer null değil, 4096 byte sınırı ile güvenli kontrol
    let len = unsafe {
        let mut len = 0isize;
        // Max 4096 karakter kontrol et
        while len < 4096 && *s.offset(len) != 0 {
            len += 1;
        }
        len
    };
    
    if len > 0 {
        // SAFETY: len kadar bayt geçerli ve null-terminated olduğunu kontrol ettik
        let slice = unsafe { core::slice::from_raw_parts(s, len as usize) };
        write(slice);
    }
}




