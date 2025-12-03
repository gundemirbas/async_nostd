//! Minimal syscall wrappers - only what's actually used

#![no_std]

// Core syscalls
unsafe fn syscall1(n: u64, a1: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") n,
            in("rdi") a1,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

unsafe fn syscall2(n: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

unsafe fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

unsafe fn syscall4(n: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

unsafe fn syscall5(n: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

unsafe fn syscall6(n: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            in("r9") a6,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

// Public API - only used syscalls
pub fn write(fd: i32, buf: &[u8]) -> isize {
    unsafe { syscall3(1, fd as u64, buf.as_ptr() as u64, buf.len() as u64) as isize }
}

pub fn read(fd: i32, buf: &mut [u8]) -> isize {
    unsafe { syscall3(0, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as isize }
}

pub fn exit(code: i32) -> ! {
    unsafe {
        syscall1(60, code as u64);
    }
    loop {
        core::hint::spin_loop();
    }
}

pub fn mmap(addr: usize, len: usize, prot: i32, flags: i32) -> *mut u8 {
    let ret = unsafe { syscall6(9, addr as u64, len as u64, prot as u64, flags as u64, !0, 0) };
    if ret < 0 {
        core::ptr::null_mut()
    } else {
        ret as *mut u8
    }
}

pub fn eventfd(initval: u32, flags: i32) -> i32 {
    let ret = unsafe { syscall2(290, initval as u64, flags as u64) };
    if ret < 0 { -1 } else { ret as i32 }
}

pub fn close(fd: i32) -> i32 {
    let ret = unsafe { syscall1(3, fd as u64) };
    if ret < 0 { -1 } else { ret as i32 }
}

// Utility: Format usize to decimal ASCII
#[inline]
pub fn format_usize(n: usize) -> ([u8; 20], usize) {
    let mut buf = [0u8; 20];
    if n == 0 {
        buf[0] = b'0';
        return (buf, 1);
    }
    let mut num = n;
    let mut i = 0;
    while num > 0 {
        buf[i] = b'0' + (num % 10) as u8;
        num /= 10;
        i += 1;
    }
    // Reverse
    let mut j = 0;
    while j < i / 2 {
        buf.swap(j, i - 1 - j);
        j += 1;
    }
    (buf, i)
}

#[inline]
pub fn write_usize(fd: i32, n: usize) {
    let (buf, len) = format_usize(n);
    let _ = write(fd, &buf[..len]);
}

/// Write a signed 64-bit integer as decimal ASCII to `fd`.
pub fn write_isize(fd: i32, mut n: i64) {
    if n < 0 {
        let _ = write(fd, b"-");
        n = -n;
    }
    let (buf, len) = format_usize(n as usize);
    let _ = write(fd, &buf[..len]);
}

#[repr(C)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

pub fn ppoll(fds: *mut PollFd, nfds: usize) -> isize {
    // ppoll with infinite timeout (NULL timespec pointer)
    unsafe { syscall5(271, fds as u64, nfds as u64, 0, 0, 0) as isize }
}

pub fn ppoll_timeout(fds: *mut PollFd, nfds: usize, timeout_ms: i64) -> isize {
    // ppoll with timeout: struct timespec { tv_sec, tv_nsec }
    let ts = [timeout_ms / 1000, (timeout_ms % 1000) * 1_000_000];
    unsafe { syscall5(271, fds as u64, nfds as u64, ts.as_ptr() as u64, 0, 0) as isize }
}

pub fn fcntl(fd: i32, cmd: i32, arg: usize) -> isize {
    unsafe { syscall3(72, fd as u64, cmd as u64, arg as u64) as isize }
}

pub const F_SETFL: i32 = 4;
pub const O_NONBLOCK: usize = 0x800;

// Socket syscalls
pub fn socket(domain: i32, type_: i32, protocol: i32) -> i32 {
    let ret = unsafe { syscall3(41, domain as u64, type_ as u64, protocol as u64) };
    if ret < 0 { -1 } else { ret as i32 }
}

pub fn setsockopt(fd: i32, level: i32, optname: i32, optval: *const u8, optlen: usize) -> i32 {
    let ret = unsafe {
        syscall5(
            54,
            fd as u64,
            level as u64,
            optname as u64,
            optval as u64,
            optlen as u64,
        )
    };
    ret as i32
}

pub const SOL_SOCKET: i32 = 1;
pub const SO_REUSEADDR: i32 = 2;
pub const SO_REUSEPORT: i32 = 15;

pub fn bind(fd: i32, addr: *const u8, addrlen: usize) -> isize {
    unsafe { syscall3(49, fd as u64, addr as u64, addrlen as u64) as isize }
}

pub fn listen(fd: i32, backlog: i32) -> isize {
    unsafe { syscall2(50, fd as u64, backlog as u64) as isize }
}

pub fn accept4(fd: i32, addr: *mut u8, addrlen: *mut usize, flags: i32) -> isize {
    unsafe { syscall4(288, fd as u64, addr as u64, addrlen as u64, flags as u64) as isize }
}

pub fn connect(fd: i32, addr: *const u8, addrlen: usize) -> isize {
    unsafe { syscall3(42, fd as u64, addr as u64, addrlen as u64) as isize }
}

pub fn sendto(
    fd: i32,
    buf: *const u8,
    len: usize,
    flags: i32,
    dest: *const u8,
    addrlen: usize,
) -> isize {
    unsafe {
        syscall6(
            44,
            fd as u64,
            buf as u64,
            len as u64,
            flags as u64,
            dest as u64,
            addrlen as u64,
        ) as isize
    }
}

pub fn recvfrom(
    fd: i32,
    buf: *mut u8,
    len: usize,
    flags: i32,
    src: *mut u8,
    addrlen: *mut usize,
) -> isize {
    unsafe {
        syscall6(
            45,
            fd as u64,
            buf as u64,
            len as u64,
            flags as u64,
            src as u64,
            addrlen as u64,
        ) as isize
    }
}

pub fn getsockname(fd: i32, addr: *mut u8, addrlen: *mut usize) -> isize {
    unsafe { syscall3(51, fd as u64, addr as u64, addrlen as u64) as isize }
}

pub fn getpeername(fd: i32, addr: *mut u8, addrlen: *mut usize) -> isize {
    unsafe { syscall3(52, fd as u64, addr as u64, addrlen as u64) as isize }
}

// Byte-order helpers
pub fn htons(x: u16) -> u16 {
    x.to_be()
}
pub fn ntohs(x: u16) -> u16 {
    u16::from_be(x)
}

// Clone for thread spawning
pub fn clone(flags: u64, stack: *mut u8, ptid: *mut i32, ctid: *mut i32, tls: u64) -> isize {
    unsafe { syscall5(56, flags, stack as u64, ptid as u64, ctid as u64, tls) as isize }
}

// waitpid wrapper
pub fn waitpid(pid: i32, status: *mut i32, options: i32) -> isize {
    unsafe { syscall3(61, pid as u64, status as u64, options as u64) as isize }
}

// rt_sigaction wrapper. On x86_64 the syscall signature is:
// long rt_sigaction(int signum, const struct sigaction *act,
//                   struct sigaction *oldact, size_t sigsetsize);
pub fn rt_sigaction(signum: i32, act: *const u8, oldact: *mut u8, sigsetsize: usize) -> isize {
    unsafe {
        syscall4(
            13,
            signum as u64,
            act as u64,
            oldact as u64,
            sigsetsize as u64,
        ) as isize
    }
}

pub fn nanosleep(seconds: u64) -> isize {
    // Legacy wrapper (seconds). Keep for compatibility but prefer nanosleep_ns.
    let ts = [seconds, 0u64];
    unsafe { syscall2(35, &ts as *const u64 as u64, 0) as isize }
}

/// Sleep for the given duration in nanoseconds using `nanosleep` syscall.
pub fn nanosleep_ns(nanos: u64) -> isize {
    let sec = nanos / 1_000_000_000;
    let nsec = (nanos % 1_000_000_000) as u64;
    let ts = [sec, nsec];
    unsafe { syscall2(35, &ts as *const u64 as u64, 0) as isize }
}

// Thread-local storage structure (minimal)
#[repr(C)]
struct TlsBlock {
    self_ptr: *mut TlsBlock, // Point to itself (required by x86_64 TLS ABI)
    _padding: [u64; 15],     // Reserve space for future use
}

pub fn spawn_thread(f: extern "C" fn(*mut u8), arg: *mut u8, stack_size: usize) -> Result<(), i32> {
    const PROT_RW: i32 = 0x3;
    const MAP_PRIVATE_ANON: i32 = 0x22;

    // Allocate stack
    let stack = mmap(0, stack_size, PROT_RW, MAP_PRIVATE_ANON);
    if stack.is_null() {
        return Err(-1);
    }

    // Allocate TLS block
    let tls = mmap(0, 4096, PROT_RW, MAP_PRIVATE_ANON);
    if tls.is_null() {
        return Err(-1);
    }

    // Initialize TLS block - self-pointer required by x86_64 TLS ABI
    unsafe {
        let tls_block = tls as *mut TlsBlock;
        (*tls_block).self_ptr = tls_block;
    }

    let stack_top = unsafe { stack.add(stack_size) };

    // CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_SETTLS
    // 0x10000 = CLONE_THREAD: real threads sharing PID (needed for eventfd wakeups)
    // 0x80000 = CLONE_SETTLS: set TLS pointer to avoid segfaults
    const FLAGS: u64 = 0x100 | 0x200 | 0x400 | 0x800 | 0x10000 | 0x80000;

    let ret = clone(
        FLAGS,
        stack_top,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        tls as u64, // TLS pointer
    );
    if ret == 0 {
        // Child thread
        f(arg);
        exit(0);
    }
    if ret < 0 { Err(ret as i32) } else { Ok(()) }
}
