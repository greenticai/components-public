# Repository Overview

## 1. High-Level Purpose
This repository is a Rust Cargo workspace for public Greentic runtime components and design-extension packages. It centralizes shared package metadata, lint policy, and dependency versions for crates under `crates/`.

Current scope includes chat2data components, message/event adapters, `component-pack2flow`, `component-http`, `component-sorx-business`, extension packages for Designer node/tool metadata, HTTP shared logic, and fixture export tooling.

## 2. Main Components and Functionality
- **Path:** `Cargo.toml` (workspace root)
- **Role:** Workspace policy and shared dependency source of truth.
- **Key functionality:**
  - Declares members via `crates/*`.
  - Sets shared metadata including `rust-version = "1.91"`.
  - Stores Greentic dependency versions in root (`0.4` series).
  - Applies workspace lints (`unsafe_code = forbid`; clippy warnings).

- **Path:** `ci/local_check.sh`
- **Role:** Local CI wrapper.
- **Key functionality:**
  - Runs the repository's formatting, lint, build, and test checks.

- **Path:** `crates/component-chat2data-*`
- **Role:** Runtime components for parsing, translating, executing, rendering, and validating chat-to-data workflows.
- **Key functionality:**
  - Expose component-v0.6 operations and schemas via Rust `describe()` implementations and component manifests.
  - Include component-level validation, deterministic JSON outputs, and unit tests.

- **Path:** `crates/component-events2msg` and `crates/component-msg2events`
- **Role:** Runtime adapters between event-shaped and message-shaped payloads.
- **Key functionality:**
  - Route, extract, and validate payloads with JSON schema metadata and tests.

- **Path:** `crates/component-http` and `crates/http-core`
- **Role:** HTTP runtime component and shared HTTP configuration/request helpers.
- **Key functionality:**
  - `component-http` exposes request/stream operations for outbound HTTP use.
  - `http-core` owns reusable auth, URL, curl, config, and node parsing logic with focused tests.

- **Path:** `crates/component-pack2flow`
- **Role:** Deterministic transfer utility component (`local_router -> pack2flow`).
- **Key functionality:**
  - Parses transfer input (`target.flow`, optional `target.node`, params/hints/payload).
  - Emits transfer control directives and stable machine-readable error codes.
  - Includes unit and conformance tests for transfer contract behavior.

- **Path:** `crates/component-sorx-business`
- **Role:** Generic runtime client for locked Sorx business actions and read-only Sorx business queries.
- **Key functionality:**
  - Exposes action operations: `list_business_actions`, `get_business_action_schema`, `dry_run_locked_action`, `invoke_locked_action`.
  - Exposes read-only query/explain operations: `query_business_entity`, `query_business_evidence`, `explain_business_action_mapping`.
  - Uses a generic `action_ref` + `values` + `options` input contract for locked action calls.
  - Performs local envelope validation, contract hash shape checks, optional metadata/schema validation, idempotency checks, and dry-run-before-invoke behavior.
  - Normalizes Sorx success, server error, and contract drift responses.
  - Keeps Sorx authoritative for business semantics, policy, approvals, provider binding, execution, and audit.
  - Includes native unit tests for manifest/describe metadata, request mapping, validation, drift handling, output normalization, and domain-agnostic schemas.

- **Path:** `crates/http-extension`, `crates/webhook-extension`, `crates/llm-generic-extension`, `crates/platform-extension`
- **Role:** Designer extension packages.
- **Key functionality:**
  - Ship `describe.json` metadata, node type descriptors, tools, prompts, schemas, i18n, and generated WIT bindings as applicable.
  - Designer node types are contributed through `contributions.nodeTypes` with snake_case `config_schema`.
  - `http-extension` and `webhook-extension` declare full tool metadata in `contributions.tools[]`: `description`, `input_schema` (JSON Schema serialized as a string), and `capabilities`. These mirror each crate's `src/tools/mod.rs::list_tools()` table and its handlers — keep the two in sync. The declarative fields are the only source of tool metadata once an extension moves to `apiVersion: greentic.ai/v2`, where the runtime stops calling the wasm `list-tools` export.
  - `llm-generic-extension` and `platform-extension` contribute no tools.

- **Path:** `crates/gtest-fixture-exporter`
- **Role:** Helper for producing fixture metadata for Greentic test flows.

## 3. Work In Progress, TODOs, and Stubs
- **Location:** `crates/component-pack2flow/src/lib.rs` (`jump` adapter function)
- **Status:** Stub integration point
- **Short description:** Adapter currently validates inputs and serves as the integration point for future host/runtime transfer primitive binding.

- **Location:** `crates/component-pack2flow/README.md` (merge section)
- **Status:** Partial
- **Short description:** v1 uses shallow merge; deep merge is explicitly deferred.

Search for explicit markers (`TODO`, `FIXME`, `XXX`, `HACK`, `unimplemented!`, `todo!`, etc.) found no marker strings in tracked source files during the last overview refresh.

## 4. Broken, Failing, or Conflicting Areas
- **Location:** Workspace checks
- **Evidence:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, and `cargo test --workspace --all-targets` pass. `bash ci/local_check.sh` now gets past `make build` / `make test` and fails only on its last step, `greentic-integration-tester run --gtest tests/gtests/README ...`, with `command not found` when that binary is not installed in the environment. The previously recorded `rust-lld` failure on `make build` (WIT export names such as `cabi_post_greentic:extension-base/lifecycle@0.1.0#init`) did not reproduce during the last refresh.
- **Likely cause / nature of issue:** `greentic-integration-tester` is an external tool the wrapper assumes on `PATH`; it is not vendored in this repo.

- **Location:** `crates/http-extension/describe.json`, `crates/webhook-extension/describe.json`
- **Evidence:** `gtdx lint --dir <crate>` reports two errors on each file: `E_SCHEMA_HOST` (`$schema` still points at `describe-v1.json`) and `E_ENGINE_DEPRECATED` (the `engine` block should become `compat`). Both files also still use `runtime.component`, which the current `greentic-extension-sdk-contract` `Runtime` struct rejects in favour of `runtime.components`, so a whole-document `serde_json::from_str::<DescribeJson>` fails on them.
- **Likely cause / nature of issue:** Both extensions are still on `apiVersion: greentic.ai/v1` and have not been migrated to the v2 describe shape. Pre-existing; the migration changes runtime dispatch and belongs in its own change.

- **Location:** Publish/runtime workflows
- **Evidence:** Publish workflows depend on GHCR auth/permissions, target availability, and network connectivity.
- **Likely cause / nature of issue:** These workflows can fail in CI environments lacking package write permission, registry access, or required targets.

## 5. Notes for Future Work
- Bind `component-pack2flow` transfer behavior to the concrete runtime transfer primitive once the engine-facing API surface is available in this crate.
- Keep component manifests and Rust `describe()` schema metadata synchronized when adding new runtime components.
- Keep Designer node type descriptors in design-extension `describe.json` files, not runtime component manifests.
- Add Designer node types for Sorx business actions through a design-extension crate when the registry format and action discovery flow are finalized.
