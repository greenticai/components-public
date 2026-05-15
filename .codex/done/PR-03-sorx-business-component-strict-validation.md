# PR 03 - Add strict validation and drift handling to `component-sorx-business`

## Repository

Current checkout: `greentic-ai-org/components-public`

## Objective

Make `component-sorx-business` fail safely before invoking side-effectful Sorx actions.

This PR depends on PR 01 and PR 02.

## Current repo facts to honor

- Keep validation code inside `crates/component-sorx-business`; do not introduce a shared crate unless another Greentic crate already provides the exact abstraction.
- Keep native unit tests focused on pure request building, validation, and response normalization.
- Keep the wasm HTTP call path behind the same host HTTP pattern used by `component-http`.

## New Behavior

Before `invoke_locked_action` sends `POST /v1/sorx/business-actions/{id}/invoke`, the component should:

1. validate `action_ref.id`
2. validate `action_ref.version`
3. validate `action_ref.contract_hash`
4. fetch action metadata with `get_business_action_schema` only when needed for validation and not already supplied through a documented input/config test seam
5. compare fetched or supplied action metadata hash with `action_ref.contract_hash`
6. validate `values` against the action input schema if metadata includes a JSON Schema
7. reject unknown fields when that JSON Schema says `additionalProperties: false`
8. require `options.idempotency_key` when action metadata says idempotency is required
9. support `options.dry_run_first` by calling the dry-run endpoint before invoke and aborting invoke if dry run fails

Do not try to reimplement Sorx policy, approvals, provider binding, or execution logic locally.

## Input Extension

Extend `options` to:

```json
{
  "options": {
    "idempotency_key": "...",
    "dry_run_first": true,
    "fail_on_warning": false,
    "require_explanation": true
  }
}
```

If this PR adds a way to pass action metadata directly for testing/offline validation, keep it clearly separate from the stable public operation input shape and document it as an implementation detail.

## Drift Behavior

If local validation or Sorx reports:

```text
contract_hash_mismatch
version_mismatch
```

normalize the error to:

```json
{
  "ok": false,
  "error": {
    "code": "action_contract_drift",
    "message": "The action contract changed. Revalidate the flow node."
  },
  "expected": {},
  "actual": {}
}
```

Preserve useful Sorx status/details under a `sorx` or `details` object without changing the stable top-level error code.

## JSON Schema Scope

Implement only the subset needed by this PR unless an existing dependency in the workspace already provides JSON Schema validation:

- object type
- `required`
- primitive type checks for string, number/integer, boolean, object, array
- `additionalProperties: false`

If broader JSON Schema validation is needed, add a focused dependency only after confirming it is compatible with wasm32-wasip2.

## Tests

Add tests for:

- missing action ID rejected
- missing version rejected
- missing hash rejected
- malformed hash rejected
- local hash mismatch normalized as `action_contract_drift`
- Sorx hash/version mismatch normalized as `action_contract_drift`
- schema validation rejects a missing required value
- schema validation rejects unknown value fields when `additionalProperties: false`
- required idempotency key failure
- dry-run-first failure prevents invoke request construction
- Sorx server-side validation/policy error is propagated without pretending success

## Docs

Update `crates/component-sorx-business/README.md` with:

- why contract hashes are required
- how flow/node lock metadata should provide them
- what to do when drift is detected
- what local validation does and does not guarantee

## Acceptance Criteria

```bash
cargo test --workspace
cargo build --target wasm32-wasip2 --release
```
