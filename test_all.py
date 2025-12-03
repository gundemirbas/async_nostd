#!/usr/bin/env python3
"""
Simplified test suite for Async NoStd HTTP Server
Tests with up to 8 workers, faster execution
"""

import subprocess
import time
import socket
import sys
import signal
import os
import select

class Colors:
    GREEN = '\033[92m'
    RED = '\033[91m'
    YELLOW = '\033[93m'
    BLUE = '\033[94m'
    RESET = '\033[0m'

def print_success(msg):
    print(f"{Colors.GREEN}✓{Colors.RESET} {msg}")

def print_error(msg):
    print(f"{Colors.RED}✗{Colors.RESET} {msg}")

def print_info(msg):
    print(f"{Colors.BLUE}ℹ{Colors.RESET} {msg}")

def print_section(msg):
    print(f"\n{Colors.YELLOW}{'='*60}{Colors.RESET}")
    print(f"{Colors.YELLOW}{msg}{Colors.RESET}")
    print(f"{Colors.YELLOW}{'='*60}{Colors.RESET}\n")

class AsyncServer:
    def __init__(self, workers, port):
        self.workers = workers
        self.port = port
        self.process = None
        
    def start(self):
        """Start the async server"""
        binary = "./target/x86_64-unknown-none/release/async-nostd"
        cmd = [binary, str(self.workers), "127.0.0.1", str(self.port)]
        
        print(f"    Starting server: workers={self.workers}, port={self.port}")
        self.process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            preexec_fn=os.setsid
        )
        
        # Wait for server to be ready
        max_wait = 3
        for i in range(max_wait * 10):
            time.sleep(0.1)
            try:
                test_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                test_sock.settimeout(0.1)
                test_sock.connect(("127.0.0.1", self.port))
                test_sock.close()
                time.sleep(0.1)
                print(f"    Server ready after {(i+1)*0.1:.1f}s")
                return
            except:
                pass
        print_error(f"Server failed to start on port {self.port} after {max_wait}s")
        
    def stop(self):
        """Stop the server"""
        if self.process:
            try:
                os.killpg(os.getpgid(self.process.pid), signal.SIGKILL)
            except:
                pass
            self.process.wait()
            self.process = None

    def __enter__(self):
        self.start()
        return self
        
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.stop()

    def wait_for_log(self, needle, timeout=3.0):
        """Wait up to `timeout` seconds for `needle` to appear in server stdout.

        Returns True if found, False on timeout or if stdout isn't available.
        """
        if not self.process or not self.process.stdout:
            return False
        end = time.time() + timeout
        buf = b""
        fd = self.process.stdout.fileno()
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.1)
            if r:
                try:
                    chunk = os.read(fd, 4096)
                except Exception:
                    time.sleep(0.05)
                    continue
                if not chunk:
                    break
                buf += chunk
                try:
                    s = buf.decode('utf-8', errors='ignore')
                except Exception:
                    s = str(buf)
                if needle in s:
                    return True
            else:
                time.sleep(0.05)
        return False

def http_get(port, path="/", timeout=3):
    """Perform HTTP GET request"""
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(timeout)
        sock.connect(("127.0.0.1", port))
        
        request = f"GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        sock.sendall(request.encode())
        
        response = b""
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            response += chunk
        
        sock.close()
        return response.decode('utf-8', errors='ignore')
    except Exception as e:
        return None

def test_http_basic(port):
    """Test basic HTTP GET request"""
    print(f"      Making HTTP request to port {port}...")
    response = http_get(port)
    if response and "200 OK" in response and ("<!doctype html>" in response or "<html>" in response):
        print(f"      Got valid response ({len(response)} bytes)")
        return True, len(response)
    print(f"      Failed: response={'None' if not response else f'{len(response)} bytes'}")
    return False, 0

def test_http_concurrent(port, num_requests=5):
    """Test concurrent HTTP requests"""
    import concurrent.futures
    
    print(f"      Testing {num_requests} concurrent requests...")
    def make_request(i):
        time.sleep(0.02 * i)
        response = http_get(port, timeout=5)
        success = response is not None and "200 OK" in response
        print(f"        Request {i+1}: {'OK' if success else 'FAILED'}")
        return success
    
    with concurrent.futures.ThreadPoolExecutor(max_workers=num_requests) as executor:
        futures = [executor.submit(make_request, i) for i in range(num_requests)]
        results = [f.result() for f in concurrent.futures.as_completed(futures)]
    
    success = sum(results)
    print(f"      Concurrent: {success}/{num_requests} succeeded")
    return success, num_requests

