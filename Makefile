.PHONY: build test

# The WIT design-extension crates ship a checked-in wit-bindgen `bindings.rs`
# whose `cabi_post_*` export symbols (containing `:`/`@`/`#`) are NOT cfg-gated
# to wasm32, so a NATIVE cdylib link emits a version-script that rust-lld 1.95
# rejects. These crates are only ever consumed as wasm components (built by
# `cargo component build --target wasm32-wasip2`); a native cdylib is never
# produced in CI or distribution. Exclude them from the native workspace build.
# Their non-link compilation is still covered by `cargo test`/`clippy
# --all-targets` (here and in CI), which build the rlib without materializing
# the cdylib.
build:
	cargo build --workspace \
		--exclude http-extension \
		--exclude webhook-extension \
		--exclude platform-extension

test:
	cargo test --workspace --all-targets
