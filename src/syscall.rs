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

// Safe wrapper around mmap syscall (anonymous, private) that returns pointer or null on error.
pub fn mmap_alloc(size: usize) -> *mut u8 {
    // Constants for mmap
    const PROT_READ: usize = 0x1;
    const PROT_WRITE: usize = 0x2;
    const MAP_PRIVATE: usize = 0x02;
    const MAP_ANONYMOUS: usize = 0x20;

    let prot = PROT_READ | PROT_WRITE;
    let flags = MAP_PRIVATE | MAP_ANONYMOUS;

    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 9usize, // __NR_mmap on x86_64
            in("rdi") 0usize, // addr = NULL
            in("rsi") size,
            in("rdx") prot,
            in("r10") flags,
            in("r8") !0usize, // fd = -1
            in("r9") 0usize,   // offset
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }

    if ret < 0 {
        core::ptr::null_mut()
    } else {
        ret as *mut u8
    }
}

/// Spawn a thread-like child using `clone` and an mmap-allocated stack.
/// Safe wrapper that internally uses unsafe syscalls. The callback must be
/// `extern "C" fn(*mut u8)`; in the child the function is run and
/// `exit(0)` is invoked afterwards.
pub fn spawn_thread(f: extern "C" fn(*mut u8), arg: *mut u8, stack_size: usize) -> Result<(), ()> {
    use core::ptr;

    // Allocate stack via mmap
    let stack = mmap_alloc(stack_size);
    if stack.is_null() {
        return Err(());
    }
    let stack_top = unsafe { stack.add(stack_size) };

    // Clone flags for thread-like behavior
    const CLONE_VM: u64 = 0x00000100;
    const CLONE_FS: u64 = 0x00000200;
    const CLONE_FILES: u64 = 0x00000400;
    const CLONE_SIGHAND: u64 = 0x00000800;
    // Do not use CLONE_THREAD here — it's tricky without full pthread setup.
    // Use SIGCHLD (17) as the child termination signal so parent can reap the child.
    const SIGCHLD: u64 = 17;
    const FLAGS: u64 = CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | SIGCHLD;

    // Safety: perform raw clone syscall
    let ret = unsafe { clone_thread(FLAGS, stack_top, ptr::null_mut(), ptr::null_mut(), 0) };
    if ret == 0 {
        // child
        f(arg);
        // if returns, exit
        unsafe { syscall_exit(0) }
    }

    if ret < 0 {
        Err(())
    } else {
        Ok(())
    }
}

/// Raw clone syscall wrapper for x86_64 Linux.
///
/// Safety: caller must provide a valid `stack_top` pointer (stack grows down),
/// and appropriate `flags`. This performs a raw syscall and returns the
/// syscall return value (child returns 0, parent returns child's tid/pid).
///
/// Signature follows the raw Linux syscall convention:
/// rdi = flags, rsi = newsp (stack pointer), rdx = parent_tidptr,
/// r10 = child_tidptr, r8 = newtls
pub unsafe fn clone_thread(flags: u64, stack_top: *mut u8, parent_tid: *mut i32, child_tid: *mut i32, newtls: u64) -> isize {
    let ret: isize;
    core::arch::asm!(
        "syscall",
        in("rax") 56u64, // __NR_clone on x86_64
        in("rdi") flags,
        in("rsi") stack_top,
        in("rdx") parent_tid,
        in("r10") child_tid,
        in("r8") newtls,
        lateout("rax") ret,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}




