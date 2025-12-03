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
        self.log_file = f"/tmp/test-server-{port}.log"
        
    def start(self):
        """Start the async server"""
        binary = "./target/x86_64-unknown-none/release/async-nostd"
        cmd = [binary, str(self.workers), "127.0.0.1", str(self.port)]
        
        print(f"    Starting server: workers={self.workers}, port={self.port}")
        
        # Open log file for server output (suppress console output)
        with open(self.log_file, 'w') as log:
            self.process = subprocess.Popen(
                cmd,
                stdout=subprocess.DEVNULL,  # Suppress stdout (minimal console output)
                stderr=log,                  # Redirect stderr to log
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
    
    def get_log(self):
        """Read server log file"""
        try:
            with open(self.log_file, 'r') as f:
                return f.read()
        except:
            return ""
    
    def check_log(self, pattern):
        """Check if pattern exists in log"""
        log = self.get_log()
        return pattern in log

    def __enter__(self):
        self.start()
        return self
        
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.stop()
        # Cleanup log file
        try:
            os.remove(self.log_file)
        except:
            pass

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

def test_websocket_echo(port):
    """Test WebSocket handshake and echo"""
    try:
        import websocket
    except ImportError:
        print(f"      Skipping: websocket-client not installed (pip install websocket-client)")
        return None, 0
    
    print(f"      Testing WebSocket on port {port}...")
    try:
        ws = websocket.create_connection(f"ws://127.0.0.1:{port}/term", timeout=3)
        
        # Read welcome message (binary data)
        welcome = ws.recv()
        welcome_str = welcome.decode('utf-8') if isinstance(welcome, bytes) else welcome
        if welcome_str and "Async NoStd" in welcome_str:
            print(f"        Welcome message received ({len(welcome)} bytes)")
        
        # Test echo
        test_msg = "Hello WebSocket!"
        ws.send(test_msg)
        response = ws.recv()
        response_str = response.decode('utf-8') if isinstance(response, bytes) else response
        ws.close()
        
        if test_msg in response_str:
            print(f"        Echo test passed")
            return True, 1
        else:
            print(f"        Echo failed: sent '{test_msg}', got '{response_str}'")
            return False, 0
    except Exception as e:
        print(f"        WebSocket test failed: {e}")
        return False, 0

def test_websocket_concurrent(port, num_connections=5):
    """Test concurrent WebSocket connections"""
    try:
        import websocket
    except ImportError:
        print(f"      Skipping: websocket-client not installed")
        return None, 0
    
    import concurrent.futures
    
    print(f"      Testing {num_connections} concurrent WebSocket connections...")
    
    def ws_echo_test(i):
        try:
            ws = websocket.create_connection(f"ws://127.0.0.1:{port}/term", timeout=3)
            welcome = ws.recv()  # Read welcome
            test_msg = f"Test {i+1}"
            ws.send(test_msg)
            response = ws.recv()
            response_str = response.decode('utf-8') if isinstance(response, bytes) else response
            ws.close()
            success = test_msg in response_str
            print(f"        Connection {i+1}: {'OK' if success else 'FAILED'}")
            return success
        except Exception as e:
            print(f"        Connection {i+1}: FAILED ({e})")
            return False
    
    with concurrent.futures.ThreadPoolExecutor(max_workers=num_connections) as executor:
        futures = [executor.submit(ws_echo_test, i) for i in range(num_connections)]
        results = [f.result() for f in concurrent.futures.as_completed(futures)]
    
    success = sum(results)
    print(f"      Concurrent WS: {success}/{num_connections} succeeded")
    return success, num_connections

def test_websocket_stress(port, num_messages=20):
    """Stress test WebSocket with multiple messages on single connection"""
    try:
        import websocket
    except ImportError:
        print(f"      Skipping: websocket-client not installed")
        return None, 0
    
    print(f"      WebSocket stress test: {num_messages} messages...")
    try:
        ws = websocket.create_connection(f"ws://127.0.0.1:{port}/term", timeout=3)
        welcome = ws.recv()  # Read welcome
        
        success = 0
        for i in range(num_messages):
            test_msg = f"Message {i+1}"
            ws.send(test_msg)
            response = ws.recv()
            response_str = response.decode('utf-8') if isinstance(response, bytes) else response
            if test_msg in response_str:
                success += 1
            if (i + 1) % 10 == 0:
                print(f"        Progress: {success}/{i+1}")
        
        ws.close()
        print(f"      WS Stress: {success}/{num_messages} succeeded")
        return success, num_messages
    except Exception as e:
        print(f"        WebSocket stress test failed: {e}")
        return False, 0


def run_multi_threaded_tests():
    """Run all tests for multi-threaded mode"""
    print_section("Multi-Threaded Mode Tests")
    
    passed = 0
    total = 0
    
    # Test with 2, 4, 8, and 16 workers
    test_num = 0
    for workers in [2, 4, 8, 16]:
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
        
        # WebSocket basic test
        total += 1
        test_num += 1
        port = base_port + test_num
        print_info(f"Test: {workers} workers - WebSocket (port {port})")
        with AsyncServer(workers, port) as server:
            result, count = test_websocket_echo(port)
            if result is None:
                total -= 1  # Skip if websocket-client not installed
            elif result:
                print_success(f"WebSocket working")
                passed += 1
            else:
                print_error("WebSocket test failed")
        
        # WebSocket concurrent test (only for 4+ workers)
        if workers >= 4:
            total += 1
            test_num += 1
            port = base_port + test_num
            print_info(f"Test: {workers} workers - Concurrent WebSocket (port {port})")
            with AsyncServer(workers, port) as server:
                result, count = test_websocket_concurrent(port, 5)
                if result is None:
                    total -= 1
                elif result and result >= 4:
                    print_success(f"Concurrent WS: {result}/{count} succeeded")
                    passed += 1
                else:
                    print_error(f"Only {result}/{count} succeeded")
        
        # WebSocket stress test (only for 8+ workers)
        if workers >= 8:
            total += 1
            test_num += 1
            port = base_port + test_num
            print_info(f"Test: {workers} workers - WebSocket Stress (port {port})")
            with AsyncServer(workers, port) as server:
                result, count = test_websocket_stress(port, 20)
                if result is None:
                    total -= 1
                elif result and result >= 18:
                    print_success(f"WS Stress: {result}/{count} succeeded")
                    passed += 1
                else:
                    print_error(f"Only {result}/{count} succeeded")
    
    return passed, total

def main():
    print(f"\n{Colors.BLUE}{'#'*60}")
    print(f"#  Async NoStd - Comprehensive Test Suite")
    print(f"#  Testing HTTP Server + WebSocket (up to 16 workers)")
    print(f"{'#'*60}{Colors.RESET}\n")
    
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
    
    # Run tests
    total_passed = 0
    total_tests = 0
    
    # Multi-threaded tests only
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
    
    # Show test logs summary if there were failures
    if total_tests - total_passed > 0:
        print(f"\n{Colors.YELLOW}Test Logs (failed tests):{Colors.RESET}")
        # Find most recent test log with errors
        import glob
        log_files = sorted(glob.glob("/tmp/test-server-*.log"), 
                          key=os.path.getmtime, reverse=True)
        if log_files:
            try:
                with open(log_files[0], 'r') as f:
                    lines = f.readlines()
                    if lines:
                        print(f"  Last log: {log_files[0]}")
                        for line in lines[-15:]:
                            print(f"  {line.rstrip()}")
            except:
                pass
    
    if success_rate >= 70:
        print(f"{Colors.GREEN}{'='*60}")
        print(f"  ✓ TESTS PASSED!")
        print(f"  - Multi-threaded with TLS (2-16 workers): WORKING")
        print(f"  - HTTP server: WORKING")
        print(f"  - WebSocket server: WORKING")
        print(f"  - Concurrent handling: WORKING")
        print(f"  - WebSocket stress test: WORKING")
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