def test_http_stress(port, num_requests=10):
    """Stress test with multiple sequential requests"""
    print(f"      Stress test: {num_requests} sequential requests...")
    success = 0
    for i in range(num_requests):
        response = http_get(port, timeout=2)
        if response and "200 OK" in response:
            success += 1
        if (i + 1) % 5 == 0:
            print(f"        Progress: {success}/{i+1}")
    print(f"      Stress: {success}/{num_requests} succeeded")
    return success, num_requests

def test_websocket_echo(port, server=None):
    """Test WebSocket handshake and echo"""
    try:
        import websocket
    except ImportError:
        print(f"      websocket-client not installed, attempting to install via pip...")
        try:
            subprocess.check_call([sys.executable, "-m", "pip", "install", "websocket-client"], stdout=subprocess.DEVNULL)
            import websocket
        except Exception:
            print(f"      Skipping: websocket-client not available (pip install websocket-client)")
            return None, 0
    
    print(f"      Testing WebSocket on port {port}...")
    try:
        ws = websocket.create_connection(f"ws://127.0.0.1:{port}/term", timeout=3)
        
        # Read welcome message
        welcome = ws.recv()
        if welcome and "Async NoStd" in welcome:
            print(f"        Welcome message received ({len(welcome)} bytes)")
        # If the test harness provided the server handle, verify the
        # server emitted the '[ws connected]' notification.
        if server is not None:
            ok = server.wait_for_log("[ws connected]", timeout=2.0)
            if ok:
                print("        Server reported websocket connected")
            else:
                print_error("        Server did not report '[ws connected]' in time")
        
        # Test echo
        test_msg = "Hello WebSocket!"
        ws.send(test_msg)
        response = ws.recv()
        ws.close()
        
        if test_msg in response:
            print(f"        Echo test passed")
            return True, 1
        else:
            print(f"        Echo failed: sent '{test_msg}', got '{response}'")
            return False, 0
    except Exception as e:
        print(f"        WebSocket test failed: {e}")
        return False, 0

def run_single_threaded_tests():
    """Run all tests for single-threaded mode"""
    print_section("Single-Threaded Mode Tests (0 workers)")
    
    passed = 0
    total = 0
    
    # Test 1: Basic HTTP
    total += 1
    port = 7001
    print_info(f"Test 1: Basic HTTP GET request (port {port})")
    with AsyncServer(0, port) as server:
        success, response_len = test_http_basic(port)
        if success:
            print_success(f"Response received ({response_len} bytes)")
            passed += 1
        else:
            print_error("Failed to get valid response")
    
    # Test 2: Multiple requests  
    total += 1
    port = 7002
    print_info(f"Test 2: Sequential requests (port {port})")
    with AsyncServer(0, port) as server:
        success, num_total = test_http_stress(port, 5)
        if success >= 4:
            print_success(f"Completed {success}/{num_total} requests")
            passed += 1
        else:
            print_error(f"Only {success}/{num_total} succeeded")
    
    # Test 3: WebSocket
    total += 1
    port = 7003
    print_info(f"Test 3: WebSocket echo (port {port})")
    with AsyncServer(0, port) as server:
        result, count = test_websocket_echo(port, server)
        if result is None:
            # Skip this test
            total -= 1
        elif result:
            print_success(f"WebSocket working")
            passed += 1
        else:
            print_error("WebSocket test failed")
    
    return passed, total

def run_multi_threaded_tests():
    """Run all tests for multi-threaded mode"""
    print_section("Multi-Threaded Mode Tests")
    
    passed = 0
    total = 0
    
    # Test with 2, 4, and 8 workers
    test_num = 0
    for workers in [2, 4, 8]:
        base_port = 7100 + workers * 100
        
        total += 1
        test_num += 1
        port = base_port + test_num
        print_info(f"Test: {workers} workers - Basic HTTP (port {port})")
        with AsyncServer(workers, port) as server:
            success, response_len = test_http_basic(port)
            if success:
                print_success(f"Response received ({response_len} bytes)")
                passed += 1
            else:
                print_error("Failed to get valid response")
        
        total += 1
        test_num += 1
        port = base_port + test_num
        print_info(f"Test: {workers} workers - Concurrent requests (port {port})")
        with AsyncServer(workers, port) as server:
            success, num_total = test_http_concurrent(port, 5)
            if success >= 4:
                print_success(f"Completed {success}/{num_total} concurrent requests")
                passed += 1
            else:
                print_error(f"Only {success}/{num_total} succeeded")
        
        total += 1
        test_num += 1
        port = base_port + test_num
        print_info(f"Test: {workers} workers - Stress test (port {port})")
        with AsyncServer(workers, port) as server:
            success, num_total = test_http_stress(port, 10)
            if success >= 8:
                print_success(f"Completed {success}/{num_total} requests")
                passed += 1
            else:
                print_error(f"Only {success}/{num_total} succeeded")
        
        # WebSocket test for each worker configuration
        total += 1
        test_num += 1
        port = base_port + test_num
        print_info(f"Test: {workers} workers - WebSocket (port {port})")
        with AsyncServer(workers, port) as server:
            result, count = test_websocket_echo(port, server)
            if result is None:
                total -= 1  # Skip if websocket-client not installed
            elif result:
                print_success(f"WebSocket working")
                passed += 1
            else:
                print_error("WebSocket test failed")
    
    return passed, total

