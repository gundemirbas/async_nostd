#![no_std]
#![no_main]

mod syscall;
mod runtime;
mod executor;

// futures::ready not needed — example async removed
use syscall::{write, exit, print_cstring};
use runtime::read_ptr_array;
use executor::Executor;
extern crate alloc;
use alloc::boxed::Box;

/// Sayıyı yazdır
#[forbid(unsafe_code)]
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
        
        // Ters çevir
        while temp_i > 0 {
            temp_i -= 1;
            buf[i] = buf[temp_i];
            i += 1;
        }
    }
    
    write(&buf[..i]);
}

//let fut = async move {
async fn task_fut(i : isize) {
    write(b"Task ");
    // print task id
    print_number(i as isize);
    write(b" completed\n");
}

// Main modülünde unsafe kod yasak (runtime ve syscall modüllerinde izinli)
#[forbid(unsafe_code)]
fn main(argc: isize, argv: *const *const u8) -> ! {
    // argc ve argv bilgilerini stdout'a yazdır
    write(b"Program started\n");
    
    // argc'yi yazdır
    write(b"argc: ");
    print_number(argc);
    write(b"\n");
    
    // argv elemanlarını yazdır
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
    
    // Basit async fonksiyon demo
    write(b"\nRunning async function...\n");
    // Create an executor and run 32 concurrent tasks using cloned workers.
    let executor = Executor::new();

    // Enqueue 32 simple tasks
    for i in 0..32 {


        // Box the future and enqueue; ignore enqueue errors for simplicity
        let _ = executor.enqueue_task(Box::new(task_fut(i)));
    }

    // Start worker threads (e.g., 4 workers)
    // Determine worker count from argv[1] (if provided), default to 16
    let mut worker_count: usize = 16;
    if argc > 1 {
        let s = read_ptr_array(argv, 1);
        if let Some(n) = runtime::parse_cstring_usize(s) {
            if n > 0 { worker_count = n; }
        }
    }

    let _ = executor.start_workers(worker_count);

    // Wait until all tasks complete
    executor.wait_all();

    write(b"All concurrent tasks completed\n");
    
    exit(0);
}

// example async func removed — not used in this freestanding build

// No host entrypoint — this project targets a freestanding custom target.
