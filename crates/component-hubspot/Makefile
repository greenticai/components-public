.PHONY: build test lint check wasm

BUILD_FLAGS ?=

default: build

build:
	cargo build --target wasm32-wasip2 --release

wasm:
	if ! cargo component --version >/dev/null 2>&1; then \
		echo "cargo-component is required for component@0.6.0 builds"; \
		echo "install with: cargo install cargo-component --locked"; \
		exit 1; \
	fi
	cargo component build --release --target wasm32-wasip2

lint:
	cargo fmt --all
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace --all-targets
