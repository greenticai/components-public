# events-triggers-ext

A Greentic Designer **design** extension that ships `nodeType` descriptors for three event-based trigger primitives — timer/schedule, SMS (Twilio), and email (SendGrid) — so they appear as first-class trigger nodes in the Greentic Designer palette.

- **id:** `greentic.events-triggers`
- **version:** `0.1.0-research`
- **kind:** DesignExtension (nodeTypes-only; no embedded WASM tools)

## What it does

This extension carries no runtime logic of its own. It provides the **designer-side descriptor** (JSON Schema, icon, category, color) that the Designer needs to render the inspector forms for each trigger node. At runtime, execution is fully delegated to the corresponding `events-*` packs:

| Node type | Runtime pack |
|---|---|
| `timer-trigger` | `events-timer` (OCI: `ghcr.io/greenticai/packs/events/events-timer:stable`) |
| `sms-trigger` | `events-sms-twilio` (OCI: `ghcr.io/greenticai/packs/events/events-sms-twilio:stable`) |
| `email-trigger` | `events-email-sendgrid` (OCI: `ghcr.io/greenticai/packs/events/events-email-sendgrid:stable`) |

## Trigger nodes

### Timer (`timer-trigger`)

Fires a flow after a configurable delay. This is a delay-based timer, not a cron scheduler.

| Config field | Required | Type | Description |
|---|---|---|---|
| `enabled` | **Yes** | boolean | Whether the timer source is active |
| `timezone` | No | string | IANA timezone (default: `UTC`) |
| `default_delay_seconds` | No | number | Delay in seconds before firing (default: `30`) |
| `persistence_key_prefix` | No | string | Override prefix for persisted scheduled entries |

Output port: **Triggered**

### SMS — Twilio (`sms-trigger`) — `events.sms.twilio`

Starts a flow when an inbound SMS is received via a Twilio-backed messaging provider.

> **Note:** Twilio credentials (Account SID, Auth Token) are **not** configured here. They are held by the messaging provider referenced by `messaging_provider_id`. The trigger node only carries the provider reference and optional overrides.

| Config field | Required | Type | Description |
|---|---|---|---|
| `messaging_provider_id` | **Yes** | string | Stable provider id used for routing and persistence keys |
| `from` | No | string | Default sender number for outbound SMS |
| `persistence_key_prefix` | No | string | Override prefix for persisted inbound requests |

Output port: **Received**

### Email — SendGrid (`email-trigger`) — `events.email.sendgrid`

Starts a flow when an inbound email is received via the SendGrid Inbound Parse webhook.

> **Note:** The SendGrid API key is **not** configured here. It is held by the messaging provider referenced by `messaging_provider_id`. The trigger node only carries the provider reference and optional overrides.

| Config field | Required | Type | Description |
|---|---|---|---|
| `messaging_provider_id` | **Yes** | string | Stable provider id used for routing and persistence keys |
| `from` | No | string | Default From address for outbound integrations |
| `persistence_key_prefix` | No | string | Override prefix for persisted inbound requests |

Output port: **Received**

## Installation

**Via the Greentic Store (recommended):**

```bash
gtdx install greentic.events-triggers
```

**Bundled in a pack:** include `greentic.events-triggers` in your `bundle.yaml` `extensions` list.

The extension is automatically available in the Designer palette after installation. The corresponding `events-*` runtime packs must also be installed (or bundled) for flows to execute.

## Building

```bash
cargo component build --release --target wasm32-wasip2 --package events-triggers-ext
```

Or using the convenience script (requires `describe.json` and `jq`):

```bash
./build.sh
```

## Publish

Store publish via CI:
1. Bump `version` in `describe.json` and `Cargo.toml`
2. Commit + push to main
3. Tag: `git tag events-triggers-ext-v<version> && git push origin events-triggers-ext-v<version>`
4. The `publish-events-triggers-ext` workflow posts the `.gtxpack` to the Store

## Layout

- `describe.json` — extension manifest with the three `nodeType` entries + inline JSON Schemas
- `src/lib.rs` — WASM guest exports (no-op stubs; no tools in v0.1)
- `wit/` — WIT contract
- `i18n/en.json` — English locale strings for node labels, descriptions, and config field names
- `assets/icon.svg` — placeholder trigger glyph (shared across all three nodes in v0.1)

## Extension metadata

- **Package:** `greentic:events-triggers`
- **Kind:** Design (nodeTypes-only, no tools)
- **Offered capability:** `greentic:events-triggers/trigger-nodes`

---

## Contract field notes

