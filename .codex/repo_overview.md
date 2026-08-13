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
  - All four are on `apiVersion: greentic.ai/v2` (`$schema` = `store.greentic.cloud/schemas/describe-v2.json`). Migrated with `greentic_extension_sdk_contract::migration::migrate_v0_4_x_value`, the contract crate's own v1->v2 helper — use it rather than hand-editing if another describe needs migrating.
  - v2 shape notes: there is no `engine` block (version constraints live in `compat`); `runtime.components` is a map (this repo uses a single `main` entry) rather than the v1 `runtime.component` string; `NodeType.config_schema` and `Tool.input_schema` are JSON Schemas **serialized as strings**, not objects; `prompts` / `schemas` / `knowledge` entries are `{path}` objects, not bare strings.
  - `runtime.components.main.sha256` and `.gtpack.sha256` are committed as all-zero placeholders. `gtdx publish` substitutes the real `extension.wasm` digest while packing, so leave them as zeros; `gtdx lint --publish` flags them (`E_SHA256_ZERO`) but plain `gtdx lint` and `gtdx validate` are clean.
  - `http-extension` and `webhook-extension` declare full tool metadata in `contributions.tools[]`: `description`, `input_schema`, and `capabilities`. These mirror each crate's `src/tools/mod.rs::list_tools()` table and its handlers — keep the two in sync, because on the v2 path the runtime never calls the wasm `list-tools` export and the describe is the only source.
  - `llm-generic-extension` and `platform-extension` contribute no tools (their `list_tools()` returns an empty vec), so the v2 switch removes nothing from them.
  - The publish workflows pin `gtdx-version: "=1.3.0-research.1"`. The action's default installs the latest crates.io release (1.1.5), which is too old for these describes. Do not drop the pin without re-verifying via `gtdx publish --dry-run`.

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

- **Location:** `crates/*/describe.json` — `runtime.components.main.world`
- **Evidence:** The v1->v2 migration helper emits the literal `"main"` for `world`, and nothing in this repo supplies a better value: all four `wit/world.wit` files declare `package greentic:http; world extension`, including `webhook-extension`, `platform-extension`, and `llm-generic-extension`, whose `[package.metadata.component] package` says otherwise. `gtdx validate`, `gtdx lint`, and `gtdx publish --dry-run` all accept `"main"`.
- **Likely cause / nature of issue:** The `package greentic:http` line looks copy-pasted across the three non-http extensions. Deciding the real world reference needs the WIT packages fixed first; left as the helper default rather than guessed.

- **Location:** Publish/runtime workflows
- **Evidence:** Publish workflows depend on GHCR auth/permissions, target availability, and network connectivity.
- **Likely cause / nature of issue:** These workflows can fail in CI environments lacking package write permission, registry access, or required targets.

## 5. Notes for Future Work
- Bind `component-pack2flow` transfer behavior to the concrete runtime transfer primitive once the engine-facing API surface is available in this crate.
- Keep component manifests and Rust `describe()` schema metadata synchronized when adding new runtime components.
- Keep Designer node type descriptors in design-extension `describe.json` files, not runtime component manifests.
- Add Designer node types for Sorx business actions through a design-extension crate when the registry format and action discovery flow are finalized.
