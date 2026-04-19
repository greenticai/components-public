# Design: `greentic.http` DesignExtension

**Date**: 2026-04-19
**Status**: Draft — awaiting user approval
**Scope**: `components-public` workspace

## Purpose

Ship a Greentic Designer Extension (`greentic.http`) that teaches the
Designer's LLM to generate valid YGTc HTTP-call nodes from natural-language
intent, `curl` commands, or Adaptive Card submit contexts. The extension
publishes to the Greentic Store as a `.gtxpack`; users install it in
Designer via `gtdx install greentic.http`.

The extension is **design-time only**. The runtime HTTP node is the
existing `component-http` WASM (published to GHCR), which the generated
YGTc nodes reference via `oci://` URIs. Users who install the extension
do not get a runtime artifact from it — the runtime is pulled by
`greentic-runner` when the flow executes.

## Motivation

Current workflow for adding an HTTP call to a flow:

1. User reads `component-http` documentation
2. User hand-writes YAML stanza in `.ygtc`
3. User manually configures `secret:NAME` references, headers, body templates
4. User manually maps inputs from previous flow step (e.g. card submit output)

This is error-prone (invalid JSON templates, missing `Content-Type`,
unsafe bare tokens) and does not scale to non-developer users who work
through the Designer LLM.

The extension removes this friction: user describes intent, LLM produces
a validated YGTc node stanza with correct config, safe secret references,
and explicit input mapping.

## Architecture Overview

Three crates in `components-public/crates/`:

```
http-extension (NEW)   ──► extension.wasm → .gtxpack → Greentic Store
  │ DesignExtension         LLM tools + prompts + schemas
  ▼
http-core (NEW)        ──► pure-Rust lib (no WASM deps)
  ▲                       config, auth, URL, header, curl parse, node builder
  │
component-http (EXISTS) ──► component_http.wasm → GHCR (existing CI)
                            runtime node; refactored to use http-core
```

**Separation of concerns**:

- `http-core` — pure logic, deterministic, fully unit-testable on native target
- `component-http` — WIT guest bindings + WASM runtime I/O (network via host, secrets via host)
- `http-extension` — WIT DesignExtension exports; LLM-facing tools + prompts

**Deployment split**:

- Runtime: `ghcr.io/greenticai/component/component-http:<version>` (existing pipeline)
- Design: `greentic.http@<version>.gtxpack` in Greentic Store (new pipeline)

Independent versioning. Extension version tracks UX/prompt evolution;
runtime version tracks wire-format/WASI interface changes.

## Workspace Layout

```
components-public/crates/
├── http-core/                      # NEW — pure Rust lib (~400 lines total)
│   ├── Cargo.toml                  # no wasm deps
│   ├── src/
│   │   ├── lib.rs                  # public re-exports
│   │   ├── config.rs               # ComponentConfig + validators + apply_answers
│   │   ├── auth.rs                 # AuthType enum + header builders
│   │   ├── node.rs                 # YGTc node stanza types + NodeBuilder
│   │   ├── curl.rs                 # curl command parser
│   │   └── url.rs                  # URL validation + placeholder extraction
│   └── tests/                      # unit tests per module
│
├── component-http/                 # EXISTING — refactored slim (~400 lines)
│   ├── Cargo.toml                  # + http-core = { path = "../http-core" }
│   ├── Makefile                    # unchanged
│   ├── assets/i18n/                # unchanged
│   └── src/
│       ├── lib.rs                  # ~100 lines — Component struct + WIT export macro
│       ├── bindings.rs             # wit-bindgen generated
│       ├── qa.rs                   # ComponentQaSpec builder
│       ├── request.rs              # blocking HTTP op
│       └── stream.rs               # streaming HTTP op
│
└── http-extension/                 # NEW — DesignExtension .gtxpack source
    ├── Cargo.toml                  # deps: http-core, greentic-ext-contract (WIT), serde
    ├── describe.json               # Greentic Store manifest
    ├── build.sh                    # cargo component build → .gtxpack
    ├── src/
    │   ├── lib.rs                  # ~150 lines — Component struct + WIT exports
    │   ├── bindings.rs             # wit-bindgen generated
    │   └── tools/
    │       ├── mod.rs              # list_tools() + invoke_tool() dispatch
    │       ├── generate.rs         # generate_http_node
    │       ├── validate.rs         # validate_http_config (validation interface)
    │       ├── curl_import.rs      # curl_to_node
    │       ├── auth_suggest.rs     # suggest_auth
    │       └── card_submit.rs      # generate_from_card_submit
    ├── prompts/
    │   ├── rules.md                # LLM prompt: do/don't, defaults, security
    │   └── examples.md             # sample intent → node pairs
    ├── schemas/
    │   └── http-node-v1.json       # JSON Schema for YGTc HTTP node
    ├── i18n/
    │   └── en.json                 # English-only (programmatic surface)
    ├── wit/
    │   └── world.wit               # imports greentic:extension-design/*
    └── tests/
        └── tool_roundtrip.rs       # E2E per tool
```

