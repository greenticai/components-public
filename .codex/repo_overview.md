# Repository Overview

## 1. High-Level Purpose
This repository is a Rust Cargo workspace for public Greentic components. It centralizes shared package metadata, lint policy, and dependency versions for crates under `crates/`.

Current scope is a single component crate (`component-pack2flow`) plus CI tooling. The component now implements deterministic in-pack transfer (`jump/replace`) behavior with validation, redirect guardrails, and machine-readable error codes.

## 2. Main Components and Functionality
- **Path:** `Cargo.toml` (workspace root)
- **Role:** Workspace policy and shared dependency source of truth.
- **Key functionality:**
  - Declares members via `crates/*`.
  - Sets shared metadata including `rust-version = "1.91"`.
  - Stores Greentic dependency versions in root (`0.4` series).
  - Applies workspace lints (`unsafe_code = forbid`; clippy warnings).
- **Key dependencies / integration points:**
  - Member crates use `*.workspace = true` so versions are only defined at root.

- **Path:** `rust-toolchain.toml`
- **Role:** Toolchain pin for local/CI consistency.
- **Key functionality:**
  - Pins channel `1.91.0`.

- **Path:** `ci/local_check.sh`
- **Role:** Local CI wrapper.
- **Key functionality:**
  - Runs `cargo fmt`, `cargo clippy`, `make build`, `make test`.

- **Path:** `Makefile` (root)
- **Role:** Repo-level command aliases.
- **Key functionality:**
  - `make build` -> `cargo build --workspace`
  - `make test` -> `cargo test --workspace --all-targets`

- **Path:** `.github/workflows/publish.yml`
- **Role:** Parallel quality checks and GHCR publish pipeline.
- **Key functionality:**
  - Runs `fmt`, `clippy`, and `test` in parallel jobs.
  - Builds wasm for `component-pack2flow` and pushes to GHCR with version + `latest` tags.

- **Path:** `crates/component-pack2flow`
- **Role:** Deterministic transfer utility component (`local_router -> pack2flow`).
- **Key functionality:**
  - Parses transfer input (`target.flow`, optional `target.node`, params/hints/payload).
  - Validates target flow/node against runtime-provided pack descriptor (`meta.descriptor` fallback contract).
  - Resolves start node in order: explicit node -> `start_node` in descriptor -> constant `start`.
  - Enforces redirect guard using canonical `trace.redirect_count` with fallback read from `meta.redirect_count`.
  - Applies caller-wins merge semantics (shallow):
    - `merged_payload = shallow_merge(params, payload)` where payload overrides.
    - `merged_hints = shallow_merge(hints, routing_hints)` where routing hints override.
  - Returns contract on success with `status=ok`, `jumped_to`, `trace.redirect_count`, and transfer control directive.
  - Returns contract on failure with `status=error` and stable codes:
    - `missing_flow`, `unknown_flow`, `unknown_node`, `redirect_limit`, `invalid_input`, `jump_failed`.
  - Includes unit tests for validation/merge/redirect behavior and conformance tests for transfer contract output.
- **Key dependencies / integration points:**
  - Uses Greentic guest/runtime crates for wasm target exports.
  - Emits control directive payload for host flow engine transfer handling.

## 3. Work In Progress, TODOs, and Stubs
- **Location:** `crates/component-pack2flow/src/lib.rs` (`jump` adapter function)
- **Status:** Stub integration point
- **Short description:** Adapter currently validates inputs and serves as placeholder seam; actual engine primitive call is expected to be wired through this function when host/runtime API is bound.

- **Location:** `crates/component-pack2flow/README.md` (merge section)
- **Status:** Partial
- **Short description:** v1 uses shallow merge; deep merge is explicitly deferred.

Search for explicit markers (`TODO`, `FIXME`, `XXX`, `HACK`, `unimplemented!`, `todo!`, etc.) found no marker strings in tracked files.

## 4. Broken, Failing, or Conflicting Areas
- **Location:** Workspace checks
- **Evidence:** `./ci/local_check.sh` passes; `cargo clippy --workspace --all-targets -- -D warnings` passes; tests pass.
- **Likely cause / nature of issue:** No current compile/test/lint failures detected.

- **Location:** Publish runtime assumptions
- **Evidence:** Publish workflow depends on GHCR auth/permissions and network availability at runtime.
- **Likely cause / nature of issue:** Workflow can fail in CI environments lacking package write permission or registry connectivity.

## 5. Notes for Future Work
- Bind `jump(...)` to the concrete runtime transfer primitive once the engine-facing API surface is available in this crate.
- Upgrade merge behavior from shallow to deep while preserving caller-wins precedence.
- Add integration tests that execute full host-runner transfer behavior (not only component-level contract output).
