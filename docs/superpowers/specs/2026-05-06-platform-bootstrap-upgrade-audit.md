# Platform-Bootstrap Upgrade Audit

> **Status:** finding + recommended fix
> **Date:** 2026-05-06
> **Targets:** `greentic-designer`, `greentic-bundle-extensions` (CLI), release notes
> **Spawned by:** webhook-extension split (#35) and llm-generic-extension split (#36)

## Problem

Splitting `trigger` and `llm` out of `platform-extension` creates a node-registry conflict for users who already have `greentic.platform-bootstrap-0.1.0` installed under `~/.greentic/extensions/design/`. The splits make `platform-extension` 0.3.0 ship only `start` while `greentic.webhook` 0.1.0 owns `trigger` and `greentic.llm-generic` 0.1.0 owns `llm`. After the bundled refresh the user ends up with **all four** extensions installed simultaneously, and designer's first-wins type_id deduplication picks asymmetrically.

## How designer registers nodeTypes

`greentic_ext_runtime::discovery::scan_kind_dir` reads `~/.greentic/extensions/design/`, sorts directory entries alphabetically, and returns paths in that order. Designer's `NodeTypeRegistry::register` (in `src/ui/node_registry.rs`) is **first-wins**: when an extension tries to register a `type_id` that another extension already claimed, the new one is logged as a warning and ignored.

## Concrete user state after upgrade

A user upgrading from designer with bundled `platform-bootstrap-0.1.0` to bundled `platform-bootstrap-0.3.0 + webhook-0.1.0 + llm-generic-0.1.0` will see all four extensions on disk because the bundled unpack policy in `src/ui/bundled.rs::install_all` skips any target dir that already exists — it never overwrites or removes the legacy version. So `~/.greentic/extensions/design/` ends up containing:

```
greentic.adaptive-cards-1.6.4/
greentic.http-0.1.3/
greentic.llm-generic-0.1.0/         ← new
greentic.platform-bootstrap-0.1.0/  ← legacy, still here
greentic.platform-bootstrap-0.3.0/  ← new
greentic.webhook-0.1.0/             ← new
```

Sorted alphabetically + first-wins registration order:

| Extension | Order | type_ids it tries to register | Outcome |
|---|---|---|---|
| `adaptive-cards-1.6.4` | 1 | `adaptive-card`, `component`, `condition` | wins all three |
| `http-0.1.3` | 2 | `http` | wins |
| `llm-generic-0.1.0` | 3 | `llm` | **wins** (new schema) ✅ |
| `platform-bootstrap-0.1.0` | 4 | `start`, `trigger`, `llm` | `start` wins; `trigger` **wins** (legacy schema) ❌; `llm` already taken |
| `platform-bootstrap-0.3.0` | 5 | `start` | already taken |
| `webhook-0.1.0` | 6 | `trigger` | already taken — **stale wins** ❌ |

**Asymmetric outcome:**
- `llm` resolves to the new `greentic.llm-generic` descriptor ✅ — alphabetically `llm-generic` precedes `platform-bootstrap`, so it registers first.
- `trigger` resolves to the **legacy** `greentic.platform-bootstrap-0.1.0` descriptor ❌ — alphabetically `platform-bootstrap` precedes `webhook`. Inspector renders the old-shape config schema (`auth: bool`) instead of the new one (`auth: object` with `type` enum + `secret_ref`).

The bug is silent: no warnings reach the user, the inspector form just looks "wrong" relative to the published schema.

## Why the bundled-fallback policy doesn't fix it

`install_all` is intentionally conservative: it only writes to disk when the target dir is absent. That keeps user-installed extensions (via `gtdx install`) from being clobbered by the bundled fallback. The cost of that guarantee is that legacy bundled versions linger forever once a user has unpacked them once.

## Fix options (ranked)

### A. Designer: replace first-wins with version-aware precedence on duplicate `type_id` (recommended)

Change `NodeTypeRegistry::register` so that when the same `type_id` is offered by multiple extensions, the descriptor sourced from the extension with the **higher version** wins. Tie-breaker: prefer the extension whose `metadata.id` does NOT match a legacy bootstrap (i.e. prefer `greentic.webhook` over `greentic.platform-bootstrap` when both ship the same trigger node).

This fixes the upgrade path for every user without requiring CLI action. It also future-proofs further splits (e.g. provider-specific webhook variants).

Implementation cost: ~30 lines in `src/ui/node_registry.rs`, plus tests for the precedence rule.

### B. Designer: self-cleanup of superseded `platform-bootstrap` versions on boot

When designer boots and finds both `greentic.platform-bootstrap-X.Y.Z/` and `greentic.platform-bootstrap-A.B.C/` installed, delete the older directory if A.B.C ≥ 0.2.0 (the cutover release where node types started moving out). Fixes the conflict at source by removing the legacy descriptor entirely.

Risk: silent deletion of user-installed extensions feels surprising. Mitigation: log a one-line info message ("removing superseded bootstrap version 0.1.0 in favour of 0.3.0").

### C. Document a manual cleanup step in the release notes

Tell users to run:

```bash
gtdx remove greentic.platform-bootstrap@0.1.0
```

after upgrading. Lowest implementation cost; highest user-action cost. Some users will skip it, miss the asymmetric bug, file confused issues.

### D. Ship a one-shot migration command

Add `gtdx migrate-platform-bootstrap` (or similar) to `greentic-bundle-extensions/greentic-ext-cli`. Runs the cleanup logic once, idempotently. Discoverable and explicit, but requires users to know to run it.

### E. Last-wins instead of first-wins

Flip the registry semantics. Loading order then dictates winners — `webhook` would win because it loads after `platform-bootstrap-0.1.0`, and `llm-generic` would still win because it loads before `platform-bootstrap-0.1.0` (which never had `llm` in its 0.3.0 form anyway). But last-wins inverts a contract some other extension authors may already rely on. Risky to flip universally.

## Recommendation

Combine **A + C** for this release cycle:

1. Land option A (version-aware precedence) in the next designer minor. Same PR adds release notes.
2. Add the manual cleanup tip from C as a **belt-and-braces** step in the changelog. Users who never read changelog still get the right behavior because of A.

Defer B and D — both require more design work (silent deletion policy, CLI surface). They become unnecessary if A lands.

## Acceptance criteria

- [ ] `NodeTypeRegistry::register` prefers higher-version extensions on duplicate `type_id`
- [ ] Test: register `trigger` from a v0.1.0 extension, then v0.2.0, then v0.1.0 again — final winner is v0.2.0
- [ ] Test: existing flows with `type: trigger` continue to resolve a renderer
- [ ] Release notes mention the optional `gtdx remove greentic.platform-bootstrap@0.1.0` cleanup