**File size budget**: every file under 500 lines (matches `components-public` house rules).

## `describe.json` Manifest

```json
{
  "$schema": "https://store.greentic.ai/schemas/describe-v1.json",
  "apiVersion": "greentic.ai/v1",
  "kind": "DesignExtension",
  "metadata": {
    "id": "greentic.http",
    "name": "HTTP Client",
    "version": "0.1.0",
    "summary": "Generate and validate HTTP call nodes for Greentic flows",
    "description": "Teaches the Designer LLM to generate YGTc HTTP nodes from natural language, curl commands, or card submit contexts. Wraps component-http runtime — does not make HTTP calls itself.",
    "author": { "name": "Greentic", "email": "team@greentic.ai" },
    "license": "MIT",
    "repository": "https://github.com/greenticai/components-public",
    "keywords": ["http", "api", "rest", "curl", "design-extension"]
  },
  "engine": {
    "greenticDesigner": ">=0.6.0",
    "extRuntime": "^0.1.0"
  },
  "capabilities": {
    "offered": [
      { "id": "greentic:http/node-generator", "version": "1.0.0" },
      { "id": "greentic:http/validate",       "version": "1.0.0" },
      { "id": "greentic:http/curl-import",    "version": "1.0.0" }
    ],
    "required": []
  },
  "runtime": {
    "component": "extension.wasm",
    "memoryLimitMB": 32,
    "permissions": {
      "network": [],
      "secrets": [],
      "callExtensionKinds": []
    }
  },
  "contributions": {
    "schemas": ["schemas/http-node-v1.json"],
    "prompts": ["prompts/rules.md", "prompts/examples.md"],
    "tools": [
      { "name": "generate_http_node",         "export": "greentic:extension-design/tools.invoke-tool" },
      { "name": "validate_http_config",       "export": "greentic:extension-design/validation.validate-content" },
      { "name": "curl_to_node",               "export": "greentic:extension-design/tools.invoke-tool" },
      { "name": "suggest_auth",               "export": "greentic:extension-design/tools.invoke-tool" },
      { "name": "generate_from_card_submit",  "export": "greentic:extension-design/tools.invoke-tool" }
    ]
  }
}
```

**Permissions all empty**: extension is pure compute-in-process. No host
calls for network, secrets, or cross-extension. Security posture matches
the minimum-privilege principle.

**Memory limit 32 MB**: tighter than `greentic.adaptive-cards` (64 MB)
because there is no large embedded JSON schema — the HTTP node schema is
compact.

## LLM Tools

Five tools registered in `describe.json:contributions.tools`. Four
dispatch through the `tools.invoke-tool` WIT export; one
(`validate_http_config`) dispatches through `validation.validate-content`
so Designer's non-LLM content validation path reuses the same logic.
`list_tools()` returns all five definitions with input JSON Schemas for
LLM tool-call reasoning (mirrors the `greentic.adaptive-cards` pattern
where `validate_card` is both a listed tool and the validation export).

### 1. `generate_http_node(intent, context?)`

Generate a YGTc HTTP node stanza from a natural-language intent.

**Input**:
```json
{
  "intent": "POST to CRM /api/leads with JSON body, bearer auth",
  "context": {
    "base_url_hint": "https://crm.example.com",
    "secret_names": ["CRM_TOKEN", "SLACK_WEBHOOK"]
  }
}
```

**Output** (deterministic JSON, sorted keys):
```json
{
  "node_id": "post_to_crm",
  "component": "oci://ghcr.io/greenticai/component/component-http:0.1.0",
  "config": {
    "base_url": "https://crm.example.com",
    "auth_type": "bearer",
    "auth_token": "secret:CRM_TOKEN",
    "timeout_ms": 15000,
    "default_headers": { "Content-Type": "application/json" }
  },
  "inputs": {
    "method": "POST",
    "path": "/api/leads"
  },
  "rationale": "Chose bearer auth based on intent keyword; CRM_TOKEN picked from available secrets; default timeout 15s."
}
```

### 2. `validate_http_config(content_type, content_json)` — via `validation.validate-content`

