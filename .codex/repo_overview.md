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
- **Evidence:** `cargo test --workspace --all-targets` and `cargo clippy --workspace --all-targets` pass after adding `component-sorx-business`. `bash ci/local_check.sh` fails in its `make build` step (`cargo build --workspace`) while linking existing native extension crates (`platform-extension`, `llm-generic-extension`, `webhook-extension`): `rust-lld` rejects WIT export names such as `cabi_post_greentic:extension-base/lifecycle@0.1.0#init` in the native version script.
- **Likely cause / nature of issue:** Existing extension crates are cdylib/WIT-oriented and do not currently native-link cleanly under the repo-level `make build`; the new Sorx component builds natively and for `wasm32-wasip2`.

- **Location:** Publish/runtime workflows
- **Evidence:** Publish workflows depend on GHCR auth/permissions, target availability, and network connectivity.
- **Likely cause / nature of issue:** These workflows can fail in CI environments lacking package write permission, registry access, or required targets.

## 5. Notes for Future Work
- Bind `component-pack2flow` transfer behavior to the concrete runtime transfer primitive once the engine-facing API surface is available in this crate.
- Keep component manifests and Rust `describe()` schema metadata synchronized when adding new runtime components.
- Keep Designer node type descriptors in design-extension `describe.json` files, not runtime component manifests.
- Add Designer node types for Sorx business actions through a design-extension crate when the registry format and action discovery flow are finalized.
