//! Minimal async demo

#![no_std]
#![no_main]

extern crate alloc;
use alloc::boxed::Box;
use async_executor::Executor;
use async_net::{AcceptFuture, ConnectFuture, RecvFuture, SendFuture, SockAddrIn, htons, AF_INET, SOCK_STREAM};
use async_syscall as sys;

fn write(s: &[u8]) { let _ = sys::write(1, s); }

fn format_num(n: isize) -> [u8; 20] {
    let mut buf = [0u8; 20];
    if n == 0 {
        buf[0] = b'0';
        return buf;
    }
    let mut num = if n < 0 { -n } else { n };
    let mut i = 0;
    while num > 0 {
        buf[i] = b'0' + (num % 10) as u8;
        num /= 10;
        i += 1;
    }
    if n < 0 {
        buf[i] = b'-';
        i += 1;
    }
    let mut j = 0;
    while j < i / 2 {
        buf.swap(j, i - 1 - j);
        j += 1;
    }
    buf
}

fn print_num(n: isize) {
    let buf = format_num(n);
    let mut len = 0;
    while len < 20 && buf[len] != 0 { len += 1; }
    write(&buf[..len]);
}

#[unsafe(no_mangle)]
pub extern "C" fn main(argc: isize, argv: *const *const u8) -> ! {
    write(b"Async NoStd Demo\n");
    
    let worker_count = if argc > 1 {
        let arg = async_runtime::read_ptr_array(argv, 1);
        async_runtime::parse_cstring_usize(arg).unwrap_or(4)
    } else {
        4
    };

    write(b"Workers: ");
    print_num(worker_count as isize);
    write(b"\n\n");

    let executor = Executor::new();
    let exec = executor.clone();

    let demo = async move {
        write(b"[demo] Starting TCP echo\n");

        let sfd = sys::socket(AF_INET, SOCK_STREAM, 0);
        if sfd < 0 {
            write(b"[demo] Socket failed\n");
            return;
        }

        let _ = sys::fcntl(sfd, sys::F_SETFL, sys::O_NONBLOCK);
        
        let addr = SockAddrIn {
            sin_family: AF_INET as u16,
            sin_port: htons(0),
            sin_addr: 0x0100007fu32,
            sin_zero: [0u8; 8],
        };

        if sys::bind(sfd, (&addr as *const SockAddrIn) as *const u8, core::mem::size_of::<SockAddrIn>()) < 0 {
            write(b"[demo] Bind failed\n");
            let _ = sys::close(sfd);
            return;
        }

        if sys::listen(sfd, 4) < 0 {
            write(b"[demo] Listen failed\n");
            let _ = sys::close(sfd);
            return;
        }

        let mut sa = SockAddrIn { sin_family: 0, sin_port: 0, sin_addr: 0, sin_zero: [0u8; 8] };
        let mut len = core::mem::size_of::<SockAddrIn>();
        if sys::getsockname(sfd, (&mut sa as *mut SockAddrIn) as *mut u8, &mut len as *mut usize) >= 0 {
            let port = u16::from_be(sa.sin_port);
            write(b"[demo] Port: ");
            print_num(port as isize);
            write(b"\n");

            let server = async move {
                let cfd = AcceptFuture::new(sfd).await;
                if cfd >= 0 {
                    write(b"[server] Connected\n");
                    let mut buf = [0u8; 64];
                    let r = sys::recvfrom(cfd as i32, buf.as_mut_ptr(), 64, 0,
                                          core::ptr::null_mut(), core::ptr::null_mut());
                    if r > 0 {
                        write(b"[server] Recv: ");
                        write(&buf[..r as usize]);
                        let _ = sys::sendto(cfd as i32, b"pong\n".as_ptr(), 5, 0,
                                           core::ptr::null(), 0);
                    }
                    let _ = sys::close(cfd as i32);
                }
            };
            let _ = exec.enqueue_task(Box::new(server));

            let cfd = sys::socket(AF_INET, SOCK_STREAM, 0);
            if cfd >= 0 {
                let _ = sys::fcntl(cfd, sys::F_SETFL, sys::O_NONBLOCK);
                let client_addr = SockAddrIn {
                    sin_family: AF_INET as u16,
                    sin_port: htons(port),
                    sin_addr: 0x0100007fu32,
                    sin_zero: [0u8; 8],
                };

                let con = ConnectFuture::new(cfd, (&client_addr as *const SockAddrIn) as *const u8,
                                             core::mem::size_of::<SockAddrIn>()).await;
                if con >= 0 {
                    let _ = SendFuture::new(cfd, b"hello\n").await;
                    write(b"[client] Sent: hello\n");
                    
                    let resp = RecvFuture::new(cfd, 64).await;
                    if resp.len() > 0 {
                        write(b"[client] Recv: ");
                        write(&resp);
                    }
                }
                let _ = sys::close(cfd);
            }
        }

        let _ = sys::close(sfd);
        write(b"[demo] Done\n");
    };

    let _ = executor.enqueue_task(Box::new(demo));
    let _ = executor.start_workers(worker_count);
    executor.wait_all();

    write(b"\nCompleted\n");
    async_runtime::close_eventfd();
    sys::exit(0);
}
