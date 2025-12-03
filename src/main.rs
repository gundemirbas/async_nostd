//! Minimal async demo

#![no_std]
#![no_main]

extern crate alloc;
use async_executor::Executor;
use async_http as http;
use async_net::{AF_INET, SOCK_STREAM, SockAddrIn};
use async_syscall as sys;
use core::task::Context;

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
    
    if sys::bind(
        sfd,
        (&addr as *const SockAddrIn) as *const u8,
        core::mem::size_of::<SockAddrIn>(),
    ) < 0
    {
        let _ = sys::close(sfd);
        return Err(());
    }
    
    if sys::listen(sfd, 128) < 0 {
        let _ = sys::close(sfd);
        return Err(());
    }
    
    Ok(sfd)
}

/// Helper: Print socket info (fd and bound port).
fn print_socket_info(sfd: i32) {
    write(b"[demo] sfd=");
    sys::write_usize(1, sfd as usize);
    write(b"\n");
    
    let mut sa = SockAddrIn {
        sin_family: 0,
        sin_port: 0,
        sin_addr: 0,
        sin_zero: [0u8; 8],
    };
    let mut len = core::mem::size_of::<SockAddrIn>();
    if sys::getsockname(
        sfd,
        (&mut sa as *mut SockAddrIn) as *mut u8,
        &mut len as *mut usize,
    ) >= 0
    {
        let port = sys::ntohs(sa.sin_port);
        write(b"[demo] Port: ");
        sys::write_usize(1, port as usize);
        write(b"\n");
    }
}

/// Callback invoked by acceptor thread for each accepted connection.
extern "C" fn handle_accepted_connection(cfd: i32) {
    let task = alloc::boxed::Box::new(http::handle_http_connection(cfd));
    let handle = async_runtime::register_task(task);
    async_runtime::wake_handle(handle);
}

/// Application entry point called by runtime after parsing argc/argv.
/// The runtime passes safe Rust types (worker_count, listen_ip, listen_port).
#[unsafe(no_mangle)]
pub extern "C" fn main(worker_count: usize, listen_ip: u32, listen_port: usize) -> ! {
    write(b"Async NoStd Demo\n");
    write(b"Workers: ");
    sys::write_usize(1, worker_count);
    write(b"\n\n");

    // Create listening socket using helper function
    let sfd = match create_listening_socket(listen_ip, listen_port) {
        Ok(fd) => fd,
        Err(_) => {
            write(b"[demo] Failed to create listening socket\n");
            sys::exit(1);
        }
    };
    
    print_socket_info(sfd);

    // Spawn acceptor thread using runtime helper
    let _ = async_runtime::spawn_acceptor_thread(sfd, handle_accepted_connection);

    if worker_count == 0 {
        // Single-threaded mode: run event loop in main thread
        write(b"[demo] Single-threaded mode\n");
        
        loop {
            if let Some(h) = async_runtime::take_scheduled_task() {
                let waker = async_runtime::create_waker(h);
                let mut cx = Context::from_waker(&waker);
                let _ = async_runtime::poll_task_safe(h, &mut cx);
                continue;
            }
            async_runtime::ppoll_and_schedule();
        }
    } else {
        // Multi-threaded mode: start worker pool
        write(b"[demo] Starting ");
        sys::write_usize(1, worker_count);
        write(b" worker threads\n");
        
        let executor = Executor::new();
        executor.start_workers(worker_count)
    }
}
