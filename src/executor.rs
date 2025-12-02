use core::future::Future;
use core::task::{Context, Poll};
use core::sync::atomic::{AtomicUsize, Ordering};
// `spin::Mutex` is not needed here; runtime provides synchronization.

extern crate alloc;
use alloc::boxed::Box;

pub struct Executor;

impl Clone for Executor { fn clone(&self) -> Self { Self } }

static TASKS_REMAINING: AtomicUsize = AtomicUsize::new(0);

impl Executor {
    pub fn new() -> Self { Self }

    const STACK_SIZE: usize = 64 * 1024;

    pub fn spawn_raw(&self, f: extern "C" fn(*mut u8), arg: *mut u8) -> Result<(), ()> {
        crate::syscall::spawn_thread(f, arg, Self::STACK_SIZE)
    }

    pub fn enqueue_task(&self, task: Box<dyn Future<Output = ()> + Send + 'static>) -> Result<(), ()> {
        // Register the task with the runtime scheduler and increment outstanding counter.
        // We don't need the returned handle here.
        let _ = crate::runtime::register_task(task);
        TASKS_REMAINING.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub fn start_workers(&self, num_workers: usize) -> Result<(), ()> {
        for _ in 0..num_workers {
            self.spawn_raw(worker_trampoline, core::ptr::null_mut())?;
        }
        Ok(())
    }

    pub fn wait_all(&self) {
        while TASKS_REMAINING.load(Ordering::SeqCst) != 0 {
            core::hint::spin_loop();
        }
    }
}

extern "C" fn worker_trampoline(_arg: *mut u8) {
    // Worker loop uses runtime scheduler. It takes a scheduled task handle,
    // creates a waker for that handle and polls the task via runtime API.
    loop {
        if TASKS_REMAINING.load(Ordering::SeqCst) == 0 {
            break;
        }

        if let Some(handle) = crate::runtime::take_scheduled_task() {
            let waker = unsafe { crate::runtime::create_waker_for_handle(handle) };
            let mut cx = Context::from_waker(&waker);
            let result = unsafe { crate::runtime::poll_task(handle, &mut cx) };
            match result {
                Poll::Ready(_) => { TASKS_REMAINING.fetch_sub(1, Ordering::SeqCst); }
                Poll::Pending => {
                    // pending tasks will be woken via their Waker (runtime::wake_handle)
                }
            }
            continue;
        }

        // No scheduled tasks — let runtime poll registered fds and schedule tasks.
        crate::runtime::ppoll_and_schedule();
    }
    crate::syscall::exit(0);
}