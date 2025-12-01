#![no_std]
#![no_main]

mod syscall;
mod runtime;

use futures::future::ready;
use syscall::{write, exit, print_cstring};
use runtime::{Executor, read_ptr_array};

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
    let mut future = simple_async_function();
    
    // Executor ile future'ı çalıştır
    let executor = Executor::new();
    let _result = executor.block_on(&mut future);
    
    write(b"Async function completed\n");
    
    exit(0);
}

// Basit bir async fonksiyon
#[forbid(unsafe_code)]
async fn simple_async_function() -> i32 {
    let value = ready(42).await;
    value * 2
}
