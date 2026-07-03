# Event Triggers as Designer Nodes — Implementation Plan (Slice 1a)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a new `events-triggers-ext` design extension so timer, SMS (Twilio), and email (SendGrid) event providers appear as configurable trigger nodes in the Greentic Designer palette.

**Architecture:** A WASM (`wasm32-wasip2`) design extension in `components-public/extensions/events-triggers-ext/`, structured like the minimal `platform-extension`. Its `describe.json` declares three trigger `nodeTypes` (each with a JSON-Schema `config_schema` and a `runtime_ref`) plus a `runtime.components` map whose entries carry the existing events-* OCI references. The designer's `registry_swap` already consumes `contributions.nodeTypes`, so the nodes surface with no `greentic-designer` code change; `resolve_runtime_ref` binds each node to its events-* pack at flow-compile time.

**Tech Stack:** Rust (edition 2024, 1.95.0), `wit-bindgen-rt`, `serde`/`serde_json`, `cargo-component`, target `wasm32-wasip2`. Reference: `components-public/extensions/platform-extension` (minimal design extension) and `.../webhook-extension` (trigger nodeType shape).

## Global Constraints

- Rust 1.95.0, edition 2024 (`rust-toolchain.toml` is canonical — do not add a per-crate pin).
- Max 500 lines per Rust source file.
- English only in source, tests, comments, commit messages.
- No Claude co-author trailer on commits (safe default; `components-public` has no CLAUDE.md but sibling repos forbid it).
- `crate-type = ["cdylib", "rlib"]`; `[package.metadata.component] package = "greentic:events-triggers"`.
- Never invent OCI values: `oci_ref` strings are copied verbatim from `greentic-designer/assets/providers-registry.json`; `sha256`/`world` are resolved from the real packs (Task 6).
- Node contract fields mirror `webhook-extension/describe.json` exactly (`type_id`, `label`, `category`, `icon`, `color`, `complexity`, `config_schema`, `output_ports`, `runtime_ref`).
- Secrets are referenced by name via a `secret_ref` string field — never a raw secret value (webhook pattern).

---

## Task 1: Scaffold the extension crate (mirror `platform-extension`)

**Files:**
- Create: `extensions/events-triggers-ext/Cargo.toml`
- Create: `extensions/events-triggers-ext/wit/world.wit` (+ copy `wit/deps/` from `platform-extension`)
- Create: `extensions/events-triggers-ext/src/lib.rs`
- Create: `extensions/events-triggers-ext/build.sh`, `ci/local_check.sh`, `.gitignore`, `README.md`
- Reference: `extensions/platform-extension/{Cargo.toml,wit/world.wit,src/lib.rs,build.sh}`

**Interfaces:**
- Produces: a buildable design extension whose `manifest`/`lifecycle` world exports match `platform-extension` (a nodeTypes-only design extension needs no `tools` export — `platform-extension` proves this).

- [ ] **Step 1: Read the reference extension**

Run: `cat extensions/platform-extension/Cargo.toml extensions/platform-extension/wit/world.wit extensions/platform-extension/src/lib.rs extensions/platform-extension/build.sh`
Expected: understand the minimal manifest/lifecycle export shape and the WIT world it targets. Note the exact `[package.metadata.component.target]` and dependency versions.

- [ ] **Step 2: Copy the scaffold**

```bash
mkdir -p extensions/events-triggers-ext/src extensions/events-triggers-ext/wit extensions/events-triggers-ext/ci
cp -r extensions/platform-extension/wit/deps extensions/events-triggers-ext/wit/deps
cp extensions/platform-extension/build.sh extensions/events-triggers-ext/build.sh
cp extensions/platform-extension/ci/local_check.sh extensions/events-triggers-ext/ci/local_check.sh
cp extensions/platform-extension/.gitignore extensions/events-triggers-ext/.gitignore
```

- [ ] **Step 3: Write `Cargo.toml`** (edit the copied reference values: name, package)

```toml
[package]
name = "events-triggers-ext"
version = "0.1.0-research"
edition = "2024"
license = "MIT"
authors = ["Greentic"]

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
# Copy exact versions from platform-extension/Cargo.toml (wit-bindgen-rt, serde, serde_json)
wit-bindgen-rt = { version = "0.35", features = ["bitflags"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[package.metadata.component]
package = "greentic:events-triggers"

[package.metadata.component.target]
path = "wit"
```

- [ ] **Step 4: Write `wit/world.wit`** — copy `platform-extension/wit/world.wit`, rename the world/package identifier to `events-triggers`. Keep the same imported/exported interfaces (manifest, lifecycle).

