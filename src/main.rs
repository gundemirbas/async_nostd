#![no_std]
#![no_main]

mod syscall;
mod runtime;
mod executor;
mod net_futures;

use syscall::{write, exit, print_cstring};
use crate::net_futures::{SockAddrIn, htons, AF_INET, SOCK_STREAM};
use runtime::read_ptr_array;
use executor::Executor;
extern crate alloc;
use alloc::boxed::Box;

fn print_number(n: isize) {
    let mut buf = [0u8; 20];
    let mut num = n;
    let mut i = 0;
    
    if num == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        if num < 0 {
            write(b"-");
            num = -num;
        }
        
        let mut temp_i = 0;
        while num > 0 {
            buf[temp_i] = b'0' + (num % 10) as u8;
            num /= 10;
            temp_i += 1;
        }
        
        while temp_i > 0 {
            temp_i -= 1;
            buf[i] = buf[temp_i];
            i += 1;
        }
    }
    
    write(&buf[..i]);
}

// Removed example_task; the TCP demo is now executed as an async task using the runtime.

fn main(argc: isize, argv: *const *const u8) -> ! {
    write(b"Program started\n");
    write(b"argc: ");
    print_number(argc);
    write(b"\n");
    
    if argc > 0 && !argv.is_null() {
        write(b"Arguments:\n");
        for i in 0..argc {
            let arg_ptr = read_ptr_array(argv, i);
            if !arg_ptr.is_null() {
                write(b"  [");
                print_number(i);
                write(b"]: ");
                print_cstring(arg_ptr);
                write(b"\n");
            }
        }
    }
    
    write(b"\nRunning async tasks...\n");
    let executor = Executor::new();
    let exec_for_demo = executor.clone();

    // Start a TCP server/client demo in-process using sockets.
    // We'll bind to loopback and a dynamic port, accept in a spawned thread,
    // then connect from the main thread and exchange a small message.

    

    // Enqueue the TCP demo as a runtime task so it runs on worker threads.
    let tcp_demo = async move {
        write(b"[demo] tcp_demo start\n");

        // helper to print syscall result
            fn pr(code: isize) {
            if code < 0 {
                write(b"[err] rc=");
                // print negative as isize
                print_number(code as isize);
                write(b"\n");
            } else {
                write(b"[ok] rc=");
                print_number(code as isize);
                write(b"\n");
            }
        }

        // `SockAddrIn`, `htons`, `AF_INET`, and `SOCK_STREAM` are provided by `net_futures`.

        // create server socket
        let sfd = syscall::socket(AF_INET, SOCK_STREAM, 0);
        pr(sfd as isize);

        // set listener non-blocking so AcceptFuture can use EAGAIN
        if sfd >= 0 {
            let _ = syscall::fcntl(sfd as i32, syscall::F_SETFL, syscall::O_NONBLOCK);
        }

        if sfd >= 0 {
            let addr = SockAddrIn { sin_family: AF_INET as u16, sin_port: htons(0), sin_addr: 0x0100007fu32, sin_zero: [0u8;8] };
            let r = syscall::bind(sfd, (&addr as *const SockAddrIn) as *const u8, core::mem::size_of::<SockAddrIn>());
            pr(r);
            if r >= 0 {
                let r2 = syscall::listen(sfd, 4);
                pr(r2);
                // getsockname
                let mut sa = SockAddrIn { sin_family: 0, sin_port: 0, sin_addr: 0, sin_zero: [0u8;8] };
                let mut len: usize = core::mem::size_of::<SockAddrIn>();
                let g = syscall::getsockname(sfd, (&mut sa as *mut SockAddrIn) as *mut u8, &mut len as *mut usize);
                pr(g);
                let bound_port = if g >= 0 { u16::from_be(sa.sin_port) } else { 0 };
                write(b"[demo] bound_port="); print_number(bound_port as isize); write(b"\n");

                

                // spawn server acceptor task (handles one client then exits)
                    use crate::net_futures::{AcceptFuture, ConnectFuture, RecvFuture, SendFuture};

                        // create server acceptor async task
                        let server_acceptor = async move {
                            let mut handled = 0usize;
                            loop {
                                let af = AcceptFuture::new(sfd);
                                let cfd = af.await;
                                if cfd >= 0 {
                                    crate::syscall::write_fd(1, b"[server] accepted\n");
                                    // Blocking recv on accepted socket (simple handler)
                                    let mut recvb = [0u8; 128];
                                    let r = crate::syscall::recvfrom(cfd as i32, recvb.as_mut_ptr(), recvb.len(), 0, core::ptr::null_mut(), core::ptr::null_mut());
                                    if r > 0 {
                                        let _ = crate::syscall::write_fd(1, b"[server] handled\n");
                                        let _ = crate::syscall::sendto(cfd as i32, b"pong\n".as_ptr(), 5, 0, core::ptr::null(), 0);
                                    }
                                    let _ = crate::syscall::close(cfd as i32);
                                    handled += 1;
                                    if handled >= 1 { break; }
                                }
                            }
                        };
                let _ = exec_for_demo.clone().enqueue_task(Box::new(server_acceptor));

                // set runtime extra fd for fallback (kept for compatibility)
                runtime::set_extra_wait_fd(sfd as i32);

                // client: non-blocking connect + async send/recv using runtime's IO registry
                let cfd = syscall::socket(AF_INET, SOCK_STREAM, 0);
                pr(cfd as isize);
                if cfd >= 0 {
                    // set non-blocking before connect
                    let _ = syscall::fcntl(cfd as i32, syscall::F_SETFL, syscall::O_NONBLOCK);
                    let addr = SockAddrIn { sin_family: AF_INET as u16, sin_port: htons(bound_port), sin_addr: 0x0100007f_u32, sin_zero: [0u8;8] };

                    let con = ConnectFuture::new(cfd as i32, (&addr as *const SockAddrIn) as *const u8, core::mem::size_of::<SockAddrIn>()).await;
                    pr(con);
                    if con >= 0 {
                        let s = SendFuture::new(cfd as i32, b"hello\n").await;
                        pr(s);
                        if s >= 0 { let _ = syscall::write_fd(1, b"hello\n"); }

                        let buf = RecvFuture::new(cfd as i32, 64).await;
                        pr(buf.len() as isize);
                        if buf.len() > 0 {
                            let _ = syscall::write_fd(1, &buf[..]);
                        }
                    }
                    let _ = syscall::close(cfd as i32);
                }

                // close server socket
                let _ = syscall::close(sfd as i32);
            }
        }

        write(b"[demo] tcp_demo done\n");
    };

    let _ = executor.enqueue_task(Box::new(tcp_demo));

    let mut worker_count: usize = 16;
    if argc > 1 {
        let s = read_ptr_array(argv, 1);
        if let Some(n) = runtime::parse_cstring_usize(s) {
            if n > 0 { worker_count = n; }
        }
    }

    let _ = executor.start_workers(worker_count);

    // TCP demo ran above (client connected and exchanged messages with spawned server thread).

    executor.wait_all();

    write(b"All tasks completed\n");
    // Close runtime resources (eventfd) before exiting.
    crate::runtime::close_eventfd();
    exit(0);
}
