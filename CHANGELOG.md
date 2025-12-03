# Changelog

## [0.2.0] - 2024-12-03 - Optimization & Documentation Release

### Added
- **Config Module** (`crates/runtime/config.rs`): Centralized configuration constants
  - `MAX_TASK_SLOTS`, `HEAP_SIZE`, `WORKER_STACK_SIZE`, `TLS_BLOCK_SIZE`
  - `CLONE_FLAGS`, `LOG_FILE_PATH`, `LISTEN_BACKLOG`
- **Comprehensive Logging**: Critical operation logging
  - `[ACCEPT]`, `[HTTP]`, `[WS]`, `[ppoll]` log prefixes
  - Log file: `/tmp/async-nostd.log` (truncated on startup)
- **Test Filtering**: Command-line test selection
  - `python3 test.py [all|http|ws|stress|browser|concurrent]`
- **Browser Simulation Tests**: Multiple concurrent connection testing
  - `test_browser_simulation()`: Single browser with hold time
  - `test_multiple_browsers()`: 3 simultaneous connections
  - `test_realtime_log_monitoring()`: 5 HTTP + 5 WS requests
- **Documentation**:
  - `README.md`: Project overview, build instructions, features
  - `PROJECT_STRUCTURE.md`: Detailed architecture, patterns, statistics
  - `CHANGELOG.md`: Version history

### Changed
- **Renamed**: `test_all.py` → `test.py`
- **Optimized**: Removed unused imports (`Ordering` in HTTP crate)
- **Simplified**: Removed empty `print_socket_info()` function
- **Improved**: Code organization with config constants
- **Enhanced**: Test output - only show failures + summary by default

### Removed
- **Dependency**: `async-pty` (unused, 13 lines)
- **File**: `test_all.py` (replaced by `test.py`)

### Fixed
- **Race Condition**: Closed FDs now properly removed from IO registry
- **CPU Spike**: 200% CPU usage after browser disconnect (fixed with POLLHUP detection)
- **Log Files**: Now truncated on startup (O_TRUNC flag)

### Performance
- Binary size: 35KB (unchanged)
- Test coverage: 28/28 passing (100%)
- Code: 2110 lines (was 2085, +25 from config.rs)

## [0.1.0] - Initial Release

### Features
- `#![no_std]` async runtime on bare Linux
- Lock-free task scheduler (Treiber stack)
- Multi-threaded executor (2-16 workers)
- HTTP server with embedded HTML
- WebSocket server with full protocol support
- 16MB bump allocator
- Real threads with TLS support
- Acceptor thread pattern (eliminates accept/ppoll races)

### Implementation
- 2085 lines of Rust
- 32KB binary (stripped)
- Zero external dependencies (except `spin`)
- Edition 2024 with workspace architecture