- [ ] **Step 5: Write `src/lib.rs`** — mirror `platform-extension/src/lib.rs`: implement the manifest + lifecycle exports with no tools. Change only the package identifiers in the generated `bindings` path.

- [ ] **Step 6: Build to wasm**

Run: `cd extensions/events-triggers-ext && cargo component build --release --target wasm32-wasip2`
Expected: PASS — produces `target/wasm32-wasip2/release/events_triggers_ext.wasm`.

- [ ] **Step 7: Commit**

```bash
git add extensions/events-triggers-ext
git commit -m "feat: scaffold events-triggers-ext design extension"
```

---

## Task 2: Pin the SDK-contract field requirements (no-guess gate)

**Files:**
- Reference (read-only): `greentic-designer-sdk/crates/*` (the `greentic-extension-sdk-contract` source) OR the crates.io docs for `greentic_extension_sdk_contract::{DescribeJson, describe::NodeType, runtime_component::RuntimeComponent}`.

**Interfaces:**
- Produces: the exact required-vs-optional field list for `NodeType` and `RuntimeComponent`, used verbatim in Task 3.

- [ ] **Step 1: Read the contract structs**

Run: `rg -n "struct NodeType|struct RuntimeComponent|struct Contributions" -A 20 greentic-designer-sdk/crates`
Expected: capture which `RuntimeComponent` fields are required (`oci_ref`, `sha256`, `world`?) vs `Option`, and the exact `NodeType` field names/casing (`type_id`, `config_schema`, `output_ports`, `runtime_ref`).

- [ ] **Step 2: Record findings in the README**

Append a short "Contract field notes" section to `extensions/events-triggers-ext/README.md` listing the required fields, so Task 3 authors `describe.json` against the real schema.

- [ ] **Step 3: Commit**

```bash
git add extensions/events-triggers-ext/README.md
git commit -m "docs: record sdk-contract field requirements for describe.json"
```

---

## Task 3: Author `describe.json` (nodeTypes + runtime.components) with a parse test

**Files:**
- Create: `extensions/events-triggers-ext/describe.json`
- Create: `extensions/events-triggers-ext/tests/describe_contract.rs`
- Reference: `extensions/webhook-extension/describe.json` (nodeType shape), `greentic-designer/assets/providers-registry.json` (OCI refs)

**Interfaces:**
- Consumes: SDK-contract field list (Task 2).
- Produces: a valid `DescribeJson` with three trigger nodeTypes whose `runtime_ref` keys resolve to `runtime.components` entries. `config_schema` bodies are added in Task 4 (Task 3 uses `{"type":"object"}` placeholders that Task 4 replaces).

- [ ] **Step 1: Write the failing test** (`tests/describe_contract.rs`)

```rust
use greentic_extension_sdk_contract::DescribeJson;

fn describe() -> DescribeJson {
    let raw = include_str!("../describe.json");
    serde_json::from_str(raw).expect("describe.json must deserialize into DescribeJson")
}

#[test]
fn declares_three_trigger_nodetypes() {
    let d = describe();
    let ids: Vec<&str> = d
        .contributions
        .node_types
        .iter()
        .map(|nt| nt.type_id.as_str())
        .collect();
    assert!(ids.contains(&"timer-trigger"));
    assert!(ids.contains(&"sms-trigger"));
    assert!(ids.contains(&"email-trigger"));
    for nt in &d.contributions.node_types {
        assert_eq!(nt.category.as_deref(), Some("trigger"), "{} category", nt.type_id);
    }
}

#[test]
fn every_runtime_ref_resolves_to_a_component_with_oci_ref() {
    let d = describe();
    for nt in &d.contributions.node_types {
        let rr = nt.runtime_ref.as_deref().expect("trigger needs runtime_ref");
        let comp = d.runtime.components.get(rr).expect("runtime_ref must resolve");
        let oci = comp.oci_ref.as_deref().expect("component needs oci_ref");
        assert!(oci.contains("ghcr.io/greenticai/packs/events/"), "{oci}");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p events-triggers-ext --test describe_contract`
Expected: FAIL — `describe.json` does not exist / does not parse.

- [ ] **Step 3: Write `describe.json`** (metadata/engine/runtime blocks mirror `webhook-extension`; `sha256`/`world` are placeholders replaced in Task 6; `config_schema` are `{"type":"object"}` placeholders replaced in Task 4)

