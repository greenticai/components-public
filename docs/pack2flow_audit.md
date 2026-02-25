# pack2flow Audit (P2F-01)

Date: 2026-02-25

## Findings

- Component runtime contract uses the `greentic:component/component-v0-v6-v0@0.6.0` world.
- Runner consumes CBOR payloads from `component-runtime.run`.
- Returning a structured jump intent can be done cleanly inside the component output payload; no shared interface changes are required in this repo.

## Decisions Applied

- Runner is authoritative for pack graph existence/policy validation and actual dispatch.
- `pack2flow` validates input syntax/presence only.
- Success contract is namespaced jump intent marker (`greentic_control.action = "jump"`).
- Legacy `status/jumped_to` success JSON is removed from the runner-consumed path.
- `target.node`:
  - missing: allowed
  - empty/whitespace: `invalid_input.node_empty`
  - invalid identifier chars: `invalid_input.node_invalid`
- Redirect counters are runner-owned; component forwards `max_redirects` only.

## Migration Delta (JSON -> Jump)

- Previous success shape:
  - `status = "ok"`
  - `jumped_to` / `control`
- New success shape:
  - `greentic_control = { v: 1, action: "jump", target{flow,node?}, params, hints, max_redirects, reason }`
- Error shape remains:
  - `status = "error"`
  - `error.code`, `error.text`

## Scope Boundaries

- This repo owns unit tests, schemas, wasm build, and fixtures.
- Runner-host integration harness is owned by `greentic-runner` (PR-RUN-JUMP-02).
