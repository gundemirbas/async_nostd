//! Minimal async demo

#![no_std]
#![no_main]

extern crate alloc;
use async_executor::Executor;
use async_http as http;
use async_net::{AF_INET, SOCK_STREAM, SockAddrIn};
use async_syscall as sys;

#[inline(always)]
fn write(s: &[u8]) {
    let _ = sys::write(1, s);
}

/// Helper: Create and configure a listening socket.
fn create_listening_socket(ip: u32, port: usize) -> Result<i32, ()> {
    let sfd = sys::socket(AF_INET, SOCK_STREAM, 0);
    if sfd < 0 {
        return Err(());
    }
    
    // Enable SO_REUSEADDR
    let optval: i32 = 1;
    let _ = sys::setsockopt(
        sfd,
        sys::SOL_SOCKET,
        sys::SO_REUSEADDR,
        (&optval as *const i32) as *const u8,
        core::mem::size_of::<i32>(),
    );
    
    let addr = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: sys::htons(port as u16),
        sin_addr: ip,
        sin_zero: [0u8; 8],
    };
    
    let bind_result = sys::bind(
        sfd,
        (&addr as *const SockAddrIn) as *const u8,
        core::mem::size_of::<SockAddrIn>(),
    );
    if bind_result < 0 {
        let _ = sys::close(sfd);
        return Err(());
    }
    
    let listen_result = sys::listen(sfd, async_runtime::LISTEN_BACKLOG);
    if listen_result < 0 {
        let _ = sys::close(sfd);
        return Err(());
    }
    
    Ok(sfd)
}

/// Callback invoked by acceptor thread for each accepted connection.
extern "C" fn handle_accepted_connection(cfd: i32) {
    async_runtime::log_write(b"[ACCEPT] fd=");
    sys::write_usize(async_runtime::LOG_FD.load(core::sync::atomic::Ordering::Relaxed), cfd as usize);
    async_runtime::log_write(b"\n");
    
    let task = alloc::boxed::Box::new(http::handle_http_connection(cfd));
    let handle = async_runtime::register_task(task);
    async_runtime::wake_handle(handle);
}

/// Application entry point called by runtime after parsing argc/argv.
/// The runtime passes safe Rust types (worker_count, listen_ip, listen_port).
#[unsafe(no_mangle)]
pub extern "C" fn main(worker_count: usize, listen_ip: u32, listen_port: usize) -> ! {
    // Open log file
    let log_fd = sys::open(
        async_runtime::LOG_FILE_PATH.as_ptr(),
        sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC,
        0o644,
    );
    if log_fd >= 0 {
        async_runtime::LOG_FD.store(log_fd, core::sync::atomic::Ordering::Relaxed);
    }
    
    // Console output - minimal
    write(b"Async NoStd Server\n");
    write(b"Workers: ");
    sys::write_usize(1, worker_count);
    write(b" | Port: ");
    sys::write_usize(1, listen_port);
    write(b" | Log: /tmp/async-nostd.log\n");

    // Create listening socket using helper function
    let sfd = match create_listening_socket(listen_ip, listen_port) {
        Ok(fd) => fd,
        Err(_) => sys::exit(1),
    };

    // Spawn acceptor thread using runtime helper
    let _ = async_runtime::spawn_acceptor_thread(sfd, handle_accepted_connection);
    
    let executor = Executor::new();
    executor.start_workers(worker_count)
}