```json
{
  "apiVersion": "greentic.ai/v2",
  "kind": "DesignExtension",
  "compat": { "min_designer_version": ">=1.2.0", "min_runner_version": "^0.12.0", "contract_version": "1.2.0" },
  "metadata": {
    "id": "greentic.events-triggers",
    "name": "Event Triggers",
    "version": "0.1.0-research",
    "summary": "Timer, SMS, and email event providers as flow trigger nodes",
    "author": { "name": "Greentic" },
    "license": "MIT"
  },
  "engine": { "greenticDesigner": ">=1.2.0", "extRuntime": "^0.12.0" },
  "capabilities": { "offered": [], "required": [] },
  "runtime": {
    "memoryLimitMB": 64,
    "permissions": { "network": [], "secrets": [], "callExtensionKinds": [] },
    "components": {
      "events-timer": { "oci_ref": "oci://ghcr.io/greenticai/packs/events/events-timer:stable", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "world": "TBD-task6" },
      "events-sms-twilio": { "oci_ref": "oci://ghcr.io/greenticai/packs/events/events-sms-twilio:stable", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "world": "TBD-task6" },
      "events-email-sendgrid": { "oci_ref": "oci://ghcr.io/greenticai/packs/events/events-email-sendgrid:stable", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "world": "TBD-task6" }
    }
  },
  "contributions": {
    "nodeTypes": [
      { "type_id": "timer-trigger", "label": "Timer / Schedule", "category": "trigger", "icon": "clock", "color": "#0ea5e9", "complexity": "simple", "config_schema": "{\"type\":\"object\"}", "output_ports": [{ "name": "default", "label": "Triggered" }], "runtime_ref": "events-timer" },
      { "type_id": "sms-trigger", "label": "SMS (Twilio)", "category": "trigger", "icon": "message-square", "color": "#0ea5e9", "complexity": "simple", "config_schema": "{\"type\":\"object\"}", "output_ports": [{ "name": "default", "label": "Received" }], "runtime_ref": "events-sms-twilio" },
      { "type_id": "email-trigger", "label": "Email (SendGrid)", "category": "trigger", "icon": "mail", "color": "#0ea5e9", "complexity": "simple", "config_schema": "{\"type\":\"object\"}", "output_ports": [{ "name": "default", "label": "Received" }], "runtime_ref": "events-email-sendgrid" }
    ]
  }
}
```

> If Task 2 found `sha256`/`world` are non-optional and reject the placeholders at parse time, keep syntactically valid placeholders (64 hex zeros / a non-empty string) until Task 6 replaces them — deserialization only needs the fields present, not resolved.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p events-triggers-ext --test describe_contract`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add extensions/events-triggers-ext/describe.json extensions/events-triggers-ext/tests/describe_contract.rs
git commit -m "feat: declare three trigger nodeTypes bound to events-* packs"
```

---

## Task 4: Author the three `config_schema` bodies with validation tests

**Files:**
- Modify: `extensions/events-triggers-ext/describe.json` (replace the three `config_schema` placeholders)
- Create: `extensions/events-triggers-ext/tests/config_schema.rs`
- Reference: `greentic-docs/src/content/docs/providers/events/sms-twilio.md` (Twilio fields — confirmed: `account_sid`, `auth_token`, `from_number`)

**Interfaces:**
- Consumes: the nodeTypes from Task 3.
- Produces: each `config_schema` is a valid JSON object schema with the fields below; secrets are `secret_ref` strings.

- [ ] **Step 1: Write the failing test** (`tests/config_schema.rs`)

```rust
use greentic_extension_sdk_contract::DescribeJson;
use serde_json::Value;

fn schema_for(type_id: &str) -> Value {
    let d: DescribeJson = serde_json::from_str(include_str!("../describe.json")).unwrap();
    let nt = d.contributions.node_types.iter().find(|n| n.type_id == type_id).unwrap();
    serde_json::from_str(nt.config_schema.as_deref().unwrap()).expect("config_schema is valid JSON")
}

fn required(schema: &Value) -> Vec<String> {
    schema["required"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default()
}

#[test]
fn timer_requires_schedule() {
    let s = schema_for("timer-trigger");
    assert_eq!(s["type"], "object");
    assert!(required(&s).contains(&"schedule".to_string()));
}

#[test]
fn sms_requires_from_and_secret_refs() {
    let s = schema_for("sms-trigger");
    let req = required(&s);
    assert!(req.contains(&"from_number".to_string()));
    assert!(req.contains(&"account_sid".to_string()));
    assert!(req.contains(&"auth_token".to_string()));
    // secret fields describe a secret_ref, not a raw value
    assert!(s["properties"]["account_sid"]["description"].as_str().unwrap().to_lowercase().contains("secret"));
}

#[test]
fn email_requires_from_and_api_key_secret() {
    let s = schema_for("email-trigger");
    let req = required(&s);
    assert!(req.contains(&"from_address".to_string()));
    assert!(req.contains(&"api_key".to_string()));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p events-triggers-ext --test config_schema`
