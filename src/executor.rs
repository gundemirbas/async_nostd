use core::future::Future;
use core::task::{Context, Poll};
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

extern crate alloc;
use alloc::boxed::Box;

pub struct Executor;

static TASKS_REMAINING: AtomicUsize = AtomicUsize::new(0);
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

    const STACK_SIZE: usize = 64 * 1024;

    pub fn spawn_raw(&self, f: extern "C" fn(*mut u8), arg: *mut u8) -> Result<(), ()> {
        crate::syscall::spawn_thread(f, arg, Self::STACK_SIZE)
    }

    pub fn enqueue_task(&self, task: Box<dyn Future<Output = ()> + Send + 'static>) -> Result<(), ()> {
        storage_push(task);
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
    let waker = unsafe { crate::runtime::create_waker() };
    let mut cx = Context::from_waker(&waker);

    loop {
        if TASKS_REMAINING.load(Ordering::SeqCst) == 0 {
            break;
        }

        if let Some(mut task) = storage_take_first() {
            let result = unsafe { crate::runtime::poll_boxed_future(&mut task, &mut cx) };
            match result {
                Poll::Ready(_) => TASKS_REMAINING.fetch_sub(1, Ordering::SeqCst),
                Poll::Pending => panic!("Task returned Pending"),
            };
            continue;
        }

        core::hint::spin_loop();
    }

    crate::syscall::exit(0);
}