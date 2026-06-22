# Bridge Components — `describe` Republish Checklist

- **Status:** Actionable task — quick win for the component/extension distribution RFC
- **Date:** 2026-06-22
- **Owner:** components-public maintainer / devops
- **Affected components:** `component-pack2flow`, `component-events2msg`,
  `component-msg2events`
- **Related:** greentic-designer RFC
  `docs/superpowers/specs/2026-06-22-component-extension-distribution-unification-proposal.md`
  (PR greentic-biz/greentic-designer#652)

## Symptom

Designer boot logs, for each of the three components:

```
component acquisition: describe failed; skipping
reference=oci://ghcr.io/greenticai/component/component-<name>:stable
error=missing exported descriptor instance
```

The designer pulls these at boot only to read `describe()` and ground its
flow-builder catalog. Non-fatal (baseline catalog covers them), but the
catalog stays ungrounded for these three and the WARNs are noise.

## Root cause — release drift, NOT a code defect

The **source is already correct**: each crate implements
`fn describe() -> node::ComponentDescriptor` against
`greentic_interfaces_guest::component_v0_6::node`
(= `component-descriptor@0.6.0`):

- `crates/component-pack2flow/src/lib.rs:35`
- `crates/component-events2msg/src/lib.rs:188`
- `crates/component-msg2events/src/lib.rs:186`

The problem is the **published artifact**. The only tags on
`ghcr.io/greenticai/component/component-pack2flow` (and the other two) are
`0.1.0..0.1.3 / stable / latest`, and `stable` resolves to an old build that
predates the `describe` export (verified: `stable` and `latest` have different
digests; `stable` is the older one). Contrast `component-http`, which has a
dedicated `publish-component-http.yml`, reached `1.2.0`, and describes fine.

There is **no dedicated publish workflow** for these three
(`.github/workflows/` has only `publish-component-http.yml`,
`publish-platform-extension.yml`, `publish-webhook-extension.yml`,
`publish-extension.yml`, `dev-publish.yml`). They were last published by an
older path and their `stable` tag was never moved to a `describe`-capable
build.

These three are **ref-only components** — they have no matching authoring
extension — so there is no extension to keep in lockstep here (unlike `http`).
The general lockstep design is the RFC's concern; this task is just "republish
with `describe`".

## Checklist

- [ ] **Confirm source exports `describe`** (already true — see line refs
      above). No code change expected.
- [ ] **Pick the release version.** Workspace is `1.1.0-dev.0`; `stable`
      should point at a real (non-`-dev`) release. Align with the
      `component-http` line / current release policy.
- [ ] **Build `wasm32-wasip2`** for each crate via `cargo-component`
      (`cargo component build --release -p component-<name>`).
- [ ] **Verify the artifact exports the descriptor** before publishing, e.g.
      `wasm-tools component wit <out>.wasm | grep -i 'component-descriptor@0.6.0'`
      (or instantiate and call `describe`). This is the gate that was missing.
- [ ] **Publish + move tags** to `ghcr.io/greenticai/component/component-<name>`:
      push `:<version>`, then move `:stable` (and `:latest`) to that digest,
      for all three.
- [ ] **Verify in the designer** (`GREENTIC_CHANNEL=research`, with the #652
      registry refs): boot shows **no** `describe failed` WARN for the three,
      and `component-grounded catalog built ... capabilities=N` rises from 16
      to 19.
- [ ] **Prevent recurrence:** add a per-component publish workflow mirroring
      `publish-component-http.yml` (or extend `dev-publish.yml`) so `stable`
      always tracks a `describe`-capable build for these three.

## Notes

- The designer-side registry refs were already corrected in #652
  (`packs/components/<name>:0.1.0` → `component/<name>:stable`); no further
  designer change is needed once the artifacts are republished — `:stable`
  picks them up automatically.
- Keep the standalone component-OCI publish for now; the RFC may later make the
  extension the source-of-truth, but these three have no extension today.
