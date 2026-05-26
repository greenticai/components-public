# component-sorx-business

`component-sorx-business` is a generic runtime component for calling locked Sorx agent endpoint actions from Greentic flows.

Sorx remains authoritative for business semantics: agent endpoint catalogs, schemas, policy, approvals, provider bindings, execution, and audit. This component validates the generic envelope, discovers Sorx tools/routes, invokes the matching runtime endpoint, and normalizes responses.

## Configuration

Pass configuration under `config` in the invocation payload until the runtime provides a separate typed config channel:

```json
{
  "config": {
    "sorx_base_url": "https://sorx.example.com",
    "auth": {
      "kind": "bearer_secret_ref",
      "secret_ref": "SORX_TOKEN"
    },
    "timeout_ms": 30000,
    "strict_tls": true
  }
}
```

`auth.kind` may be `none` or `bearer_secret_ref`. Secret resolution uses the Greentic secrets host interface on wasm and an environment variable with the same name in native tests.

## Locked Action Input

`invoke_locked_action`, `dry_run_locked_action`, and `explain_business_action_mapping` use this generic shape:

```json
{
  "action_ref": {
    "id": "action_id",
    "version": "0.1.0",
    "contract_hash": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
  },
  "values": {},
  "options": {
    "idempotency_key": "optional",
    "require_explanation": true,
    "dry_run_first": false,
    "fail_on_warning": false
  }
}
```

Contract hashes are required so a flow node can prove which action contract it was generated and reviewed against. Sorx currently exposes agent endpoints through `/v1/sorx/tools` and `/v1/sorx/routes`; the component validates contract hashes against local action metadata when that metadata is present.

Local validation checks the generic envelope and, when action metadata with an input schema is available, a small JSON Schema subset: object type, required fields, primitive value types, arrays, and `additionalProperties: false`. Sorx still performs authoritative validation.

## Operations

| Operation | Sorx endpoint |
| --- | --- |
| `list_business_actions` | `GET /v1/sorx/tools` |
| `get_business_action_schema` | `GET /v1/sorx/tools` and local filter by action id |
| `dry_run_locked_action` | `GET /v1/sorx/tools` and local validation/explain |
| `invoke_locked_action` | `GET /v1/sorx/routes`, then invoke the matched route |
| `explain_business_action_mapping` | `GET /v1/sorx/tools` and local validation/explain |

## Output

Success responses use:

```json
{
  "ok": true,
  "result": {},
  "sorx": {
    "status": 200
  }
}
```

Errors use:

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