Expected: FAIL — placeholders have no `required`/`properties`.

- [ ] **Step 3: Replace the three `config_schema` strings** in `describe.json`

Timer (`timer-trigger`):
```json
{"type":"object","required":["schedule"],"properties":{"schedule":{"type":"string","description":"Cron expression or interval (e.g. \"0 9 * * *\" or \"30m\")","examples":["0 9 * * 1-5","15m"]},"timezone":{"type":"string","description":"IANA timezone for cron evaluation (default UTC)","examples":["Europe/London","Asia/Jakarta"]}}}
```

SMS (`sms-trigger`):
```json
{"type":"object","required":["from_number","account_sid","auth_token"],"properties":{"from_number":{"type":"string","description":"Sender phone number in E.164 format (must belong to the Twilio account)","examples":["+14155551234"]},"account_sid":{"type":"string","description":"Secret name holding the Twilio Account SID (secret_ref)"},"auth_token":{"type":"string","description":"Secret name holding the Twilio Auth Token (secret_ref)"}}}
```

Email (`email-trigger`):
```json
{"type":"object","required":["from_address","api_key"],"properties":{"from_address":{"type":"string","description":"Verified sender address","examples":["noreply@acme.com"]},"api_key":{"type":"string","description":"Secret name holding the SendGrid API key (secret_ref)"}}}
```

> Verify the SendGrid field names and any timer schedule grammar against the events-* pack manifests when resolving digests in Task 6; adjust the schema + this test together if a field name differs.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p events-triggers-ext --test config_schema`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add extensions/events-triggers-ext/describe.json extensions/events-triggers-ext/tests/config_schema.rs
git commit -m "feat: typed config schemas for timer/sms/email triggers"
```

---

## Task 5: i18n, icons, README

**Files:**
- Create: `extensions/events-triggers-ext/i18n/en.json`
- Create: `extensions/events-triggers-ext/assets/icon.svg`
- Modify: `extensions/events-triggers-ext/README.md`
- Reference: `extensions/webhook-extension/{i18n/en.json,assets/icon.svg,README.md}`

**Interfaces:**
- Consumes: node labels from Task 3.
- Produces: locale strings for the three node labels/descriptions; a shared trigger icon; a README describing install + the three nodes.

- [ ] **Step 1: Write `i18n/en.json`** mirroring `webhook-extension/i18n/en.json` key structure, with entries for `timer-trigger`, `sms-trigger`, `email-trigger` labels + descriptions.

- [ ] **Step 2: Add `assets/icon.svg`** — copy `webhook-extension/assets/icon.svg` as a placeholder trigger glyph (per-node icons are cosmetic; a shared one is acceptable for v0.1).

- [ ] **Step 3: Write `README.md`** — what the extension is, the three nodes, how it installs (`gtdx install` / bundled), and that runtime execution is delegated to the events-* packs.

- [ ] **Step 4: Commit**

```bash
git add extensions/events-triggers-ext/i18n extensions/events-triggers-ext/assets extensions/events-triggers-ext/README.md
git commit -m "docs: i18n, icon, and README for events-triggers-ext"
```

---

## Task 6: Resolve real `sha256` + `world` for the three events-* packs

**Files:**
- Modify: `extensions/events-triggers-ext/describe.json` (`runtime.components.*.sha256` and `.world`)

**Interfaces:**
- Consumes: the three `oci_ref` tags.
- Produces: pinned digests + world strings so `resolve_runtime_ref` yields a fully-specified `RuntimeComponent`.

> **Environment dependency:** this task needs read access to `ghcr.io/greenticai/packs/events/*` (or a local copy of the packs). If the current environment cannot reach ghcr, mark the task blocked and record it in the PR summary — Tasks 1–5 + 7 (with the `:stable` tag ref) are independently reviewable, and the digests can be pinned in a follow-up commit from an environment with registry access.

- [ ] **Step 1: Resolve each tag to a digest + read its world**

