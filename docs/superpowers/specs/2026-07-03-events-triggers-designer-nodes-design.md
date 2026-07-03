# Event Triggers as Designer Nodes — Design (Slice 1a)

Date: 2026-07-03
Repo: `components-public` (org `greenticai`)
Branch: `feat/events-triggers-ext` (worktree, based on `research`)

## Context & Problem

The Greentic **event-provider family** already ships as published runtime OCI packs and is
catalogued in the designer's `providers-registry.json` (`category: "events"`):

```
ghcr.io/greenticai/packs/events/events-webhook | events-timer
                                  events-email  | events-email-sendgrid
                                  events-sms    | events-sms-twilio
```

At **runtime** these work (used by `greentic-demo/quickstart-event-demo`). In the **designer**,
however, only **webhook** is selectable as a trigger — and it appears not via the event-provider
mechanism but because the `webhook-extension` (a `DesignExtension`) contributes a `type_id: "trigger"`
node via `describe.json` `contributions.nodeTypes`. SMS, email, and timer have no such contribution,
so a designer user cannot drop them on a flow as triggers.

Audit finding (2026-07-03): the WIT `event-source` interface (`list-trigger-types` / `trigger-schema`)
is **defined but dead** — no dispatch in `greentic-ext-runtime` (`runtime.rs` / `host_bindings.rs`),
no extension implements it. The live path for surfacing a trigger in the palette is
`contributions.nodeTypes`, consumed by the designer's `registry_swap` (`/api/node-types`).

## Goal

Make **timer**, **sms** (Twilio), and **email** (SendGrid) selectable and configurable as trigger
nodes in the designer, using the proven `contributions.nodeTypes` mechanism — mirroring
`webhook-extension`. Near-zero change to `greentic-designer`; the work is a new design extension.

### Non-goals (deferred)

- Webhook trigger (already shipped via `webhook-extension`).
- Generic `events-sms` / `events-email` (non-Twilio / non-SendGrid) — ship the concrete providers first.
- Reviving the WIT `event-source` interface (dead code; not touched).
- `fast2flow` inbound routing surface, guardrail/RAG/LLM flow-parity (separate Session-1 sub-projects).
- Per-channel config schema in the deploy `ChannelsStep` (sub-project 1c) — though the `secret_ref`
  pattern established here is the same one 1c will reuse.

## Approach — new `events-triggers-ext` DesignExtension (Approach A)

A new WASM design extension at `components-public/extensions/events-triggers-ext/`, structured like
the **minimal** `platform-extension` (a nodeTypes-only design extension: no design-time tools required
for a node to appear — `platform-extension` contributes `start` with none). Its `describe.json`
contributes three trigger `nodeTypes`. Because `registry_swap` already consumes
`contributions.nodeTypes`, the nodes surface in the palette automatically.

Rejected alternatives:
- **B — designer synthesizes nodeTypes from the `events` registry category.** Requires a
  `greentic-designer` code change and still needs a home for config schemas; diverges from the
  webhook pattern.
- **C — extend `webhook-extension`.** Conflates the webhook design extension with unrelated event
  providers; less cohesive.

## Architecture

```
components-public/extensions/events-triggers-ext/
  Cargo.toml                 # cdylib+rlib, wasm32-wasip2, package = greentic:events-triggers
  describe.json              # kind: DesignExtension
    contributions.nodeTypes:
      - timer-trigger   (category "trigger", config_schema, output_ports, runtime_ref → events-timer)
      - sms-trigger     (category "trigger", config_schema, output_ports, runtime_ref → events-sms-twilio)
      - email-trigger   (category "trigger", config_schema, output_ports, runtime_ref → events-email-sendgrid)
  src/lib.rs, src/bindings.rs  # minimal design world export (manifest/lifecycle), mirror platform-extension
  wit/world.wit                # design world (vendored, mirror platform-extension)
  i18n/en.json                 # node labels/descriptions
  assets/icon.svg              # trigger icons (per node or shared)
  build.sh, ci/local_check.sh, README.md
```

### Discovery & data flow (unchanged designer plumbing)

```
events-triggers-ext installed (~/.greentic/extensions/design/…)
   → GET /api/node-types (registry_swap reads contributions.nodeTypes)
   → CanvasContextMenu shows timer/sms/email under the "trigger" category
   → user drops a node
   → Inspector renders config_schema via the existing JsonSchemaForm
   → pack build resolves runtime_ref (resolve_runtime_ref) to the events-* runtime component
   → the events-* pack executes the trigger at runtime
```

### Node contract (per trigger)

Each nodeType mirrors `webhook-extension`'s shape:
- `type_id`, `label`, `category: "trigger"`, `icon`, `color`, `complexity: "simple"`.
- `config_schema`: a JSON-Schema string (see Config Schemas below).
- `output_ports`: at minimum `default` ("Triggered"); sms/email add no `rejected` port (that is a
  webhook-auth concept) unless a provider defines a failure branch.
- `runtime_ref`: the binding to the executing events-* pack (see Runtime Binding).

## Config Schemas

Secrets follow the webhook pattern: a `secret_ref` string field (the **name** of a secret), never a
raw value.

| Trigger | Fields | Secret refs | Source |
|---|---|---|---|
| **timer** | `schedule` (cron or interval, required), `timezone` (optional) | — | events-timer pack |
| **sms** (Twilio) | `from_number` (E.164, required) | `account_sid`, `auth_token` (both required, as `secret_ref`) | `providers/events/sms-twilio.md` (confirmed) |
| **email** (SendGrid) | `from_address` (required) | `api_key` (required, as `secret_ref`) | events-email-sendgrid pack |

