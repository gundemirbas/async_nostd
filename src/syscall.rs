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

// Generic write to fd
pub fn write_fd(fd: i32, buf: &[u8]) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1usize,
            in("rdi") fd as usize,
            in("rsi") buf.as_ptr(),
            in("rdx") buf.len(),
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

// Generic read from fd
#[allow(dead_code)]
pub fn read_fd(fd: i32, buf: &mut [u8]) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 0usize,
            in("rdi") fd as usize,
            in("rsi") buf.as_mut_ptr(),
            in("rdx") buf.len(),
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

// eventfd2 syscall: returns fd or -1 on error
pub fn eventfd(initval: u32, flags: i32) -> i32 {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 290usize,
            in("rdi") initval as usize,
            in("rsi") flags as usize,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    if ret < 0 { -1 } else { ret as i32 }
}

// close syscall
pub fn close(fd: i32) -> i32 {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 3usize,
            in("rdi") fd as usize,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    if ret < 0 { -1 } else { ret as i32 }
}

// Minimal `pollfd` struct for use with `poll`/`ppoll` wrappers.
#[repr(C)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

// poll syscall wrapper: takes pointer to `PollFd`, number of fds, and timeout in milliseconds.
// `poll` wrapper removed (unused); `ppoll` is used by the runtime.

// ppoll syscall wrapper. Signature matches Linux ppoll: (struct pollfd *fds, nfds_t nfds,
// const struct timespec *tmo_p, const sigset_t *sigmask, size_t sigsetsize)
pub fn ppoll(fds: *mut PollFd, nfds: usize, tmo_p: *const u8, sigmask: *const u8, sigsetsize: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 271usize,
            in("rdi") fds as usize,
            in("rsi") nfds,
            in("rdx") tmo_p as usize,
            in("r10") sigmask as usize,
            in("r8") sigsetsize,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

// fcntl syscall wrapper (minimal): int fcntl(int fd, int cmd, long arg);
pub fn fcntl(fd: i32, cmd: i32, arg: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 72usize,
            in("rdi") fd as usize,
            in("rsi") cmd as usize,
            in("rdx") arg,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

pub const F_SETFL: i32 = 4;
pub const O_NONBLOCK: usize = 0x800;

// socket syscall: domain, type, protocol -> fd or -1
pub fn socket(domain: i32, type_: i32, protocol: i32) -> i32 {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 41usize,
            in("rdi") domain as usize,
            in("rsi") type_ as usize,
            in("rdx") protocol as usize,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    if ret < 0 { -1 } else { ret as i32 }
}

// socketpair syscall: domain, type, protocol, int sv[2]
// (removed) socketpair wrapper — unused in current demo

// bind syscall: int bind(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
pub fn bind(fd: i32, addr: *const u8, addrlen: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 49usize,
            in("rdi") fd as usize,
            in("rsi") addr as usize,
            in("rdx") addrlen,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

// listen syscall: int listen(int sockfd, int backlog);
pub fn listen(fd: i32, backlog: i32) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 50usize,
            in("rdi") fd as usize,
            in("rsi") backlog as usize,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

// accept4 syscall: int accept4(int sockfd, struct sockaddr *addr, socklen_t *addrlen, int flags);
pub fn accept4(fd: i32, addr: *mut u8, addrlen: *mut usize, flags: i32) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 288usize,
            in("rdi") fd as usize,
            in("rsi") addr as usize,
            in("rdx") addrlen as usize,
            in("r10") flags as usize,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

// connect syscall: int connect(int sockfd, const struct sockaddr *addr, socklen_t addrlen);
pub fn connect(fd: i32, addr: *const u8, addrlen: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 42usize,
            in("rdi") fd as usize,
            in("rsi") addr as usize,
            in("rdx") addrlen,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

// sendto syscall: ssize_t sendto(int sockfd, const void *buf, size_t len, int flags,
//                                 const struct sockaddr *dest_addr, socklen_t addrlen);
pub fn sendto(fd: i32, buf: *const u8, len: usize, flags: i32, dest: *const u8, addrlen: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 44usize,
            in("rdi") fd as usize,
            in("rsi") buf as usize,
            in("rdx") len,
            in("r10") flags as usize,
            in("r8") dest as usize,
            in("r9") addrlen,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

// recvfrom syscall: ssize_t recvfrom(int sockfd, void *buf, size_t len, int flags,
//                                     struct sockaddr *src_addr, socklen_t *addrlen);
pub fn recvfrom(fd: i32, buf: *mut u8, len: usize, flags: i32, src: *mut u8, addrlen: *mut usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 45usize,
            in("rdi") fd as usize,
            in("rsi") buf as usize,
            in("rdx") len,
            in("r10") flags as usize,
            in("r8") src as usize,
            in("r9") addrlen as usize,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

// getsockname syscall: int getsockname(int sockfd, struct sockaddr *addr, socklen_t *addrlen);
pub fn getsockname(fd: i32, addr: *mut u8, addrlen: *mut usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 51usize,
            in("rdi") fd as usize,
            in("rsi") addr as usize,
            in("rdx") addrlen as usize,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
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




