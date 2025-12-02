//! Task executor

#![no_std]

extern crate alloc;
use alloc::boxed::Box;
use core::future::Future;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::task::{Context, Poll};

pub struct Executor;

impl Clone for Executor {
    fn clone(&self) -> Self { Self }
}

static TASKS_REMAINING: AtomicUsize = AtomicUsize::new(0);

impl Executor {
    pub fn new() -> Self { Self }

    pub fn enqueue_task(&self, task: Box<dyn Future<Output = ()> + Send + 'static>) -> Result<(), ()> {
        let _ = async_runtime::register_task(task);
        TASKS_REMAINING.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn start_workers(&self, num_workers: usize) -> Result<(), ()> {
        for _ in 0..num_workers {
            async_syscall::spawn_thread(worker_trampoline, core::ptr::null_mut(), 64 * 1024)?;
        }
        Ok(())
    }

    pub fn wait_all(&self) {
        while TASKS_REMAINING.load(Ordering::Relaxed) != 0 {
            core::hint::spin_loop();
        }
    }
}

extern "C" fn worker_trampoline(_arg: *mut u8) {
    loop {
        if TASKS_REMAINING.load(Ordering::Relaxed) == 0 { break; }

        if let Some(handle) = async_runtime::take_scheduled_task() {
            let waker = unsafe { async_runtime::create_waker_for_handle(handle) };
            let mut cx = Context::from_waker(&waker);
            let result = unsafe { async_runtime::poll_task(handle, &mut cx) };
            if let Poll::Ready(_) = result {
                TASKS_REMAINING.fetch_sub(1, Ordering::Relaxed);
            }
            continue;
        }

        async_runtime::ppoll_and_schedule();
    }
    async_syscall::exit(0);
}
