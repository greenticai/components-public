# PR 04 - Add generic entity and evidence query operations

## Repository

Current checkout: `greentic-ai-org/components-public`

## Objective

Extend `component-sorx-business` with read-only query operations that remain explicit, generic, and schema-guided.

This PR depends on PR 01 and should build on PR 03 validation helpers where they apply.

## Current repo facts to honor

- Add operations to `crates/component-sorx-business/component.manifest.json` and Rust `describe()` metadata.
- Keep runtime operations in the component crate. Add Designer node types only through a design-extension crate if this PR explicitly introduces one.
- Do not add feature-gated stubs for endpoints. Implement request builders and host HTTP calls the same way as the action operations, with native tests using pure request/response seams.

## New Operations

Add:

```text
query_business_entity
query_business_evidence
explain_business_action_mapping
```

These are not fuzzy runtime intent operations. Inputs must use explicit concept/entity/action/query references.

## `query_business_entity`

Input schema:

```json
{
  "type": "object",
  "required": ["concept", "selector"],
  "properties": {
    "concept": { "type": "string", "minLength": 1 },
    "selector": {
      "type": "object",
      "required": ["kind"],
      "properties": {
        "kind": { "type": "string", "minLength": 1 },
        "fields": { "type": "object", "additionalProperties": true }
      },
      "additionalProperties": true
    },
    "options": {
      "type": "object",
      "properties": {
        "limit": { "type": "integer", "minimum": 1, "default": 5 }
      },
      "additionalProperties": false
    }
  },
  "additionalProperties": false
}
```

Map to:

```http
POST /v1/sorx/entities/query
```

The component treats concept names as opaque strings.

## `query_business_evidence`

Input schema:

```json
{
  "type": "object",
  "required": ["scope"],
  "properties": {
    "scope": {
      "type": "object",
      "properties": {
        "root_entities": {
          "type": "array",
          "items": {
            "type": "object",
            "required": ["entity_type", "entity_id"],
            "properties": {
              "entity_type": { "type": "string", "minLength": 1 },
              "entity_id": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
          }
        },
        "include_related": {
          "type": "array",
          "items": { "type": "object", "additionalProperties": true }
        }
      },
      "additionalProperties": true
    },
    "query": { "type": "string" },
    "limit": { "type": "integer", "minimum": 1, "default": 5 }
  },
  "additionalProperties": false
}
```

Map to:

```http
POST /v1/sorx/evidence/query
```

## `explain_business_action_mapping`

Input schema should reuse the `action_ref` + `values` shape from PR 02:

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
    "values": { "type": "object", "additionalProperties": true }
  },
  "additionalProperties": false
}
```

Map to:

```http
POST /v1/sorx/business-actions/{id}/explain
```

The operation returns how values map to the canonical Sorx payload and must not perform side effects.

## Output Normalization

Use the same success/error envelope style as PR 01:

```json
{
  "ok": true,
  "result": {},
  "sorx": {
    "status": 200
  }
}
```

## Tests

Add tests for:

- all three operations appear in manifest and Rust describe metadata
- entity query request shape and endpoint
- evidence query request shape and endpoint
- action mapping explain request shape and endpoint
- concept/entity names are treated as opaque strings with no domain-specific allowlist
- Sorx success and error responses are normalized
- operations are read-only by construction: only POST query/explain endpoints, never `/invoke`

## Docs

Update `crates/component-sorx-business/README.md` with examples for:

- read-only entity query
- evidence query
- action mapping explanation

Use neutral placeholders such as `concept_name`, `entity_type`, and `entity_id` in canonical docs. Domain examples may appear only in a clearly labeled example section.

## Acceptance Criteria

```bash
cargo test --workspace
cargo build --target wasm32-wasip2 --release
```
