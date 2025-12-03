//! Runtime configuration constants

/// Maximum number of task slots
pub const MAX_TASK_SLOTS: usize = 1024;

/// Maximum freelist size for scheduler nodes
pub const FREELIST_MAX: usize = 256;

/// Heap size for bump allocator (16MB)
pub const HEAP_SIZE: usize = 16 * 1024 * 1024;

/// Default worker thread stack size (64KB)
pub const WORKER_STACK_SIZE: usize = 64 * 1024;

/// TLS block size (4KB)
pub const TLS_BLOCK_SIZE: usize = 4096;

/// Clone flags for thread spawning
/// CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_SETTLS
pub const CLONE_FLAGS: u64 = 0x100 | 0x200 | 0x400 | 0x800 | 0x10000 | 0x80000;

/// mmap protection flags (read + write)
pub const PROT_RW: i32 = 0x3;

/// mmap flags (private + anonymous)
pub const MAP_PRIVATE_ANON: i32 = 0x22;

/// Default log file path
pub const LOG_FILE_PATH: &[u8] = b"/tmp/async-nostd.log\0";

/// Socket listen backlog
pub const LISTEN_BACKLOG: i32 = 128;