> Source: `greentic-designer-sdk/crates/greentic-extension-sdk-contract/src/`
> These notes are the authoritative input for authoring `describe.json` (consumed by Task 3).

### Top-level `DescribeJson` shape

Rust source: `describe/mod.rs:25`

All top-level keys (`deny_unknown_fields` — no extras allowed):

| JSON key | Rust field | Required? | Notes |
|---|---|---|---|
| `$schema` | `schema_ref` | Optional | `skip_serializing_if = "Option::is_none"` |
| `apiVersion` | `api_version` | **Required** | e.g. `"greentic.ai/v2"` |
| `kind` | `kind` | **Required** | e.g. `"DesignExtension"` |
| `compat` | `compat` | **Required** | `{min_designer_version, min_runner_version, contract_version}` — semver strings |
| `metadata` | `metadata` | **Required** | see Metadata table below |
| `engine` | `engine` | Optional | `{greenticDesigner, extRuntime}` — semver-req strings |
| `capabilities` | `capabilities` | **Required** | `{offered: [], required: []}` — both default to empty |
| `runtime` | `runtime` | **Required** | see Runtime table below |
| `execution` | `execution` | Optional | `DesignExtension` MUST NOT set this; only `Bundle` kind may |
| `contributions` | `contributions` | **Required** | see Contributions table below |
| `localization` | `localization` | Optional | |
| `signature` | `signature` | Optional | |
| `manifestSha256` | `manifest_sha256` | Optional | recommended for production packs |
| `requiredSecrets` | `required_secrets` | Optional (defaults `[]`) | array of `SecretRequirement` |

### `Metadata` fields

Rust source: `describe/mod.rs:171`

| JSON key | Required? | Notes |
|---|---|---|
| `id` | **Required** | reverse-DNS string, e.g. `"greentic.events-triggers"` |
| `name` | **Required** | human-readable display name |
| `version` | **Required** | semver string |
| `summary` | **Required** | `LocalizedString` (plain string or `{en: "...", ...}` map) |
| `description` | Optional | same type as `summary` |
| `author` | **Required** | `{name: string, email?: string, publicKey?: string}` |
| `license` | **Required** | SPDX identifier, e.g. `"MIT"` |
| `homepage` | Optional | |
| `repository` | Optional | |
| `keywords` | Optional (defaults `[]`) | |
| `icon` | Optional | |
| `screenshots` | Optional (defaults `[]`) | |

### `Runtime` fields

Rust source: `describe/mod.rs:220`

| JSON key | Required? | Notes |
|---|---|---|
| `memoryLimitMB` | Optional (default `64`) | integer in `[1, 1024]` — validated at parse time |
| `permissions` | **Required** | `{network:[], secrets:[], callExtensionKinds:[], llmRoles?:[], oauthProviders?:[]}` |
| `components` | **Required** | `BTreeMap<string, RuntimeComponent>` — **must have at least one entry** |

### `RuntimeComponent` fields

Rust source: `runtime_component.rs:12`

| JSON key | Rust field | Required? | Notes |
|---|---|---|---|
| `oci_ref` | `oci_ref` | Optional | OCI image ref. At least one of `oci_ref` or `gtpack` **must** be present |
| `gtpack` | `gtpack` | Optional | `RuntimeGtpack` struct (see below). At least one of `oci_ref` or `gtpack` **must** be present |
| `sha256` | `sha256` | **Required** | 64-char lowercase hex string; newtype `Sha256` validates `^[0-9a-f]{64}$` |
| `world` | `world` | **Required** | WIT world string, e.g. `"greentic:events-triggers/extension@1.0.0"` |

Constraint (enforced at deserialize time): `oci_ref.is_none() && gtpack.is_none()` → parse error. Both may be present simultaneously.

#### `RuntimeGtpack` fields (the `gtpack` sub-object)

Rust source: `describe/provider.rs:12`

| JSON key | Required? | Notes |
|---|---|---|
| `file` | **Required** | wasm filename inside the pack, e.g. `"extension.wasm"` |
| `sha256` | **Required** | 64-char lowercase hex only — uppercase rejected at parse time |
| `pack_id` | **Required** | e.g. `"greentic.events-triggers"` |
| `component_version` | **Required** | semver string of the packed component |

The `runtime.components` object is a **plain JSON map** (not an array). Each key is a `ComponentId` string (the slot name, e.g. `"events-triggers"`); each value is a `RuntimeComponent` object.

### `Contributions` fields

Rust source: `describe/contributions.rs:25`

All fields default to empty vec and are omitted from JSON when empty. The struct uses `rename_all = "camelCase"` **except** `dwProviders` (explicit rename).

