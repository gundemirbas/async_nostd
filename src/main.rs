//! Minimal async demo

#![no_std]
#![no_main]

extern crate alloc;
use alloc::boxed::Box;
use async_executor::Executor;
use async_http as http;
use async_net::{AF_INET, AcceptFuture, SOCK_STREAM, SockAddrIn};
use async_syscall as sys;

#[inline(always)]
fn write(s: &[u8]) {
    let _ = sys::write(1, s);
}

// HTTP accept loop: accept connections and handle them inline
async fn server_task(sfd: i32, _exec: Executor) {
    loop {
        let cfd = AcceptFuture::new(sfd).await;
        if cfd >= 0 {
            // Handle connection directly (inline) for simplicity
            let _ = sys::fcntl(cfd as i32, sys::F_SETFL, sys::O_NONBLOCK);
            http::handle_http_connection(cfd as i32).await;
        }
    }
}

// Application entry point called by runtime after parsing argc/argv.
// The runtime passes safe Rust types (worker_count, listen_ip, listen_port).
#[unsafe(no_mangle)]
pub extern "C" fn main(worker_count: usize, listen_ip: u32, listen_port: usize) -> ! {
    write(b"Async NoStd Demo\n");

    write(b"Workers: ");
    sys::write_usize(1, worker_count);
    write(b"\n\n");

    let executor = Executor::new();
    let exec = executor.clone();

    // Create the listening socket before spawning workers so the FD is present
    // in the shared file table and visible to all worker processes.
    let sfd = sys::socket(AF_INET, SOCK_STREAM, 0);
    if sfd < 0 {
        write(b"[demo] Socket failed\n");
        sys::exit(1);
    }
    // Enable SO_REUSEADDR to allow rebinding after crash
    let optval: i32 = 1;
    let _ = sys::setsockopt(
        sfd,
        sys::SOL_SOCKET,
        sys::SO_REUSEADDR,
        (&optval as *const i32) as *const u8,
        core::mem::size_of::<i32>(),
    );
    let _ = sys::fcntl(sfd, sys::F_SETFL, sys::O_NONBLOCK);
    let addr = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: sys::htons(listen_port as u16),
        sin_addr: listen_ip,
        sin_zero: [0u8; 8],
    };
    if sys::bind(
        sfd,
        (&addr as *const SockAddrIn) as *const u8,
        core::mem::size_of::<SockAddrIn>(),
    ) < 0
    {
        write(b"[demo] Bind failed\n");
        let _ = sys::close(sfd);
        sys::exit(1);
    }
    if sys::listen(sfd, 128) < 0 {
        write(b"[demo] Listen failed\n");
        let _ = sys::close(sfd);
        sys::exit(1);
    }
    // Print the actual port chosen
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

    if worker_count == 0 {
        // Single-threaded mode
        use core::task::Context;
        write(b"[demo] Single-threaded mode\n");

        // Register task directly and schedule it
        let handle = async_runtime::register_task(Box::new(server_task(sfd, exec.clone())));
        async_runtime::wake_handle(handle);

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
        // Multi-threaded mode with worker pool
        write(b"[demo] Starting ");
        sys::write_usize(1, worker_count);
        write(b" worker threads\n");

        // Register and wake the server task
        let handle = async_runtime::register_task(Box::new(server_task(sfd, exec.clone())));
        async_runtime::wake_handle(handle);

        // Start worker threads - this never returns, workers run forever
        executor.start_workers(worker_count)
    }
}