Validate a generated node. Uses the standard validation interface
(content_type must be `"http-node"`).

**Output**:
```json
{
  "valid": false,
  "diagnostics": [
    { "severity": "error",   "code": "url:invalid-scheme",    "message": "base_url must be http:// or https://", "path": "config.base_url" },
    { "severity": "warning", "code": "auth:bare-token",       "message": "auth_token looks like raw token; use secret:NAME reference", "path": "config.auth_token" },
    { "severity": "warning", "code": "timeout:too-long",      "message": "timeout_ms 120000 exceeds recommended 60000", "path": "config.timeout_ms" }
  ]
}
```

Diagnostic codes are stable strings, documented in `prompts/rules.md` so
the LLM can reason about fixes.

### 3. `curl_to_node(curl_cmd, node_id?)`

Parse a raw `curl` command into a YGTc node.

**Input**:
```json
{
  "curl_cmd": "curl -X POST https://api.example.com/users -H 'Authorization: Bearer xxx' -H 'X-Team: qa' -d '{\"name\":\"alice\"}'",
  "node_id": "create_user"
}
```

**Behavior**:
- `-H 'Authorization: Bearer <token>'` → `auth_type: "bearer"` + rewrite token as `secret:HTTP_TOKEN` (warning to rename)
- Other `-H` flags → `default_headers` map
- `-d` body → preserved as body template string
- `-X` method → `inputs.method`
- Unknown flags (`-F`, `--data-urlencode`) → diagnostic `"curl:unsupported-flag"` in output

Parsing via `http-core::curl::parse_curl()`. Deterministic ordering of
headers (sorted by name).

### 4. `suggest_auth(api_description)`

Recommend auth configuration for a known or described API.

**Input**:
```json
{ "api_description": "GitHub REST API v3" }
```

**Output**:
```json
{
  "auth_type": "bearer",
  "auth_token": "secret:GITHUB_TOKEN",
  "api_key_header": null,
  "default_headers": { "Accept": "application/vnd.github+json" },
  "rationale": "GitHub REST v3 uses Personal Access Tokens via Bearer auth. 'Accept' header recommended for version pinning.",
  "confidence": "high"
}
```

Knowledge patterns documented in `prompts/examples.md`:
- REST w/ bearer PAT (GitHub, GitLab, DigitalOcean, …)
- API key header (OpenAI, Anthropic, Airtable, …)
- Basic auth (legacy intranet, some SaaS)
- OAuth2 → points to separate OAuth extension (not in scope here)
- Webhook-style (no auth, HMAC signature in custom header)

Unknown API → `auth_type: "unknown"` + `confidence: "low"` (LLM should
ask user follow-up).

### 5. `generate_from_card_submit(card_schema, api_intent, node_id?)`

Generate a YGTc HTTP node that maps Adaptive Card submit fields to an
API request body. The **primary use case** driving this design: user
submits card → HTTP call sends card data to external API.

**Input**:
```json
{
  "card_schema": { /* Adaptive Card JSON with Input.Text/Input.ChoiceSet elements */ },
  "api_intent": "POST to /api/tickets with form fields as JSON",
  "node_id": "submit_ticket"
}
```

**Behavior**:
1. Extract `Input.*` element IDs from card schema (e.g. `subject`, `priority`, `description`)
2. Build body template mapping each card field to a JSON path: `${submit.<field>}`
3. Generate full node with `inputs.body_template` containing the mapping
4. Emit warning for card fields not obviously matching API fields

**Output**:
```json
{
  "node_id": "submit_ticket",
  "component": "oci://ghcr.io/greenticai/component/component-http:0.1.0",
  "config": { "base_url": "...", "auth_type": "bearer", "auth_token": "secret:TICKETS_TOKEN" },
  "inputs": {
    "method": "POST",
    "path": "/api/tickets",
    "body_template": "{\"subject\":\"${submit.subject}\",\"priority\":\"${submit.priority}\",\"description\":\"${submit.description}\"}"
  },
  "mapping": {
    "card_to_body": {
      "submit.subject": "body.subject",
      "submit.priority": "body.priority",
      "submit.description": "body.description"
    },
    "unmapped_card_fields": ["submit.internal_note"]
  },
  "rationale": "..."
}
```

### Error Handling (all tools)

- `InvalidInput` — args JSON does not match tool input schema
- `Internal` — parse/serialize error (tools must not panic)
- Both use `types::ExtensionError` from `greentic-ext-contract`

All tool outputs are deterministic: same input produces byte-identical
output (sorted keys, stable formatting). This makes E2E tests reliable.

## `component-http` Refactor

