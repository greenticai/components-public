#!/usr/bin/env bash
set -euo pipefail

cargo fmt
cargo clippy
cargo component build --release --target wasm32-wasip2
make build
make test
