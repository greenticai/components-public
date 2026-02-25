# component-pack2flow

`pack2flow` is a deterministic transfer utility for Greentic packs.

Usage pattern:
- `local_router` decides destination
- `pack2flow` returns deterministic jump intent

## Guarantees (v1)

- Jump/replace semantics only (no call/return).
- Runner is authoritative for flow/node existence + policy checks.
- Component validates input syntax only and proposes jump intent.
- Redirect counting is runner-owned; component forwards `max_redirects`.

## Input Contract (JSON/CBOR payload)

- `target.flow` (string, required)
- `target.node` (string, optional)
  - missing: runner resolves default
  - empty/whitespace: `invalid_input.node_empty`
  - invalid identifier chars: `invalid_input.node_invalid`
- `params` (map, optional): payload defaults
- `hints` (map, optional): routing-hints defaults
- `payload` (map, optional): current caller payload
- `routing_hints` (map, optional): current caller hints
- `max_redirects` (int, optional, default `3`)
- `reason` (string, optional)

## Merge Precedence

Caller data is authoritative.

- `merged_payload = shallow_merge(params, payload)` where `payload` wins
- `merged_hints = shallow_merge(hints, routing_hints)` where `routing_hints` wins

(v1 uses shallow merge; deep merge can be added later.)

## Output Contract

Success (`greentic_control` marker payload):

```json
{
  "greentic_control": {
    "v": 1,
    "action": "jump",
    "operation": "handle_message",
    "target": {
      "flow": "...",
      "node": "..."
    },
    "params": {},
    "hints": {},
    "max_redirects": 3,
    "reason": null
  }
}
```

Failure:

```json
{
  "status": "error",
  "error": {
    "code": "missing_flow",
    "text": "Missing required target.flow"
  }
}
```

Error codes implemented:
- `missing_flow`
- `invalid_input.node_empty`
- `invalid_input.flow_invalid`
- `invalid_input.node_invalid`
- `invalid_input`
- `jump_failed`

## Build/Test

```bash
cargo component build --release --target wasm32-wasip2
cargo test --workspace --all-targets
```
