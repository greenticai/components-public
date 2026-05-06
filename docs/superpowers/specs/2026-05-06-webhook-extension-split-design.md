# Webhook Extension Split — Design Spec

> **Status:** draft
> **Author:** Bima
> **Date:** 2026-05-06
> **Targets:** `components-public`, `greentic-designer`, `greentic-start`/`greentic-runner`

## Problem

`platform-extension` currently ships three node types — `start`, `trigger`, `llm` — under one umbrella because we needed a bootstrap fallback when the designer's node-registry was empty. That umbrella has outgrown its purpose:

- **`trigger`** is a webhook ingress — runtime-side, listens on an HTTP port, validates auth, kicks off flow execution. It's a *capability* of the operator, not WASM logic.
- **`http`** (in `http-extension`) is an egress component — WASM-side, receives input from the flow, calls outbound HTTP via host imports, returns response.
- **`start`** is a designer-only structural primitive — every flow has one, no runtime work involved.
- **`llm`** is a generic AI step that ships in `platform-extension` historically but has overlap with `greentic-llm-extensions/llm-openai`.

These four have nothing in common architecturally. Bundling them violates the rule that a `.gtxpack` should describe one capability profile. Concretely, `webhook trigger` belongs to a *different runtime kind* than `http egress component` — they should live in separate extensions so capabilities, manifests, and lifecycle stay coherent.

## Proposal