The exact `schedule` grammar for timer and the SendGrid field names are pinned during the plan by
reading each events-* pack's own manifest/config (the registry `ref` resolves to the pack).

## Runtime Binding (the key integration decision)

A trigger nodeType must bind to the events-* pack that executes it. Options:

- **(a) — recommended:** the nodeType carries the events-* OCI reference already present in
  `providers-registry.json` (e.g. `events-timer` → `oci://ghcr.io/greenticai/packs/events/events-timer:stable`),
  and the designer's `resolve_runtime_ref` resolves it the same way it resolves
  `webhook-extension`'s `runtime_ref: "webhook"`.
- (b) the design extension declares the events-* pack as its own bundled runtime component.

Recommendation: **(a)** — reuse the refs the registry already carries; no duplication. The plan MUST
first read exactly how `webhook-extension`'s `runtime_ref: "webhook"` resolves to a runnable component
(`resolve_extensions.rs` / `extension_lifecycle.rs`) and mirror it identically, so timer/sms/email
resolve through the same path. If webhook's `runtime_ref` resolves to a component bundled with the
extension rather than an external OCI ref, option (b) applies and the spec is updated accordingly.
This is the single load-bearing risk and is resolved by inspection before implementation.

## Testing

1. **Unit (extension):** `describe.json` parses; each `config_schema` is valid JSON-Schema; required
   fields and `secret_ref` shapes present. Mirror `webhook-extension`'s test layout.
2. **Integration (designer, read-only against the built extension):** with `events-triggers-ext`
   installed, `/api/node-types` returns the three trigger nodeTypes under the `trigger` category, each
   with its `config_schema`. (Runs in the designer repo's test suite; no designer code change expected
   — this asserts the contribution is consumed.)
3. **End-to-end proof — timer:** timer needs no credentials. Build a flow with a timer trigger, pack
   it, and run it via Run Demo (embedded runner-host) to prove `runtime_ref` resolves and the events-*
   pack executes end-to-end. sms/email are verified through config + pack-build; their live execution
   needs Twilio/SendGrid credentials and is validated separately.

## Repo, Branch, Isolation

- Repo `components-public` (greenticai). Branch `feat/events-triggers-ext` off `research`, in worktree
  `.worktrees/events-triggers-ext`.
- **No Claude co-author trailer** — verify each touched repo's `CLAUDE.md`. (`greentic-designer` /
  `greentic-designer-admin` forbid it; confirm the `components-public` policy before committing.)
- Isolation vs the other parallel sessions: S2 = `greentic-designer-admin`, S3 = `greentic-biz/component-doc-ext`.
  This slice touches `components-public` only. If integration test #2 reveals a required
  `greentic-designer` change (it should not), that becomes an explicit S1↔designer handshake item.

## Risks / Open Questions (resolved before or during plan)

1. **Runtime binding (a vs b)** — resolved by reading webhook's `runtime_ref` resolution first.
2. **Does a nodeTypes-only design extension need any WIT tool export?** `platform-extension` suggests
   no; confirm by mirroring its `world.wit` / `lib.rs` exactly.
3. **Exact events-* config field names** (timer schedule grammar, SendGrid fields) — pinned from each
   pack's manifest during the plan.
4. **`components-public` CLAUDE.md / signing** — confirm describe-signing + CI expectations before the
   first commit (mirror webhook-extension's `build.sh` + `ci/local_check.sh`).

---

## Runtime execution model (Task 6 update — source-of-truth alignment)

This extension is **design-time only**: it surfaces the three trigger nodes and their typed config schemas in the Designer palette. It does NOT execute events. **Execution requires the operator to install or bundle the matching `events-*` provider packs** (`ghcr.io/greenticai/packs/events/events-{timer,sms-twilio,email-sendgrid}`) into the deployment (`bundle.yaml` `extension_providers` → `greentic-bundle` → `providers/*.gtpack`). `greentic-start` discovers providers from `<bundle>/providers/*.gtpack` by domain and fires them by `provider` name — NOT via this extension's `runtime_ref`. The `runtime_ref → RuntimeComponent.oci_ref` only pins a component into the flow pack's resolve sidecar; how it maps to `pack + component_ref` is defined by consumer crates (`greentic-events` / `greentic-deployer` / `greentic-pack`) and is an unverified follow-up. **SMS and email credentials are NOT in the trigger config** — they are delegated to a separate messaging provider named by `messaging_provider_id`.

### Config schema corrections (Task 6)

Investigation of the real pack sources revealed the prior schemas were hand-authored and incorrect. The corrected schemas are:

| Trigger | Required field(s) | Notes |
|---|---|---|
| `timer-trigger` | `enabled` (boolean) | Delay-based, not cron. Optional: `timezone`, `default_delay_seconds`, `persistence_key_prefix`. |
| `sms-trigger` | `messaging_provider_id` (string) | No Twilio SID/token here — delegated. Optional: `from`, `persistence_key_prefix`. |
| `email-trigger` | `messaging_provider_id` (string) | No SendGrid API key here — delegated. Optional: `from`, `persistence_key_prefix`. |

### Runtime component binding corrections (Task 6)

- `world`: `greentic:provider/schema-core@1.0.0` (was placeholder `TBD-task6`)
- `sha256` digests updated to resolved pack digests (see `describe.json` `runtime.components`).
