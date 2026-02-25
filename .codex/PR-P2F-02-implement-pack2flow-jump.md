# PR-P2F-02: Implement pack2flow proposing Jump outcome (Option A) — v1
Date: 2026-02-24
Repo: component-pack2flow

## Objective
Implement `pack2flow` as a reusable in-pack **jump proposer**:
- It makes **no routing decisions**.
- It returns a structured **Jump** outcome to the runner-host (Option A).

## Inputs (v1)
Parse CBOR map:
- `target.flow` (required)
- `target.node` (optional)
- `params` (optional map) — defaults merged into payload
- `hints` (optional map) — defaults merged into hints
- `max_redirects` (optional int; default 3)
- `reason` (optional string)

## Merge precedence
Caller wins:
- merged_payload = merge(params_defaults, current_payload) with current overriding
- merged_hints   = merge(hints_defaults, current_hints) with current overriding

## Validation (component-level; runner re-validates)
- missing flow → `missing_flow`
- if node provided but empty/invalid → `invalid_input` (optional)
Note: existence validation is performed by runner as the authority; component does not need pack graph access.

## Output (Option A)
Return a structured outcome:
- `Outcome::Jump { flow, node, payload: merged_payload, hints: merged_hints, max_redirects, reason }`
On parsing/validation error:
- return standard component error payload with:
  - `{"status":"error","error":{"code":"missing_flow","text":"..."}}`

## Tests
1) Unit tests:
   - parse target.flow required
   - parse optional node
   - params/hints merge precedence
2) Integration fixture with runner-host (as available in your harness):
   - Flow A uses pack2flow to jump to Flow B; B returns marker.
3) Loop fixture:
   - A jumps to A with max_redirects=3; runner returns redirect_limit error

## Docs
- README usage pattern:
  - local_router (decide) → pack2flow (jump)
  - pack2flow is deterministic; runner validates/applies jump

## Acceptance criteria
- pack2flow builds to wasm32-wasip2 and returns Jump outcome.
- Works with runner-host PR-RUN-JUMP-02.
- No dependencies on greentic-interfaces/qa/providers.
