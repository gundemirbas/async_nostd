//! Network primitives

#![no_std]

extern crate alloc;
use alloc::vec::Vec;
use core::pin::Pin;
use core::task::{Context, Poll};

pub const AF_INET: i32 = 2;
pub const SOCK_STREAM: i32 = 1;

#[repr(C)]
pub struct SockAddrIn {
    pub sin_family: u16,
    pub sin_port: u16,
    pub sin_addr: u32,
    pub sin_zero: [u8; 8],
}

pub fn htons(x: u16) -> u16 { x.to_be() }

// Futures
pub struct AcceptFuture {
    fd: i32,
    registered: bool,
}

impl AcceptFuture {
    pub fn new(fd: i32) -> Self { Self { fd, registered: false } }
}

impl core::future::Future for AcceptFuture {
    type Output = isize;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut sa_buf = [0u8; 32];
        let mut salen: usize = sa_buf.len();
        let r = async_syscall::accept4(self.fd, sa_buf.as_mut_ptr(), &mut salen as *mut usize, 0);
        if r >= 0 { return Poll::Ready(r); }
        if r == -11 {
            if !self.registered {
                async_runtime::register_fd_waker(self.fd, 0x0001, cx.waker().clone());
                self.registered = true;
            }
            return Poll::Pending;
        }
        Poll::Ready(r)
    }
}

pub struct ConnectFuture {
    fd: i32,
    addr: Vec<u8>,
    addrlen: usize,
    registered: bool,
}

impl ConnectFuture {
    pub fn new(fd: i32, addr_ptr: *const u8, addrlen: usize) -> Self {
        let mut v = Vec::with_capacity(addrlen);
        unsafe {
            v.set_len(addrlen);
            core::ptr::copy_nonoverlapping(addr_ptr, v.as_mut_ptr(), addrlen);
        }
        Self { fd, addr: v, addrlen, registered: false }
    }
}

impl core::future::Future for ConnectFuture {
    type Output = isize;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let r = async_syscall::connect(self.fd, self.addr.as_ptr(), self.addrlen);
        if r >= 0 { return Poll::Ready(r); }
        if r == -115 || r == -11 {
            if !self.registered {
                async_runtime::register_fd_waker(self.fd, 0x0004, cx.waker().clone());
                self.registered = true;
            }
            return Poll::Pending;
        }
        Poll::Ready(r)
    }
}

pub struct RecvFuture {
    fd: i32,
    buf: Vec<u8>,
    registered: bool,
}

impl RecvFuture {
    pub fn new(fd: i32, cap: usize) -> Self {
        let mut v = Vec::with_capacity(cap);
        unsafe { v.set_len(cap); }
        Self { fd, buf: v, registered: false }
    }
}

impl core::future::Future for RecvFuture {
    type Output = Vec<u8>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let r = async_syscall::recvfrom(self.fd, self.buf.as_mut_ptr(), self.buf.len(), 0,
                                        core::ptr::null_mut(), core::ptr::null_mut());
        if r > 0 {
            unsafe { self.buf.set_len(r as usize); }
            return Poll::Ready(core::mem::take(&mut self.buf));
        }
        if r == 0 {
            unsafe { self.buf.set_len(0); }
            return Poll::Ready(core::mem::take(&mut self.buf));
        }
        if r == -11 {
            if !self.registered {
                async_runtime::register_fd_waker(self.fd, 0x0001, cx.waker().clone());
                self.registered = true;
            }
            return Poll::Pending;
        }
        unsafe { self.buf.set_len(0); }
        Poll::Ready(core::mem::take(&mut self.buf))
    }
}

pub struct SendFuture {
    fd: i32,
    buf: Vec<u8>,
    registered: bool,
}

impl SendFuture {
    pub fn new(fd: i32, data: &[u8]) -> Self {
        let mut v = Vec::new();
        v.extend_from_slice(data);
        Self { fd, buf: v, registered: false }
    }
}

impl core::future::Future for SendFuture {
    type Output = isize;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let r = async_syscall::sendto(self.fd, self.buf.as_ptr(), self.buf.len(), 0,
                                      core::ptr::null(), 0);
        if r >= 0 { return Poll::Ready(r); }
        if r == -11 {
            if !self.registered {
                async_runtime::register_fd_waker(self.fd, 0x0004, cx.waker().clone());
                self.registered = true;
            }
            return Poll::Pending;
        }
        Poll::Ready(r)
    }
}
