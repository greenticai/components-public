# PR-P2F-01: component-pack2flow — deterministic in-pack jump
Date: 2026-02-24

## Goal
Introduce a reusable component **pack2flow** that can be embedded in any pack flow to **jump** (transfer control) to another flow/node inside the same pack.

This component does **not** do “routing” decisions. It only performs the mechanics of jumping safely and consistently.

## Scope (v1)
- Support **jump/replace** semantics only (no call/return).
- Validate target exists (flow, node if specified) before transferring.
- Enforce redirect loop guard (max redirects) using a counter in payload/state.

## Component contract (v1, convention)
Input payload (CBOR map or your standard envelope payload):
- `target.flow` (string, required)
- `target.node` (string, optional; default start node for that flow)
- `params` (map, optional) — merged into current payload for target flow/node
- `hints` (map, optional) — merged into routing hints for target
- `max_redirects` (int, optional; default 3)
- `reason` (string, optional) — for logs/audit

Output:
- On success: `{"status":"ok","jumped_to":{"flow":"...","node":"...?"}}`
- On failure: `{"status":"error","error":{"code":"...","text":"..."}}`

## Implementation plan

### 1) New component crate/repo layout
Repo (or crate) name: `component-pack2flow`
- `src/lib.rs` (component entrypoints)
- `wit/` (component world, minimal: `handle(payload)` + `describe()` if you use descriptors)
- `tests/` (unit + fixture packs)
- CI: build wasm32-wasip2, run tests

### 2) Implement jump logic
- Parse input payload map.
- Resolve flow/node in the current pack descriptor/registry.
- Loop guard:
  - read `trace.redirect_count` or `meta.redirect_count`
  - if >= max_redirects → error `redirect_limit`
  - else increment
- Execute jump using engine primitive (or equivalent): transfer to `{flow,node}` with merged payload.

### 3) Validation behavior
- Missing `target.flow` → `missing_flow`
- Unknown flow → `unknown_flow`
- Unknown node → `unknown_node`
- Redirect limit reached → `redirect_limit`

### 4) Tests
- Unit tests: parsing + validation.
- Fixture pack integration test:
  - flow A calls pack2flow to flow B; B returns marker
- Loop test: A jumps to A until limit; verify error.

### 5) Docs
- README with usage pattern: `local_router → pack2flow`

## Acceptance criteria
- pack2flow wasm builds and is embed-ready.
- Deterministic jump works with guardrails.
- No dependency added to greentic-interfaces/qa/providers.
