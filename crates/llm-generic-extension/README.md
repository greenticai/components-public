# llm-generic-extension

A Greentic Designer **design** extension that ships the canonical generic `llm` nodeType.

- id: `greentic.llm-generic`
- version: `0.1.0`
- contract: `greentic:extension-design@0.1.0`

## What it does

The generic `llm` nodeType is a fallback for prompt-driven AI steps that aren't pinned to a specific provider. Provider-specific extensions (e.g. `greentic.llm-openai`) ship richer nodeTypes that implement the actual completion call — this generic node is a placeholder the planner can target when the provider is unknown or pluggable.

Pure metadata extension: WASM exports are no-op stubs. The descriptor + JSON Schema in `describe.json` are what the designer reads to render the inspector form.

Split out of `platform-extension` per the same rationale as `webhook-extension` ([`docs/superpowers/specs/2026-05-06-webhook-extension-split-design.md`](../../docs/superpowers/specs/2026-05-06-webhook-extension-split-design.md)) — different node types belong in different extensions so capabilities, manifests, and lifecycle stay coherent.

## Build

```bash
bash crates/llm-generic-extension/build.sh
ls -lh crates/llm-generic-extension/dist/
```

## Publish

Store publish via CI (mirrors `webhook-extension`):
1. Bump `version` in both `describe.json` and `Cargo.toml`
2. Commit + push to main
3. Tag: `git tag llm-generic-ext-v<version> && git push origin llm-generic-ext-v<version>`
4. The `publish-llm-generic-extension` workflow posts the `.gtxpack` to the Store

## Layout

- `describe.json` — extension manifest with the `llm` nodeType + inline JSON Schema
- `src/lib.rs`    — WASM guest exports (no-op stubs; tools deferred to a follow-up release)
- `wit/`          — WIT contract
- `i18n/`         — locale catalogs

## Future work

Design-time tools to add in a follow-up:
- `validate_llm_config` — JSON Schema + semantic validation (model name, temperature range)
- `suggest_model` — recommend a model based on the flow's other nodes / overall task type
- `render_prompt_preview` — interpolate `{{params.*}}` references against sample data and show the resolved prompt
