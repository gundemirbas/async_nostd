//! Task executor

#![no_std]

extern crate alloc;
use alloc::boxed::Box;
use core::future::Future;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::task::{Context, Poll};

pub struct Executor;

impl Clone for Executor {
    fn clone(&self) -> Self {
        Self
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

static TASKS_REMAINING: AtomicUsize = AtomicUsize::new(0);

impl Executor {
    pub fn new() -> Self {
        Self
    }

    pub fn enqueue_task(&self, task: Box<dyn Future<Output = ()> + Send + 'static>) {
        let _ = async_runtime::register_task(task);
        TASKS_REMAINING.fetch_add(1, Ordering::Relaxed);
    }

    pub fn start_workers(&self, num_workers: usize) -> ! {
        // Wrapper to match spawn_thread signature
        extern "C" fn worker_wrapper(arg: *mut u8) {
            worker_loop(arg)
        }

        for _ in 0..num_workers {
            let _ = async_syscall::spawn_thread(worker_wrapper, core::ptr::null_mut(), async_runtime::WORKER_STACK_SIZE);
        }
        // Main thread becomes a worker too
        worker_loop(core::ptr::null_mut())
    }

    pub fn wait_all(&self) {
        while TASKS_REMAINING.load(Ordering::Relaxed) != 0 {
            core::hint::spin_loop();
        }
    }
}

extern "C" fn worker_loop(_arg: *mut u8) -> ! {
    // Worker runs forever, polling tasks and handling IO
    let mut poll_count = 0u64;
    loop {
        if let Some(handle) = async_runtime::take_scheduled_task() {
            poll_count += 1;
            if poll_count % 10000 == 0 {
                let _ = async_syscall::write(1, b"[worker] Polled ");
                async_syscall::write_usize(1, poll_count as usize);
                let _ = async_syscall::write(1, b" tasks\n");
            }
            let waker = async_runtime::create_waker(handle);
            let mut cx = Context::from_waker(&waker);
            
            let result = async_runtime::poll_task_safe(handle, &mut cx);
            match result {
                Poll::Ready(_) => {
                    TASKS_REMAINING.fetch_sub(1, Ordering::Relaxed);
                }
                Poll::Pending => {
                    // Task is waiting for IO, waker is registered.
                    // Do NOT re-schedule here; ppoll will wake it when ready.
                }
            }
            continue;
        }

        async_runtime::ppoll_and_schedule();
    }
}
