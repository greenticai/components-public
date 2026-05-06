# webhook-extension

A Greentic Designer **design** extension that ships the canonical webhook trigger nodeType.

- id: `greentic.webhook`
- version: `0.1.0`
- contract: `greentic:extension-design@0.1.0`

## What it does

Webhook is operator-side ingress — the runtime exposes an HTTP listener, validates auth, and kicks off flow execution when an inbound request matches the configured path. This extension carries no WASM logic; it only ships the descriptor + JSON Schema the designer needs to render the inspector form.

Split out of `platform-extension` per [`docs/superpowers/specs/2026-05-06-webhook-extension-split-design.md`](../../docs/superpowers/specs/2026-05-06-webhook-extension-split-design.md).

## Build

```bash
bash crates/webhook-extension/build.sh
ls -lh crates/webhook-extension/dist/
```

## Publish

Store publish via CI (mirrors `http-extension`):
1. Bump `version` in both `describe.json` and `Cargo.toml`
2. Commit + push to main
3. Tag: `git tag webhook-ext-v<version> && git push origin webhook-ext-v<version>`
4. The `publish-webhook-extension` workflow posts the `.gtxpack` to the Store

## Layout

- `describe.json` — extension manifest with the `trigger` nodeType + inline JSON Schema
- `src/lib.rs`    — WASM guest exports (no-op stubs; tools deferred to a follow-up release)
- `wit/`          — WIT contract
- `i18n/`         — locale catalogs

## Future work

Design-time tools to add in a follow-up:
- `validate_webhook_config` — JSON Schema + semantic validation
- `suggest_path` — generate a sensible URL path from flow context
- `infer_auth_from_curl` — parse a curl example, extract auth + signature scheme