Split `trigger` out of `platform-extension` into its own design extension `greentic.webhook`. Leave `start` in `platform-extension` (it's the only true bootstrap primitive). Defer `llm` extraction to a follow-up.

### Boundaries after split

| Extension | type_id | Side | Notes |
|---|---|---|---|
| `greentic.platform-bootstrap` v0.2.0 | `start` | designer-only | Structural primitive. No runtime work. |
| `greentic.webhook` v0.1.0 (NEW) | `trigger` | runtime ingress | HTTP listener config. No WASM logic. |
| `greentic.http` v0.1.3 (existing) | `http` | WASM egress | Outbound HTTP via host imports. |
| `greentic.llm-openai` v0.1.0 (existing) | `llm-openai` | WASM compute | OpenAI-compatible LLM. |

### `webhook-extension` shape

```
crates/webhook-extension/
├── Cargo.toml                # cdylib, edition 2024, wasm32-wasip2
├── describe.json             # kind=DesignExtension, ships trigger nodeType
├── build.sh                  # mirrors http-extension's pattern
├── prompts/
│   ├── rules.md              # design-time prompt fragment
│   └── examples.md           # webhook URL / auth examples
├── schemas/
│   └── webhook-trigger-v1.json  # JSON Schema for config
├── i18n/{en,id,...}.json     # locale catalogs
├── src/lib.rs                # WIT bindings + tools impl
├── wit/world.wit             # design-extension world
└── tests/                    # native + integration smoke
```

`describe.json` skeleton:

```json
{
  "kind": "DesignExtension",
  "metadata": { "id": "greentic.webhook", "version": "0.1.0", ... },
  "capabilities": {
    "offered": [
      { "id": "greentic:webhook/trigger-spec", "version": "1.0.0" },
      { "id": "greentic:webhook/auth-helper", "version": "1.0.0" }
    ]
  },
  "runtime": {
    "component": "extension.wasm",
    "memoryLimitMB": 16,
    "permissions": { "network": [], "secrets": ["*"], "callExtensionKinds": [] }
  },
  "contributions": {
    "nodeTypes": [
      {
        "type_id": "trigger",
        "label": "Webhook Trigger",
        "category": "trigger",
        "icon": "webhook",
        "color": "#0ea5e9",
        "complexity": "simple",
        "config_schema": {
          "type": "object",
          "required": ["method", "path"],
          "properties": {
            "method": { "type": "string", "enum": ["GET", "POST", "PUT", "PATCH"], "default": "POST" },
            "path": { "type": "string", "description": "Webhook URL path (e.g. /webhook/orders)" },
            "auth": {
              "type": "object",
              "properties": {
                "type": { "type": "string", "enum": ["none", "bearer", "hmac", "basic"], "default": "none" },
                "secret_ref": { "type": "string", "description": "Secret name for the auth credential" }
              }
            },
            "signature_validation": {
              "type": "object",
              "properties": {
                "header": { "type": "string", "examples": ["X-Hub-Signature-256"] },
                "algorithm": { "type": "string", "enum": ["hmac-sha256", "hmac-sha1"] },
                "secret_ref": { "type": "string" }
              }
            },
            "allowed_sources": {
              "type": "array",
              "items": { "type": "string", "format": "ipv4-cidr" },
              "description": "Optional CIDR allowlist for source IPs"
            }
          }
        },
        "output_ports": [
          { "name": "default", "label": "Triggered" },
          { "name": "rejected", "label": "Auth failed" }
        ]
      }
    ],
    "tools": [
      { "name": "validate_webhook_config", "export": "greentic:extension-design/tools.invoke-tool" },
      { "name": "suggest_path", "export": "greentic:extension-design/tools.invoke-tool" },
      { "name": "infer_auth_from_curl", "export": "greentic:extension-design/tools.invoke-tool" }
    ]
  }
}
```

### `platform-extension` strip

Bump to `0.2.0` (breaking — the `trigger` nodeType disappears). `describe.json.contributions.nodeTypes` retains only `start`. `llm` stays for now and can move out in a separate PR.

### Runtime side (`greentic-start`)

The runtime currently dispatches based on `type_id` from the flow YAML, not extension provenance. It looks up `node.type === 'trigger'` to wire HTTP ingress.

After the split, `type_id: 'trigger'` still resolves the same — both old (`platform-extension` v0.1.0 historical) and new (`webhook-extension` v0.1.0) flows use the same string. **Runtime needs no code change.** Existing flows on disk continue to work.

If we want to rename to `webhook` later for clarity, that's a separate cutover — keep the alias `trigger → webhook` in flow loader for one minor cycle.

### Designer bundled

`bundled/manifest.json` updates:

- `greentic.platform-bootstrap`: `0.1.0 → 0.2.0`
- `greentic.webhook`: NEW entry, `0.1.0`

Refresh script (`scripts/refresh-bundled.sh`) handles both once the artifacts are on the store. No designer code change needed; the node-registry already discovers extensions by introspecting `describe.json` files at boot.

## Migration

| Scenario | Behavior after this PR |
|---|---|
| Existing flow YAML with `type: trigger` | ✅ Resolves via `webhook-extension`'s descriptor (same type_id). No-op for users. |
| User on designer < this version | Continues to use bundled platform-bootstrap 0.1.0 (which still has trigger). No fallout until they upgrade. |
| User upgrades designer | Bundled fallback unpacks both `platform-bootstrap-0.2.0` and `greentic.webhook-0.1.0`. Designer node-registry merges nodeTypes from both. |
| User has `greentic.platform-bootstrap-0.1.0/` already in `~/.greentic/extensions/design/` | gtdx unpack policy: bundled fallback only runs when target dir absent. So 0.1.0 stays — but webhook 0.1.0 unpacks fresh because that dir doesn't exist yet. Both `trigger` types appear in registry. Designer prefers higher version → uses webhook-extension's. Net effect: works. |

No data migration required.

## Out of scope (followups)

- **`llm` move** to `greentic.llm-generic` (sister of `llm-openai`). Same shape change but separate spec because it has more downstream consumers.
- **Provider-specific webhook extensions**: `provider.slack-webhook`, `provider.stripe-webhook`, `provider.github-webhook` — each can declare its own pre-validated signature schemes, source IP allowlists, payload shapes. Inherits from `greentic.webhook` capability contract.
- **Renaming `trigger` to `webhook`**: cosmetic, deferred. Keep `trigger` as alias.

## Implementation plan

### `components-public` (1 PR)

1. Scaffold `crates/webhook-extension/` mirroring `http-extension/` shape
2. Write `describe.json` with full nodeType + tools + capabilities
3. Write `schemas/webhook-trigger-v1.json` (JSON Schema source for the inline schema in describe.json)
4. Implement design-time tools in `src/lib.rs`: `validate_webhook_config`, `suggest_path`, `infer_auth_from_curl`
5. Strip `trigger` from `platform-extension/describe.json`, bump to 0.2.0
6. Add `.github/workflows/publish-webhook-extension.yml` (mirror http publish)
7. Update tag-on-version-bump action to recognize `webhook-ext-v*` pattern
8. Update CHANGELOG / README

### `greentic-designer` (1 PR)

1. Bump `platform-bootstrap` to 0.2.0 in `bundled/manifest.json` + refresh `.gtxpack`
2. Add `greentic.webhook-0.1.0` entry + `.gtxpack`
3. Sanity test: spawn a webhook trigger node in the editor → inspector form renders with the new schema fields

### `greentic-start` / runtime (no PR if backward-compat path holds)

Verify HTTP ingress dispatcher still resolves `type_id: 'trigger'` correctly. If yes, no code change. If no, file follow-up bug.

## Acceptance criteria

- [ ] `webhook-extension` `.gtxpack` published to store as `ext-v0.1.0` (or `webhook-ext-v0.1.0` per tag scheme)
- [ ] `platform-extension` `.gtxpack` published as `platform-ext-v0.2.0` without `trigger` nodeType
- [ ] Designer's bundled manifest references both
- [ ] In a fresh designer install, right-clicking the canvas shows "Webhook Trigger" under the trigger category, sourced from `greentic.webhook`
- [ ] Inspector form for the webhook node renders all 5 schema fields (method, path, auth, signature_validation, allowed_sources)
- [ ] An existing flow YAML with `type: trigger` loads cleanly and the editor resolves the renderer correctly
- [ ] Runtime smoke test: deploy a flow with the new webhook trigger, hit the configured path, verify execution kicks off

## Risks / open questions

1. **Two extensions ship the same `type_id: 'trigger'`** when the designer transitions (old platform-bootstrap still installed, new webhook-extension just unpacked). Need to confirm node-registry's deduplication picks the higher-version one. If it picks alphabetically by extension_id, `greentic.platform-bootstrap` wins over `greentic.webhook` — wrong direction. May need explicit precedence rule.

2. **Tag-on-version-bump workflow** currently keys off `Cargo.toml` version at the workspace root. Per-crate version tagging needs the workflow to handle multiple crates. Either:
   - Add per-crate Cargo.toml watching to the existing workflow
   - Or use a separate workflow per crate (mirrors http-extension's existing setup)

3. **Capability scope of webhook**: do we want `permissions.network = []` (no outbound) since webhook is purely ingress? Confirms the architectural separation. Yes — keep network empty.

4. **Naming**: `greentic.webhook` vs `greentic.webhook-trigger`. Pick one and stick with it. `greentic.webhook` reads cleaner; `webhook-trigger` is more explicit. Recommendation: `greentic.webhook`.