### Moves to `http-core`

From current `component-http/src/lib.rs`:
- `ComponentConfig` struct + `default_*` functions → `http-core::config`
- Auth type constants + header builders → `http-core::auth`
- URL validation + placeholder extraction → `http-core::url`
- Header parsing + merge helpers → `http-core::config`
- `ApplyAnswersResult` + QA config merge logic → `http-core::config`

Total moved: ~450 lines of pure logic. `http-core` ends up ~400 lines
total with added tests.

### Stays in `component-http`

WIT binding glue + runtime I/O:
- `#[cfg(target_arch = "wasm32")]` imports (node, client, secrets_store, logger_api)
- `I18N_KEYS` array (metadata surface)
- `ComponentQaSpec` + `Question` construction (WIT types — must not leak to http-core)
- Actual `request`/`stream` op implementations
- Telemetry logger calls
- Secret store resolution

### `http-core` Public API

```rust
pub use config::{ComponentConfig, ConfigError, apply_answers};
pub use auth::{AuthType, AuthHeader, build_auth_header};
pub use url::{validate_url, extract_placeholders};
pub use node::{YgtcNode, NodeBuilder, ComponentRef};
pub use curl::{parse_curl, CurlParseError};
```

### Migration Order

1. Create empty `http-core/` crate with `Cargo.toml` + `lib.rs`
2. Move `ComponentConfig` + its tests → verify `cargo test -p http-core`
3. Move auth + URL helpers → re-verify
4. Add `http-core = { path = "../http-core" }` to `component-http/Cargo.toml`
5. Refactor `component-http/src/lib.rs` → import from `http-core`, split into 4 files
6. Run WASM tests: `cargo test -p component-http --target wasm32-wasip2`
7. Run existing gtest `tests/gtests/README/07_component-http_add_to_flow.gtest` — must pass unchanged (backward compat gate)

### Risks

- **WIT type leakage**: WIT-generated types (`Question`, `ComponentQaSpec`) must stay in `component-http`. Boundary: `http-core::config::ComponentConfig` is plain serde; `component-http::qa::build_spec()` translates to WIT types.
- **QA answers compat**: existing `setup.answers.json` in gtest must keep working without changes. Mitigation: `apply_answers` signature preserved verbatim.

### PR Split

- **PR #1**: refactor `component-http` + create `http-core`. No behavior change. Existing tests + gtest pass unchanged. Reversible.
- **PR #2**: create `http-extension` crate + publish pipeline. Builds on PR #1.

## Build & Publish Pipeline

### `http-extension/build.sh`

Mirrors the `adaptive-card-extension` pattern:

```bash
#!/usr/bin/env bash
set -euo pipefail
cargo component build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/http_extension.wasm extension.wasm
VERSION=$(jq -r .metadata.version describe.json)
zip -r "greentic.http-${VERSION}.gtxpack" \
  extension.wasm \
  describe.json \
  prompts/ \
  schemas/ \
  i18n/
```

### GitHub Actions (`components-public`)

**Existing** `ci.yml` — publishes `component-http` WASM to GHCR. Unchanged.

**NEW** `.github/workflows/publish-extension.yml`:

```yaml
name: Publish HTTP Extension
on:
  push:
    tags: ['ext-v*']
  workflow_dispatch:

jobs:
  publish:
    uses: greenticai/greentic-designer-extension-action@v1
    with:
      extension-dir: crates/http-extension
      version-from-tag: true  # strips "ext-v" prefix
    secrets:
      GREENTIC_STORE_TOKEN: ${{ secrets.GREENTIC_STORE_TOKEN }}
    env:
      GREENTIC_STORE_URL: http://62.171.174.152:3030
```

**Tag conventions** (no collision):
- `v*` → `component-http` release (existing)
- `ext-v*` → `greentic.http` extension release

**Pre-flight check** (`ci/check_publisher_prefix.sh`): before tagging, hit
`GET /api/v1/publishers/greentic` to confirm `allowed_prefixes` contains
`greentic.http`. If missing, script fails with clear message to request
admin action.

**Required repo secrets**: `GREENTIC_STORE_TOKEN` (`gts_*` long-lived token
for the `greentic` publisher).

**Known quirk** (carried forward from adaptive-card experience): server
re-signs describe bytes with its own Ed25519 key per publisher — clients
do NOT sign locally before POST.

## Testing Strategy

### `http-core` (unit tests, native target)

