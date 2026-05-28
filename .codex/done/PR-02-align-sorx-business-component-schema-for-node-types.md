# PR 02 - Align Sorx action schemas for Designer node types

## Repository

Current checkout: `greentic-ai-org/components-public`

## Objective

Align `component-sorx-business` operation schemas with the shape that Greentic Designer node types can generate and bind to.

This PR depends on PR 01 adding `crates/component-sorx-business`.

## Current repo facts to honor

- Runtime component schemas live in `component.manifest.json` and Rust `describe()` metadata.
- Designer node types are contributed by design-extension `describe.json` files under `contributions.nodeTypes`.
- Existing design-extension descriptors use snake_case `config_schema`, not `configSchema`.
- Runtime component manifests do not by themselves register Designer node types.

## Required Runtime Operation

Focus on:

```text
invoke_locked_action
```

The operation input schema must separate the locked action reference from the action-specific values:

```json
{
  "type": "object",
  "required": ["action_ref", "values"],
  "properties": {
    "action_ref": {
      "type": "object",
      "required": ["id", "version", "contract_hash"],
      "properties": {
        "id": { "type": "string", "minLength": 1 },
        "version": { "type": "string", "minLength": 1 },
        "contract_hash": {
          "type": "string",
          "pattern": "^sha256:[a-f0-9]{64}$"
        }
      },
      "additionalProperties": false
    },
    "values": {
      "type": "object",
      "additionalProperties": true
    },
    "options": {
      "type": "object",
      "properties": {
        "idempotency_key": { "type": "string" },
        "require_explanation": { "type": "boolean", "default": true },
        "dry_run_first": { "type": "boolean", "default": false }
      },
      "additionalProperties": false
    }
  },
  "additionalProperties": false
}
```

Keep `dry_run_locked_action` compatible with the same `action_ref` + `values` + `options` shape.

## Designer Compatibility

If this PR also adds a Sorx design extension, add it as a separate extension crate, for example:

```text
crates/sorx-business-extension
```

That extension may contribute `contributions.nodeTypes` entries whose `config_schema` pins `action_ref` with JSON Schema `const` and exposes action-specific inputs under `values`.

Do not place Designer-only node type descriptors in `component-sorx-business/component.manifest.json`.

## Validation

The runtime component should validate:

- `action_ref.id` is a non-empty string
- `action_ref.version` is a non-empty string
- `action_ref.contract_hash` matches `^sha256:[a-f0-9]{64}$`
- `values` exists and is an object
- `options` is absent or an object with only known fields

Action-specific validation stays with Sorx in this PR. Local schema validation belongs in PR 02.

## Tests

Add tests for:

- `invoke_locked_action` input schema includes `action_ref`, `values`, and `options`
- `dry_run_locked_action` accepts the same action input shape
- invalid hash is rejected before any Sorx request is built
- generated node-shaped input can be accepted by the runtime component
- no domain-specific fields appear in the component schemas
- manifest operation schema and Rust describe metadata stay consistent

If a design extension is included, add tests that its `describe.json` parses and uses `contributions.nodeTypes[].config_schema`.

## Docs

Update `crates/component-sorx-business/README.md` with the runtime input contract.

Add Designer node type examples only if this PR includes the design extension crate.

## Acceptance Criteria

```bash
cargo test --workspace
cargo build --target wasm32-wasip2 --release
```
