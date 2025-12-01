use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

extern crate alloc;
use alloc::boxed::Box;

// Minimal concurrent executor for the freestanding target.
// - TASK_STORAGE: spinlock-protected Vec<Option<Box<dyn Future>>>.
// - TASKS_REMAINING: atomic counter for outstanding tasks.
// - Workers poll tasks until no tasks remain. Tasks must complete on first poll.

pub struct Executor;

static TASKS_REMAINING: AtomicUsize = AtomicUsize::new(0);

// Spinlock for TASK_STORAGE
static TASK_STORAGE: Mutex<Option<alloc::vec::Vec<Option<Box<dyn Future<Output = ()> + Send + 'static>>>>> =
    Mutex::new(None);

fn storage_push(task: Box<dyn Future<Output = ()> + Send + 'static>) -> usize {
    let mut guard = TASK_STORAGE.lock();
    if guard.is_none() {
        *guard = Some(alloc::vec::Vec::new());
    }
    let storage = guard.as_mut().unwrap();
    let idx = storage.len();
    storage.push(Some(task));
    idx
}

fn storage_take_first() -> Option<Box<dyn Future<Output = ()> + Send + 'static>> {
    let mut guard = TASK_STORAGE.lock();
    if let Some(vec) = guard.as_mut() {
        for slot in vec.iter_mut() {
            if slot.is_some() {
                return slot.take();
            }
        }
    }
    None
}

impl Executor {
    pub fn new() -> Self { Self }

    // `block_on` removed — executor is worker-driven in this freestanding runtime.

    const STACK_SIZE: usize = 64 * 1024;

    pub fn spawn_raw(&self, f: extern "C" fn(*mut u8), arg: *mut u8) -> Result<(), ()> {
        crate::syscall::spawn_thread(f, arg, Executor::STACK_SIZE)
    }

    /// Enqueue a boxed future and increment outstanding counter.
    pub fn enqueue_task(&self, task: Box<dyn Future<Output = ()> + Send + 'static>) -> Result<(), ()> {
        let _idx = storage_push(task);
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
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker { RawWaker::new(core::ptr::null(), &VTABLE) }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);

    let waker = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);

    loop {
        if TASKS_REMAINING.load(Ordering::SeqCst) == 0 {
            break;
        }

        if let Some(task) = storage_take_first() {
            // Poll outside the lock
            unsafe {
                let mut boxed = task;
                let mut pinned = Pin::new_unchecked(boxed.as_mut());
                match pinned.as_mut().poll(&mut cx) {
                    Poll::Ready(_) => {
                        TASKS_REMAINING.fetch_sub(1, Ordering::SeqCst);
                    }
                    Poll::Pending => panic!("Task returned Pending; executor does not support waking"),
                }
            }
            continue;
        }

        core::hint::spin_loop();
    }

    crate::syscall::exit(0);
}