- `config::apply_answers` — QA answer merge preserves defaults + overrides
- `auth::build_auth_header` — each auth type (none, bearer, api_key, basic) produces correct header
- `url::validate_url` — valid schemes, reject `file://`, reject `localhost` (optional flag), placeholder extraction
- `curl::parse_curl` — common patterns (POST -d, GET w/ headers, bearer, base64 basic), unsupported flags emit warnings
- `node::NodeBuilder` — deterministic output, sorted keys

### `component-http` (WASM target)

- Existing tests pass unchanged (backward-compat gate)
- Existing gtest `07_component-http_add_to_flow.gtest` passes unchanged

### `http-extension` (WASM target, via `greentic-ext-testing`)

- `tests/tool_roundtrip.rs` — per tool: canned input → assert output shape + determinism (run twice, byte-compare)
- `tests/validate_content.rs` — `validate_content("http-node", ...)` → expected diagnostics
- `tests/prompt_fragments.rs` — `system_prompt_fragments()` returns rules + examples in correct priority order

### Integration gtests (new)

- `08_http_extension_build.gtest` — build extension → `gtdx validate` on `.gtxpack` passes
- `09_http_extension_gtdx_install.gtest` — `gtdx install greentic.http@0.1.0` (against local Store mock or skip if no Store) → extension loadable

## i18n

Programmatic surface — English only (per user rules):

- Tool titles/descriptions (returned by `list_tools()`)
- Diagnostic messages (returned by `validate_http_config`)
- Rationale strings (returned by generate tools)
- Prompt fragments (`prompts/*.md`)

`i18n/en.json` reserved for future UI strings if Designer surfaces
extension metadata (e.g. tooltips in extension browser). MVP content is
minimal.

## Versioning

- `http-core` — ties to `components-public` workspace version
- `component-http` — ties to `components-public` workspace version (existing pattern)
- `http-extension` — **independent**, starts `0.1.0`, bumped per extension release
  - `describe.json:metadata.version` and `http-extension/Cargo.toml:version` must stay in sync (Store rejects duplicates with HTTP 409)
  - Bump both before tagging `ext-v<version>`

### OCI Ref Pinning Strategy

Generated nodes reference the runtime as:

```
oci://ghcr.io/greenticai/component/component-http:<exact-version>
```

Exact-version pin (e.g. `:0.1.0`). Rationale: the existing CI workflow
pushes tags `:<VERSION>` (full semver) and `:latest` only — it does not
push a minor-only `:0.1` tag. Using full pin gives reproducible flows.

The exact version is baked into the extension at build time from a
constant (`const RUNTIME_VERSION: &str = env!("GREENTIC_HTTP_RUNTIME_VERSION")`)
set in the extension's build script from a file
`crates/http-extension/runtime-version.txt` that tracks the
`component-http` version the extension was built against. When
`component-http` releases, update the version file + extension version,
publish a new extension release.

Alternative considered: have CI additionally push a `:0.1` minor-only
tag, allowing the extension to use a looser pin. Deferred to a future
release — adds complexity to the runtime CI with unclear benefit in v0.1.

## Open Items / Pre-Release Checks

1. **Publisher prefix verification** — confirm `greentic` publisher
   `allowed_prefixes` includes `greentic.http` before first tag. If not,
   admin action required on the Store server.
2. **OCI ref format** — decision: minor-version pin (`:0.1`) for stability.
   Revisit if runtime adopts SemVer breaking changes more aggressively.
3. **Prompt rules content** — first draft includes:
   - "Prefer `secret:NAME` over raw tokens"
   - "Default timeout 15 s, never exceed 60 s"
   - "Include `Content-Type` header when body present for POST/PUT/PATCH"
   - "Generate `node_id` in `snake_case`"
   - "Use `generate_from_card_submit` when previous flow step is a card submit node"

## Success Criteria

- `gtdx publish` for `greentic.http@0.1.0` succeeds against the live Store
- `gtdx install greentic.http` in a fresh Designer session succeeds, and the
  extension appears in `gtdx list`
- LLM in Designer can generate a valid YGTc HTTP node that, when added to
  a flow and executed by `greentic-runner`, performs a successful HTTP
  call against a mock endpoint
- Existing `component-http` gtests continue passing after refactor
- All new crates stay under the 500-lines-per-file budget

## Out of Scope (v0.1)

- OAuth2 flows (separate extension; may be `greentic.oauth` later)
- WebSocket upgrades (would need new WIT interface on runtime side)
- GraphQL-specific helpers (queries, variables, schema introspection)
- OpenAPI spec import (deferred to v0.2 if demand emerges)
- Response schema inference / target-step input mapping (distinct concern
  from request generation)
- Per-request retry/circuit-breaker policy suggestions (future advanced tool)
