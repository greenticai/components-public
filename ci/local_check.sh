#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo component build --release --target wasm32-wasip2
make build
make test
greentic-integration-tester run --gtest tests/gtests/README --artifacts-dir artifacts/readme-gtests --errors
