#!/usr/bin/env bash
set -euo pipefail

# Build
cargo +nightly build --release

BIN=./target/x86_64-unknown-none/release/async_futures_project

# Run the binary and capture output
$BIN | tee /tmp/tcp_demo.out

# Check for expected messages
if grep -q "hello" /tmp/tcp_demo.out && grep -q "pong" /tmp/tcp_demo.out; then
  echo "TCP demo succeeded"
  exit 0
else
  echo "TCP demo failed: expected 'hello' and 'pong' in output"
  exit 2
fi
