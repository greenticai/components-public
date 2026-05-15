# PR 01 - Add generic `component-sorx-business`

## Repository

Current checkout: `greentic-ai-org/components-public`

## Objective

Add a generic runtime component crate that lets flows call locked Sorx business action endpoints without embedding any domain-specific concepts.

The component must remain domain-agnostic. Do not hard-code tenant, flat, landlord, claim, order, invoice, plumber, or other vertical-specific fields in component schemas, Rust structs, tests, fixtures, or docs except as clearly marked external examples.

## Current repo facts to honor

- There is no `crates/component-sorx-business` crate in this checkout yet.
- The workspace includes crates through `members = ["crates/*"]`, so adding `crates/component-sorx-business` automatically makes it a workspace member.
- Runtime components in this repo use `crate-type = ["cdylib", "rlib"]`, `greentic:component/component-v0-v6-v0@0.6.0`, and `greentic_interfaces_guest::component_v0_6::node` for wasm exports.
- Runtime component operation schemas are declared in `component.manifest.json` under `operations[].input_schema` / `operations[].output_schema` and mirrored in Rust `describe()` metadata.
- Some existing components also expose native helper functions such as `describe_payload()` and `handle_message(...)` for tests, but the wasm ABI entry point is `node::Guest`.
- Host HTTP is available through the existing `greentic-interfaces-guest` `http-client-v1-1` feature, as used by `component-http`.

## New crate

Add:

```text
crates/component-sorx-business
```

Include:

```text
Cargo.toml
component.manifest.json
README.md
src/lib.rs
```

## Component purpose

The component is a thin client for Sorx runtime endpoints. It should not implement Sorx business logic locally.

Sorx remains authoritative for:

- action lock validation
- contract hash checks
- action schema validation
- policy and approvals
- provider bindings
- execution
- audit

## Operations

Expose these runtime operations:

```text
list_business_actions
get_business_action_schema
dry_run_locked_action
invoke_locked_action
```

Set `default_operation` to `invoke_locked_action` unless an existing runner convention in this repo requires otherwise.

Later PRs can add read-only entity/evidence queries.

## Configuration Schema

Add a component `config_schema` in `component.manifest.json` and Rust `describe()` metadata:

```json
{
  "type": "object",
  "required": ["sorx_base_url"],
  "properties": {
    "sorx_base_url": {
      "type": "string",
      "format": "uri",
      "title": "Sorx backend URL"
    },
    "auth": {
      "type": "object",
      "properties": {
        "kind": { "type": "string", "enum": ["none", "bearer_secret_ref"] },
        "secret_ref": { "type": "string" }
      },
      "additionalProperties": false
    },
    "timeout_ms": {
      "type": "integer",
      "minimum": 1,
      "default": 30000
    },
    "strict_tls": {
      "type": "boolean",
      "default": true
    }
  },
  "additionalProperties": false
}
```

Follow the existing component pattern for loading config from the invocation payload until this repo provides a separate typed runtime config channel. Keep that choice documented in the README.

## Operation Endpoints

Map operations to Sorx endpoints with request builder functions that are unit-testable without network:

```text
list_business_actions       GET  /v1/sorx/business-actions
get_business_action_schema  GET  /v1/sorx/business-actions/{id}
dry_run_locked_action       POST /v1/sorx/business-actions/{id}/dry-run
invoke_locked_action        POST /v1/sorx/business-actions/{id}/invoke
```

Normalize Sorx responses into:

```json
{
  "ok": true,
  "action_ref": {},
  "result": {},
  "sorx": {
    "status": 200,
    "audit_event_id": "...",
    "policy_decision": "allow",
    "approval_required": false
  },
  "explain": {}
}
```

Normalize errors into:

```json
{
  "ok": false,
  "error": {
    "code": "sorx_error",
    "message": "..."
  },
  "sorx": {
    "status": 400
  }
}
```

## Capabilities

Declare only necessary capabilities:

- host HTTP
- secrets, only for `auth.kind = "bearer_secret_ref"`
- telemetry if following the `component-http` logging pattern
- no filesystem
- network narrowed to the configured Sorx endpoint if the manifest/host policy supports narrowing

## Tests

Add focused native tests for:

- manifest JSON parses and includes all four operations
- Rust describe metadata includes all four operations
- list operation maps to `GET /v1/sorx/business-actions`
- get schema maps to `GET /v1/sorx/business-actions/{id}` with URL escaping or rejection of unsafe IDs
- dry run maps to `POST /v1/sorx/business-actions/{id}/dry-run`
- invoke maps to `POST /v1/sorx/business-actions/{id}/invoke`
- bearer auth header is added through a test seam without requiring real secrets
- Sorx success and error responses are normalized
- component schemas contain no domain-specific fields

## Docs

Add `crates/component-sorx-business/README.md` with:

- component purpose
- configuration contract
- operation input/output contracts
- endpoint mapping
- note that Sorx is authoritative for business semantics

Avoid claiming Designer node type behavior in this PR; Designer contributions belong in a design-extension descriptor.

## Acceptance Criteria

```bash
cargo test --workspace
cargo build --target wasm32-wasip2 --release
```
