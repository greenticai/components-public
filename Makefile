.PHONY: build test

build:
	cargo build --workspace

test:
	cargo test --workspace --all-targets