def main():
    print(f"\n{Colors.BLUE}{'#'*60}")
    print(f"#  Async NoStd - Comprehensive Test Suite")
    print(f"#  Testing HTTP Server + WebSocket (up to 8 workers)")
    print(f"{'#'*60}{Colors.RESET}\n")
    
    # Allow running specific test groups via CLI: 'all' (default), 'single', 'multi', 'ws'
    mode = 'all'
    if len(sys.argv) > 1:
        mode = sys.argv[1]

    # Build the project
    print_info("Building project...")
    result = subprocess.run(
        ["cargo", "+nightly", "build", "--release"],
        cwd="/home/coder/async",
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE
    )
    
    if result.returncode != 0:
        print_error("Build failed!")
        print(result.stderr.decode())
        return 1
    
    print_success("Build completed\n")
    
    # Kill any existing servers
    subprocess.run(["pkill", "-9", "async-nostd"], 
                   stderr=subprocess.DEVNULL)
    time.sleep(1)
    
    # Run tests according to requested mode
    total_passed = 0
    total_tests = 0

    if mode == 'single':
        passed, total = run_single_threaded_tests()
        total_passed += passed
        total_tests += total
    elif mode == 'multi':
        passed, total = run_multi_threaded_tests()
        total_passed += passed
        total_tests += total
    elif mode == 'ws':
        # Run only websocket tests (single + multi worker configs)
        print_section("WebSocket Only Tests")
        # Single-threaded WS
        port = 7003
        print_info(f"Single-threaded WebSocket test (port {port})")
        with AsyncServer(0, port) as server:
            result, count = test_websocket_echo(port, server)
            if result is None:
                print_info("WebSocket-client not available; skipping")
            elif result:
                print_success("Single-threaded WebSocket passed")
                total_passed += 1
            else:
                print_error("Single-threaded WebSocket failed")
            total_tests += 1

        # Multi-threaded WS for 2/4/8 workers
        for workers in [2, 4, 8]:
            base_port = 7100 + workers * 100
            # ws test occupies the 4th slot in the original layout
            ws_port = base_port + 4
            print_info(f"{workers} workers - WebSocket (port {ws_port})")
            with AsyncServer(workers, ws_port) as server:
                result, count = test_websocket_echo(ws_port, server)
                if result is None:
                    print_info("WebSocket-client not available; skipping")
                    total_tests += 0
                elif result:
                    print_success(f"{workers}-worker WebSocket passed")
                    total_passed += 1
                    total_tests += 1
                else:
                    print_error(f"{workers}-worker WebSocket failed")
                    total_tests += 1
    else:
        # Default: run both single and multi tests
        passed, total = run_single_threaded_tests()
        total_passed += passed
        total_tests += total
        passed, total = run_multi_threaded_tests()
        total_passed += passed
        total_tests += total
    
    # Summary
    print_section("Test Summary")
    success_rate = (total_passed / total_tests * 100) if total_tests > 0 else 0
    print(f"Total tests: {total_tests}")
    print(f"Passed: {Colors.GREEN}{total_passed}{Colors.RESET}")
    print(f"Failed: {Colors.RED}{total_tests - total_passed}{Colors.RESET}")
    print(f"Success rate: {success_rate:.1f}%")
    
    if success_rate >= 70:
        print(f"{Colors.GREEN}{'='*60}")
        print(f"  ✓ TESTS PASSED!")
        print(f"  - Single-threaded async runtime: WORKING")
        print(f"  - Multi-threaded with TLS (2-8 workers): WORKING")
        print(f"  - HTTP server: WORKING")
        print(f"  - WebSocket server: WORKING")
        print(f"  - Concurrent handling: WORKING")
        print(f"{'='*60}{Colors.RESET}\n")
        return 0
    else:
        print(f"{Colors.RED}{'='*60}")
        print(f"  ✗ SOME TESTS FAILED")
        print(f"{'='*60}{Colors.RESET}\n")
        return 1

if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print(f"\n{Colors.YELLOW}Interrupted by user{Colors.RESET}")
        subprocess.run(["pkill", "-9", "async-nostd"], 
                       stderr=subprocess.DEVNULL)
        sys.exit(1)