| JSON key | Rust field | Notes |
|---|---|---|
| `nodeTypes` | `node_types` | array of `NodeType` |
| `tools` | `tools` | array of `Tool` |
| `recipes` | `recipes` | array of `Recipe` |
| `knowledge` | `knowledge` | array of `Knowledge` |
| `prompts` | `prompts` | array of `Prompt` |
| `schemas` | `schemas` | array of `Schema` |
| `dwProviders` | `dw_providers` | array of `DwProvider` |
| `guardrails` | `guardrails` | array of `Guardrail` |

### `NodeType` fields

Rust source: `describe/contributions/node_type.rs:14`

`deny_unknown_fields` — no extras allowed. No `rename_all`; all field names serialize as-is.

| JSON key | Rust field | Required? | Notes |
|---|---|---|---|
| `type_id` | `type_id` | **Required** | string; referenced by `runtime_ref` validation at parse time |
| `label` | `label` | **Required** | `LocalizedString` (plain string or `{en: "...", ...}` map) |
| `category` | `category` | **Required** | string; e.g. `"trigger"` |
| `icon` | `icon` | **Required** | string; icon name or emoji |
| `color` | `color` | **Required** | hex color string, e.g. `"#0ea5e9"` |
| `complexity` | `complexity` | **Required** | string; e.g. `"simple"` |
| `config_schema` | `config_schema` | **Required** | JSON Schema string (serialized as a string, not an object) |
| `output_ports` | `output_ports` | Optional (default `[]`) | array of `{name: string, label: LocalizedString}` |
| `runtime_ref` | `runtime_ref` | Optional | `ComponentId` — must match a key in `runtime.components` (validated at parse time) |
| `deprecated` | `deprecated` | Optional | `Deprecated` struct |

### Cross-cutting invariants (enforced at `DescribeJson` parse time)

1. `runtime.components` must contain at least one entry.
2. `runtime.memoryLimitMB` must be in `[1, 1024]`.
3. `execution` is only allowed when `kind = BundleExtension`.
4. Every `runtime_ref` in `contributions.nodeTypes` and `contributions.tools` must reference a key that exists in `runtime.components`.
5. Every `RuntimeComponent` must have at least one of `oci_ref` or `gtpack`.
6. `RuntimeGtpack.sha256` and the top-level `RuntimeComponent.sha256` must be 64-char lowercase-hex strings.

### Example `runtime.components` pattern (from existing extensions)

```json
"components": {
  "events-triggers": {
    "gtpack": {
      "file": "extension.wasm",
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
      "pack_id": "greentic.events-triggers",
      "component_version": "0.1.0"
    },
    "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
    "world": "greentic:events-triggers/extension@1.0.0"
  }
}
```

The slot name (`"events-triggers"`) is the value used in `NodeType.runtime_ref`.

---

## Runtime execution model

This extension is **design-time only**: it surfaces the three trigger nodes (timer, SMS, email) and their typed config schemas in the Designer palette. It does NOT execute events.

**Execution requires the operator to install or bundle the matching `events-*` provider packs** into the deployment. Specifically, add these to `bundle.yaml` under `extension_providers`:

```
ghcr.io/greenticai/packs/events/events-timer:stable
ghcr.io/greenticai/packs/events/events-sms-twilio:stable
ghcr.io/greenticai/packs/events/events-email-sendgrid:stable
```

`greentic-bundle` materialises them as `providers/*.gtpack` files. `greentic-start` discovers providers from `<bundle>/providers/*.gtpack` by domain and fires them by `provider` name — NOT via this extension's `runtime_ref`. The `runtime_ref` in `describe.json` pins a component into the flow pack's resolve sidecar; how it maps to `pack + component_ref` at execution time is defined by consumer crates (`greentic-events` / `greentic-deployer` / `greentic-pack`) and is an unverified follow-up.

The pack's canonical binding is `greentic.provider-extension.v1` → `component_ref` (`events-provider-{timer,sms-twilio,email-sendgrid}`) + `export: schema-core`, world `greentic:provider/schema-core@1.0.0`.

**Known limitations:**

- The `events-timer` / `events-sms` / `events-email` demo packs use STUB sources. The genuinely runnable provider components are in `events-webhook`, `events-sms-twilio`, `events-email-sendgrid`, and `events-dummy`.
- SMS and email credentials are **not** stored in the trigger config. They are delegated to a separate messaging provider identified by `messaging_provider_id`. The trigger node config carries only the provider ID (and optional overrides such as `from` and `persistence_key_prefix`); the actual Twilio/SendGrid credentials are held by the operator's messaging provider configuration.
