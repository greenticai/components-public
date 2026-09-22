# webhook-extension

A Greentic Designer **design** extension that ships the canonical webhook trigger nodeType.

- id: `greentic.webhook`
- version: `1.3.0`
- contract: `greentic:extension-design@0.1.0`
- trigger contract: **v1** (`greentic.triggers.v1`)

## What it does

A webhook trigger starts a flow from outside. Under trigger contract v1 the
operator does not choose a URL: greentic-start serves every declared trigger at
`<deployment-prefix>/trigger/<trigger_id>`, verifies the call before the flow
runs, and enters the flow at the node AFTER the trigger.

This extension ships the node type and its JSON Schema, plus the design-time
tools the designer LLM calls while an operator configures one. The runtime half
lives in greentic-start (`src/triggers/`); the contract both sides build
against is `docs/trigger-contract-v1.md` in greentic-designer.

Two properties of that contract shape everything here, and both are checked by
tests rather than left to review:

- **A `*_ref` field carries a secret NAME (`<provider>/<key>`), never a value.**
  `validate_webhook_config` refuses a pasted credential without echoing it, and
  `infer_auth_from_curl` never copies the token out of the sample curl.
- **Verification is required.** `verify.scheme: "none"` exists so that
  accepting unverified calls is something an operator writes down on purpose,
  not something they reach by leaving a field blank. A verification failure
  never runs the flow — there is no `rejected` branch to route from, which is
  why the node has a single `default` output port.

### Why the trigger kind is declared inside `config_schema`

The designer decides a node emits a trigger declaration by reading
`x-trigger-kind` at the root of this node type's `config_schema`. It is NOT a
`contributions.trigger_kinds` key: `greentic_extension_sdk_contract`'s
`Contributions` and `NodeType` are both `deny_unknown_fields`, so a new key
there makes every designer refuse the whole describe and unload the extension.
`config_schema` is a free-form schema carried as a string, so an `x-` keyword
inside it is invisible to the typed reader and ignored by every schema
validator. `src/describe_tests.rs` pins both halves.

Split out of `platform-extension` per [`docs/superpowers/specs/2026-05-06-webhook-extension-split-design.md`](../../docs/superpowers/specs/2026-05-06-webhook-extension-split-design.md).

## Design-time tools

- `validate_webhook_config` — validate a trigger config block against contract
  v1 and return diagnostics; the pre-contract `method` / `path` / `auth` shape
  is reported with the fix rather than silently accepted
- `suggest_path` — slugify an intent into a `trigger_id` and the route the
  runtime mounts it at
- `infer_auth_from_curl` — derive the `verify` block from a sample curl; a
  timestamped scheme (Slack, Stripe) is named as unsupported rather than guessed
- `suggest_verification` — the verify / challenge / idempotency block for a
  known sender (Meta family, GitHub, or a bearer preset)

## Build

```bash
bash crates/webhook-extension/build.sh
ls -lh crates/webhook-extension/dist/
```

## Publish

Store publish via CI (mirrors `http-extension`):
1. Bump `version` in both `describe.json` and `Cargo.toml` (a test fails if they disagree)
2. Commit + push to main
3. Tag: `git tag webhook-ext-v<version> && git push origin webhook-ext-v<version>`
4. The `publish-webhook-extension` workflow posts the `.gtxpack` to the Store

## Layout

- `describe.json`      — extension manifest with the `trigger` nodeType + inline JSON Schema
- `src/lib.rs`         — WASM guest exports (tool dispatch; prompting / validation / knowledge are no-op stubs)
- `src/tools/`         — the four design-time tools
- `src/describe_tests.rs` — what `describe.json` must keep saying
- `wit/`               — WIT contract
- `i18n/`              — locale catalogs