Run (per pack), preferring the repo's own resolver if a CLI exists, else `oras`/`crane`:
```bash
oras manifest fetch ghcr.io/greenticai/packs/events/events-timer:stable --descriptor
oras manifest fetch ghcr.io/greenticai/packs/events/events-sms-twilio:stable --descriptor
oras manifest fetch ghcr.io/greenticai/packs/events/events-email-sendgrid:stable --descriptor
```
Expected: a `sha256:…` digest per pack. Read each pack's component `world` from its manifest/`component.json`.

- [ ] **Step 2: Replace the placeholders** in `describe.json` with the resolved `sha256` (hex, no `sha256:` prefix if the contract stores bare hex — match the shape Task 2 recorded) and `world` strings.

- [ ] **Step 3: Re-run the contract test**

Run: `cargo test -p events-triggers-ext --test describe_contract`
Expected: PASS (structure unchanged; values now real).

- [ ] **Step 4: Commit**

```bash
git add extensions/events-triggers-ext/describe.json
git commit -m "feat: pin events-* pack digests and worlds in runtime.components"
```

---

## Task 7: Designer integration proof (registry_swap surfaces the three triggers)

**Files:**
- Test/verify against a running/instantiated designer with the built extension installed. No `greentic-designer` code change expected; if a regression test is warranted, add `greentic-designer/tests/events_triggers_nodetypes.rs` (a small cross-repo addition, flagged as an S1↔designer handshake in the PR).

**Interfaces:**
- Consumes: the built `events-triggers-ext` wasm + `describe.json`.
- Produces: evidence that `/api/node-types` lists `timer-trigger`, `sms-trigger`, `email-trigger` under `category: "trigger"`.

- [ ] **Step 1: Install the built extension into the designer's extension dir**

```bash
mkdir -p ~/.greentic/extensions/design/events-triggers-ext-0.1.0-research
cp extensions/events-triggers-ext/describe.json ~/.greentic/extensions/design/events-triggers-ext-0.1.0-research/
cp extensions/events-triggers-ext/target/wasm32-wasip2/release/events_triggers_ext.wasm ~/.greentic/extensions/design/events-triggers-ext-0.1.0-research/extension.wasm
```

- [ ] **Step 2: Boot the designer and query node-types**

Run: `cargo run -p greentic-designer -- ui` (in the greentic-designer repo), then `curl -s localhost:PORT/api/node-types | jq '.[] | select(.category=="trigger") | .type_id'`
Expected: output includes `"timer-trigger"`, `"sms-trigger"`, `"email-trigger"`.

- [ ] **Step 3: (Conditional) add a designer regression test** only if Step 2 fails or a durable guard is wanted — a test that loads the describe fixture and asserts `registry_swap` includes the three trigger type_ids. Flag it as a cross-repo change in the PR.

- [ ] **Step 4: Commit any verification artifacts / notes**

```bash
git add -A && git commit -m "test: verify registry_swap surfaces the three trigger nodes"
```

---

## Task 8: End-to-end timer proof (Run Demo)

**Files:**
- No code; a manual/scripted verification recorded in the PR summary.

**Interfaces:**
- Consumes: everything above + a resolvable `events-timer` pack (Task 6 digests).

- [ ] **Step 1: Build a minimal flow** in the designer: `timer-trigger` (schedule `1m`) → a `template`/reply node.

- [ ] **Step 2: Run it via Run Demo** (embedded runner-host) and confirm the timer fires and the flow executes — proving `runtime_ref` → `events-timer` resolves end-to-end.

- [ ] **Step 3: Record the outcome** in the PR summary (timer proven live; sms/email verified through config + pack-build, live execution pending Twilio/SendGrid credentials).

---

## Self-Review

- **Spec coverage:** goal (3 trigger nodes) → Tasks 3/4; Approach A extension scaffold → Task 1; runtime binding (a) → Task 3 (`runtime.components`) + Task 6 (real digests); config schemas incl. secret_ref → Task 4; testing (unit + integration + timer e2e) → Tasks 3/4/7/8; non-goals (webhook, WIT event-source, fast2flow) untouched. Covered.
- **Placeholder scan:** the only intentional deferrals are `sha256`/`world` (Task 6, env-gated with an explicit command) and `config_schema` bodies (filled in Task 4) — both are concrete resolution steps, not vague "implement later".
- **Type consistency:** `type_id` values (`timer-trigger`/`sms-trigger`/`email-trigger`), `runtime_ref` keys (`events-timer`/`events-sms-twilio`/`events-email-sendgrid`), and the `runtime.components` map keys match across Tasks 3, 4, 6, 7.
