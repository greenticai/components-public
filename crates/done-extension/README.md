# greentic.done

Design extension for Done @ Greentic: ask a person to do something and react
when they answer. Spec: `docs/superpowers/specs/2026-10-29-greentic-done-extension-design.md`
in greentic-designer (sections 3, 4, 7.5). Plan: T11 of the Phase 1 plan.

## What it ships

| Piece | Where |
|---|---|
| Node `done.create_request` | `describe.json`; runs `component-done` op `done_api`, `action: create_request` |
| Node `done.read_request` | same, `action: read_request` |
| Trigger `done.event` | `describe.json`; a webhook trigger (`x-trigger-kind: webhook` inside `config_schema`) pre-filled for HMAC-SHA256 on `X-Done-Signature` |
| Tool `done_api` (agentic worker) | `src/tools.rs`; carries the secrets `done/api_key`, `done/base_url`, `done/webhook_secret` |
| Component `done-ops` | `crates/component-done` |
| Template "Ask and react" | `templates/ask-and-react/` (two recipes) |

## Rules worth keeping

- No new `contributions` key. `Contributions`, `NodeType` and `Tool` are
  `deny_unknown_fields`; `describe_tests.rs` feeds the file to the real typed reader.
- Both nodes carry `operation: "done_api"` because the designer joins a node
  type to its secrets by naming the tool. The behaviour is chosen by the
  constant `action` in each node's config.
- A node's config avoids `operation`, `input`, `config`, `payload`, `envelope`.
- Failure is a value: `{ok:false, error:{code, message}}`.
- A model cannot name the requester or the credential; the Core attributes the
  request to the API key's owner.

## Build and test

```bash
cargo test -p component-done -p done-extension
crates/done-extension/build.sh        # dev .gtxpack in dist/
```

A release needs `component-done` published to OCI first; see `build.sh`.
