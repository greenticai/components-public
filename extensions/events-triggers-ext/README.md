# events-triggers-ext

Greentic design extension that ships `nodeType` descriptors for event-based trigger nodes (timer, SMS, email) so they appear as first-class trigger primitives in the Greentic Designer palette.

## Building

```bash
cargo component build --release --target wasm32-wasip2 --package events-triggers-ext
```

Or using the convenience script (requires `describe.json` and `jq`):

```bash
./build.sh
```

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
