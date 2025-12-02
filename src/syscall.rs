//! Low-level system call wrappers

unsafe fn syscall_write(fd: i32, buf: &[u8]) {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1,
            in("rdi") fd,
            in("rsi") buf.as_ptr(),
            in("rdx") buf.len(),
            lateout("rax") _,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
}

unsafe fn syscall_exit(code: i32) -> ! {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 60,
            in("rdi") code,
            options(noreturn)
        );
    }
}

pub fn write(buf: &[u8]) {
    unsafe { syscall_write(1, buf) };
}

pub fn exit(code: i32) -> ! {
    unsafe { syscall_exit(code) };
}

pub fn print_cstring(s: *const u8) {
    if s.is_null() {
        write(b"(null)");
        return;
    }
    
    let len = unsafe {
        let mut len = 0isize;
        while len < 4096 && *s.offset(len) != 0 {
            len += 1;
        }
        len
    };
    
    if len > 0 {
        let slice = unsafe { core::slice::from_raw_parts(s, len as usize) };
        write(slice);
    }
}

pub fn mmap_alloc(size: usize) -> *mut u8 {
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
            in("rax") 9usize,
            in("rdi") 0usize,
            in("rsi") size,
            in("rdx") prot,
            in("r10") flags,
            in("r8") !0usize,
            in("r9") 0usize,
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

pub fn spawn_thread(f: extern "C" fn(*mut u8), arg: *mut u8, stack_size: usize) -> Result<(), ()> {
    use core::ptr;

    let stack = mmap_alloc(stack_size);
    if stack.is_null() {
        return Err(());
    }
    let stack_top = unsafe { stack.add(stack_size) };

    const CLONE_VM: u64 = 0x00000100;
    const CLONE_FS: u64 = 0x00000200;
    const CLONE_FILES: u64 = 0x00000400;
    const CLONE_SIGHAND: u64 = 0x00000800;
    const SIGCHLD: u64 = 17;
    const FLAGS: u64 = CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | SIGCHLD;

    let ret = unsafe { clone_thread(FLAGS, stack_top, ptr::null_mut(), ptr::null_mut(), 0) };
    if ret == 0 {
        f(arg);
        unsafe { syscall_exit(0) }
    }

    if ret < 0 { Err(()) } else { Ok(()) }
}

pub unsafe fn clone_thread(flags: u64, stack_top: *mut u8, parent_tid: *mut i32, child_tid: *mut i32, newtls: u64) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 56u64,
            in("rdi") flags,
            in("rsi") stack_top,
            in("rdx") parent_tid,
            in("r10") child_tid,
            in("r8") newtls,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}




