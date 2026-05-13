# `component-http` Republish + Three-Tier Branching Bootstrap — Design Spec

> **Status:** draft
> **Author:** Bima
> **Date:** 2026-05-13
> **Targets:** `components-public` (primary), unblocks downstream `greentic-flow`, `greentic-designer`, `greentic-demo`
> **Parent scope:** SP-1 in the "fix semua yang belum sesuai" multi-project initiative; SP-2/SP-3/SP-4 deferred.

## Problem

WASM component `component-http` (used by flow nodes for outbound HTTP, ingressed via the AdaptiveCard pack's `api_action` node) is published at `ghcr.io/greenticai/component/component-http:0.1.0` (and `:latest` resolves there). Runner-host loads the pack and calls `describe()` on each declared component to bind exported operations. For the existing OCI artifact, `describe()` fails:

```
[ERROR] PACK_LOCK_COMPONENT_DESCRIBE_FAILED components/component-http
  - describe() failed: missing exported descriptor instance
  hint: ensure the component exports greentic:component@0.6.0
```

Source HEAD (`components-public/crates/component-http/Cargo.toml`) already targets the right world:

```toml
[package.metadata.component.target]
world = "greentic:component/component-v0-v6-v0@0.6.0"
```

The mismatch is therefore **not in source**; it is **between the source HEAD and the published OCI artifact**. The artifact at `:0.1.0` was compiled when the source still exported a pre-0.6.0 world and has not been republished since the world bump. Workspace version is still `0.1.0`, so CI never produces a higher OCI tag, so consumers never see the corrected wasm.

### Visible failure mode

Reproduced against `~/Downloads/testing (1).gtbundle` (designer-generated, May 11) running under `gtc 1.0.22`:

1. Bundle loads, WebChat WS handshake completes, home card renders correctly.
2. User clicks an action that routes into a flow node calling `component-http`.
3. `describe()` failure → node un-bindable → flow stalls before populating downstream `${api_confirm_booking.flight_booking_ref}`, `${preferred_airline}`, etc. placeholders.
4. AdaptiveCard renderer hits unbound `${var}` references → AC validation hard-fails (per `project_ac_validation_mode_bug`) → TierD empty card render.

Net symptom: "WebChat UI muncul tapi card/response salah." Multiple downstream cards stay blank or wrong.

### Why component-http was missed by the recent 1.2.0 wave

Recent commits on `components-public` bumped three **extensions** to `1.2.0` research-tier:

- `ef34024 chore(http-extension): research-tier 1.2.0 + finish wasip2 alignment`
- `6479956 chore(extensions): research-tier 1.2.0 + wasip2 across 3 extensions` (webhook-ext, platform-ext, llm-generic-ext)

Those extensions have dedicated `publish-<name>-extension.yml` workflows triggered on `<name>-ext-v*` tags. `component-http` is a **WASM component**, not an extension, and has no equivalent dedicated workflow — it is published only via `ci.yml`'s matrix using the workspace version. Without a workspace bump, the OCI tag never moves.

User expectation per existing extension policy (memory `project_extension_version_scheme`): components should be on the `1.2.x` research line. They are not.

## Goals

- Publish `component-http` to `ghcr.io/greenticai/component/component-http:1.2.0` built from current source (i.e. exporting `greentic:component@0.6.0`).
- Independent component versioning, so `component-http` can iterate without dragging every other component in `crates/` along.
- Bootstrap three-tier branching (`main` → `develop` → `research`) in `components-public` as a one-time prerequisite. `research` becomes the canonical PR target for new features going forward (per `feedback_pr_target_research_directly`).

## Non-goals

- Repackage existing `.gtpack`/`.gtbundle` artifacts in `~/Downloads/` or `greentic-demo/demos/` (→ SP-3).
- Update consumer pinning in `greentic-flow/frequent-components.json` or `greentic-designer` defaults (→ SP-2).
- Bump other components (`component-pack2flow`, `component-events2msg`, `component-bundle-standard`, etc.) to `1.2.x`. Per-component decisions land later.
- Fix the AC `validation_mode=warn` hard-fail bug (→ SP-4, owned by `greentic-adaptive-card-mcp`).
- Migrate OCI `:latest` → `:stable` (→ SP-4, devops scope per `feedback_devops_team_owns_infra`).

## Approach

Single-track end-to-end execution: **forward-port `origin/main` into `origin/develop`** (matches existing repo pattern, preserves main as stable), branch `research` from unified `develop`, land Cargo + workflow changes on a feature branch off `research`, tag, publish, validate. No parallel rebasing.

> **Direction note (revised 2026-05-13, post-Task-2 investigation):** Initial spec called for promote `develop → main` to "sync 1.1.0-dev.0 lane upward". Discovered during execution that `main` carries 2 commits (`ef34024`, `6479956`) bumping 4 extensions to research-tier `1.2.0` that were never synced to develop, while develop's `1.1.0-dev.0` workspace force-jump was never synced to main. Both lanes carry legitimate parallel innovation. Reversing the direction to forward-port (`main → develop`) preserves main's stable role (`workspace = 0.1.0`, ext at per-crate `1.2.0`), matches the existing `forward-port/main-to-develop-*` PR pattern in this repo, and yields `develop` as the unified most-current baseline — which is the natural place to branch `research` from per three-tier promote direction (research → develop → main).

### Sequence (strictly sequential)

```
Step 1.  Forward-port origin/main → origin/develop             [INFRA, maintainer review]
Step 2.  Bootstrap research branch from unified develop        [INFRA, lightweight]
Step 3.  On research:
           3a. crates/component-http/Cargo.toml:
                 version.workspace = true → version = "1.2.0"
           3b. Add .github/workflows/publish-component-http.yml
Step 4.  Open PR feat/publish-component-http-1.2.0 → research  [PROCESS]
Step 5.  After merge: push tag component-http-v1.2.0 on research [OPS]
Step 6.  CI publishes :1.2.0 to OCI → validate                 [VERIFY]
```

Promotion direction post-SP-1 (reference, not action):

```
research (innovation, lands first)  →  develop (staging)  →  main (stable)
```

Forward-port direction in Step 1 is the *one-time bootstrap inverse* of this flow — sync the historical research-tier work already on `main` down into `develop` so that `research` (created in Step 2) starts from a unified base. After SP-1, normal promotion direction takes over.

## Step-by-step details

### Step 1 — Forward-port `origin/main` → `origin/develop`

`components-public` already uses `forward-port/main-to-develop-YYYYMMDD` branches periodically. This step is a fresh forward-port that brings main's research-tier ext-v1.2.0 commits into develop, producing a unified baseline for `research` to branch from in Step 2.

```bash
cd components-public
git fetch --all
git checkout -b forward-port/main-to-develop-$(date +%Y%m%d) origin/develop
git merge origin/main                    # resolve conflicts inline
git push origin HEAD
gh pr create --base develop \
  --head forward-port/main-to-develop-$(date +%Y%m%d) \
  --title "forward-port: main → develop (sync research-tier ext bumps)" \
  --body "Sync main into develop before bootstrapping research branch for three-tier branching pilot. Forward-ports the 1.2.0 ext-tier work on main into develop's 1.1.0-dev.0 baseline. Refs: project_three_tier_branching, project_research_promote_cadence."
```

**Conflict expectations.** Cargo.lock + per-crate Cargo.toml for `http-extension`, `platform-extension`, `llm-generic-extension`, `webhook-extension`. Each will carry the same disagreement: main shows `version = "1.2.0"` (research-tier release), develop shows `version = "1.1.0-dev.0"` (force-jump). Resolution: take **main's `1.2.0`** on each extension Cargo.toml conflict (preserves the published release semantics). Keep develop's `workspace.package.version = "1.1.0-dev.0"` on the root `Cargo.toml`. For Cargo.lock: take main's version (with `--theirs` from the merge POV) then `cargo update --workspace` to reconcile. Net effect: develop ends up with `workspace = 1.1.0-dev.0` + per-crate ext `1.2.0`, which is exactly the parallel-version model SP-1's component-http change extends to a new crate.

**Done criteria (revised D1).** `develop` HEAD contains both `origin/main`'s history and develop's prior commits; CI green on the PR; maintainer approves the merge.

### Step 2 — Bootstrap `research` branch

```bash
git fetch origin
git checkout -b research origin/develop
git push -u origin research
```

Optional follow-up out-of-scope for SP-1: update repo settings so `research` is the default base for new feature PRs (devops policy decision).

**Done criteria (revised D2).** `origin/research` exists, points at the same commit as `origin/develop` immediately after the Step 1 forward-port PR merged.

### Step 3 — Code changes on research

#### 3a. Per-crate version override

```bash
git checkout -b feat/publish-component-http-1.2.0 origin/research
```

`crates/component-http/Cargo.toml`:

```diff
 [package]
 name = "component-http"
-version.workspace = true
+version = "1.2.0"
 edition.workspace = true
 license.workspace = true
```

Then `cargo update -p component-http` to refresh Cargo.lock entries that reference the new version.

#### 3b. New publish workflow

Add `.github/workflows/publish-component-http.yml`, modelled on `publish-webhook-extension.yml`.

**Implementation approach: prefer reuse over hand-roll.** The repo already has `dev-publish.yml` calling `greenticai/.github/.github/workflows/wasm-component-ci.yml@main`. The first writing-plans task is to read that reusable workflow, confirm it can emit a single `component_http.wasm` artifact and whether it integrates `oras push` or only builds. Depending on that read:

- **If reusable workflow handles publish end-to-end:** the new file is thin — just `uses:` the reusable workflow with `wasm-build-args: "--target wasm32-wasip2 -p component-http"` plus the tag-derived version.
- **If reusable workflow only builds:** call the reusable workflow for build, then run the explicit `oras login` + `oras push` steps shown below in the same job.

The YAML below is the **hand-roll shape** — concrete fallback when the reusable workflow does not cover publish. The reuse path is the preferred outcome but its exact `with:` block depends on the reusable workflow's input contract, which is verified at writing-plans time.

```yaml
name: Publish Component HTTP
on:
  push:
    tags: ['component-http-v*']
  workflow_dispatch:
    inputs:
      version:
        description: 'Component version (must match Cargo.toml)'
        required: true

permissions:
  contents: read
  packages: write

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Derive version from tag
        id: ver
        shell: bash
        run: |
          set -euo pipefail
          if [ "${{ github.event_name }}" = "workflow_dispatch" ]; then
            VERSION="${{ inputs.version }}"
          else
            VERSION="${GITHUB_REF_NAME#component-http-v}"
          fi
          echo "version=${VERSION}" >> "$GITHUB_OUTPUT"
      # Build via the canonical reusable workflow:
      # uses: greenticai/.github/.github/workflows/wasm-component-ci.yml@main
      # with:
      #   wasm-build-args: "--target wasm32-wasip2 -p component-http"
      #   validate-component: false
      # (exact inputs to be confirmed by reading the reusable workflow during writing-plans)
      #
      # Then push the produced artifact:
      - uses: oras-project/setup-oras@v1
      - run: echo "${{ secrets.GITHUB_TOKEN }}" | oras login ghcr.io -u ${{ github.actor }} --password-stdin
      - run: |
          oras push ghcr.io/greenticai/component/component-http:${{ steps.ver.outputs.version }} \
            target/wasm32-wasip2/release/component_http.wasm:application/wasm
```

**Confirmed at writing-plans time:** exact inputs of `wasm-component-ci.yml@main`, whether it emits artifacts in the expected directory, and whether `oras push` is already integrated. If integrated, the standalone oras steps drop out.

#### 3c. Commit and push

```bash
git add crates/component-http/Cargo.toml Cargo.lock \
        .github/workflows/publish-component-http.yml
git commit -m "feat(component-http): bump 1.2.0 + dedicated publish workflow"
git push -u origin feat/publish-component-http-1.2.0
```

**Done criteria.** Branch pushed; `cargo build -p component-http --target wasm32-wasip2 --release` succeeds locally; PR CI green.

### Step 4 — Open PR `feat/publish-component-http-1.2.0` → research

```bash
gh pr create \
  --base research \
  --head feat/publish-component-http-1.2.0 \
  --title "feat(component-http): bump 1.2.0 + dedicated publish workflow" \
  --body "$(cat <<'EOF'
## Why

Published OCI artifact at `ghcr.io/greenticai/component/component-http:0.1.0` exports a pre-0.6.0 world; runner-host expects `greentic:component@0.6.0`. `describe()` fails on every pack that references this component. Reproduced via `~/Downloads/testing (1).gtbundle` + `gtc start`, observable as `PACK_LOCK_COMPONENT_DESCRIBE_FAILED components/component-http` in `greentic-pack doctor` output.

## What

- Pin `component-http` to `1.2.0` via per-crate override of workspace version.
- Add `.github/workflows/publish-component-http.yml` triggered on `component-http-v*` tag, mirroring the extension publish pattern.

## Test plan

- [ ] Local: `cargo build -p component-http --target wasm32-wasip2 --release` OK
- [ ] PR CI green
- [ ] Post-merge: push `component-http-v1.2.0` tag on research → workflow run SUCCESS
- [ ] `oras pull ghcr.io/greenticai/component/component-http:1.2.0` returns an artifact
- [ ] `wasm-tools component wit` reports `package greentic:component@0.6.0`
- [ ] `tools/verify-component-http-publish.sh 1.2.0` passes

## References

`project_3point_escalation`, `project_extension_version_scheme`, `project_oci_stable_migration`.
EOF
)"
```

**Done criteria.** PR merged into `research`.

### Step 5 — Tag and trigger publish

```bash
git checkout research && git pull
git tag component-http-v1.2.0 <merge-commit-sha>
git push origin component-http-v1.2.0
```

**Permissions.** Requires write access to the repo (specifically tag push). If the implementer lacks it, a maintainer pushes the tag.

**Done criteria.** Tag `component-http-v1.2.0` listed in `git ls-remote origin --tags`; `publish-component-http.yml` workflow run starts within minutes of the tag push.

### Step 6 — Validate published artifact

Three sub-checks, in order:

**6a — Workflow success.**

```bash
gh run list -w "Publish Component HTTP" --limit 1
gh run watch <run-id>
```

Expected: `completed success`.

**6b — Artifact correctness.**

```bash
mkdir -p /tmp/comp-http-1.2.0
oras pull ghcr.io/greenticai/component/component-http:1.2.0 -o /tmp/comp-http-1.2.0
wasm-tools component wit /tmp/comp-http-1.2.0/component_http.wasm | grep "package greentic:component@0.6.0"
wasm-tools validate /tmp/comp-http-1.2.0/component_http.wasm
```

Expected: the `grep` matches; `validate` exits 0.

**6c — End-to-end smoke (script).**

Build a minimal test pack that references `component-http:1.2.0`, run it through doctor and `gtc start`:

```bash
bash tools/verify-component-http-publish.sh 1.2.0
# internally:
#   construct a minimal pack with one flow node calling component-http:1.2.0
#   greentic-pack doctor --pack <pack>          # MUST NOT contain PACK_LOCK_COMPONENT_DESCRIBE_FAILED
#   gtc start <bundle>                          # MUST reach "Ready"
```

The script lives at `tools/verify-component-http-publish.sh` and is idempotent. Concrete pack/bundle construction is a writing-plans deliverable.

**Done criteria.** All three sub-checks pass.

## Testing strategy

Four layers, only the first three apply during SP-1; Layer 4 is the formal automation deliverable.

| Layer | When | What | Tool |
|---|---|---|---|
| 1 | Pre-merge on PR to research | fmt, clippy, test, wasm build | existing `ci.yml`, `branch-invariants.yml` |
| 2 | Post-tag | Publish workflow run executes to completion | `gh run watch` |
| 3 | Post-publish | Artifact pulls; exported world matches `greentic:component@0.6.0` | `oras pull`, `wasm-tools component wit` |
| 4 | Post-publish (scripted) | Test pack `greentic-pack doctor` clean; `gtc start` reaches Ready | `tools/verify-component-http-publish.sh` |

**Likely failure modes and mitigations.**

- *Cargo build fails due to rust-toolchain mismatch.* CI currently uses 1.91–1.94 across jobs. Do not override toolchain in the new workflow; defer to `rust-toolchain.toml`. Reconcile in repo defaults if necessary.
- *`oras login` fails.* `GITHUB_TOKEN` must carry `packages:write`. The workflow declares `permissions: { packages: write }`; verify token scope during PR review.
- *Reusable workflow inputs drift.* If `wasm-component-ci.yml@main` changes shape, the new workflow's `with:` block needs updating in writing-plans. Pinning the reusable workflow to a SHA rather than `@main` is a safer long-term option; out of scope for SP-1.

**Anti-regression check.** Between Step 6 success and SP-2/SP-3 kickoff, capture the OCI manifest digest for at least two other components (`component-pack2flow`, `component-events2msg`) via `oras manifest fetch ...:latest`. After SP-1 is closed, capture again and confirm no unexpected digest change. Concretely scripted in `tools/verify-component-http-publish.sh` as an optional regression step. Flag if digests differ.

## Rollout to downstream (out-of-scope, reference only)

SP-2, SP-3, SP-4 all gate on SP-1 done. Their changes are not made under this spec.

| Sub-project | Action | Repo |
|---|---|---|
| SP-2 | `frequent-components.json`: `oci://.../component-http:latest` → `:1.2.0` (or `:stable` once SP-4 ready) | `greentic-flow` |
| SP-2 | Designer auto-pin defaults updated to `:1.2.0` (if hardcoded) | `greentic-designer` |
| SP-3 | After SP-2 merges: `bash scripts/package_demos.sh` (single CI run) regenerates all 13 demo bundles | `greentic-demo` |
| SP-3 | Spot-check rebuilt bundles: `gtc start demos/telco-x-demo.gtbundle` etc. reach Ready | `greentic-demo` |
| SP-4 | Migrate `:latest` → `:stable` tag pointer; CI policy for republish-on-world-bump | devops |

## Backout and rollback

OCI tags are treated as immutable (OCI norm). If `:1.2.0` proves broken after publish:

- **Forward-only.** Push a patch tag `component-http-v1.2.1` with the fix. Consumers re-pin. Do not delete `:1.2.0`.

If Step 1 forward-port destabilises develop:

- Revert via `gh pr create --base develop --head revert/forward-port-main-to-develop-X`.
- If `research` has already been branched off the post-forward-port commit, rebase it onto the reverted develop HEAD.

If the new `publish-component-http.yml` workflow itself misbehaves post-merge:

- Revert that single file via PR. The tag remains but a tag-trigger fires only once on tag push, so a revert is safe; subsequent `workflow_dispatch` invocations will use the reverted workflow.

## Done criteria (SP-1 closed)

| # | Check | Verification |
|---|---|---|
| D1 | `develop` HEAD contains main's history post forward-port | `git merge-base --is-ancestor origin/main origin/develop && echo OK` |
| D2 | `research` branch exists at origin, equals `develop` HEAD at branch time | `git ls-remote origin research` and `git ls-remote origin develop` agree |
| D3 | `crates/component-http/Cargo.toml` has `version = "1.2.0"` on `research` | file diff |
| D4 | `.github/workflows/publish-component-http.yml` exists on `research` | file presence |
| D5 | Tag `component-http-v1.2.0` pushed | `git ls-remote origin --tags \| grep component-http-v1.2.0` |
| D6 | Publish workflow run reports SUCCESS | `gh run list -w "Publish Component HTTP" --limit 1` |
| D7 | OCI artifact pullable at `:1.2.0` | `oras pull` |
| D8 | Exported world is `greentic:component@0.6.0` | `wasm-tools component wit` |
| D9 | Test pack loads cleanly via doctor and reaches Ready in `gtc start` | `tools/verify-component-http-publish.sh 1.2.0` |

All nine must pass. When they do, SP-2 and SP-3 may begin.

## References

### Source-of-truth (verified during investigation, 2026-05-13)

- `crates/component-http/Cargo.toml`: `version.workspace = true`; world target `greentic:component/component-v0-v6-v0@0.6.0`.
- `Cargo.toml` (workspace) on `main`: `version = "0.1.0"`.
- `Cargo.toml` (workspace) on `origin/develop`: `1.1.0-dev.0` (`62f6b29 chore: force-jump dev lane to 1.1.0-dev.0 (#32)`).
- Existing publish patterns: `publish-webhook-extension.yml`, `publish-platform-extension.yml`, `publish-llm-generic-extension.yml`, all triggered on `<name>-ext-v*` tags.
- Reusable workflow used by `dev-publish.yml`: `greenticai/.github/.github/workflows/wasm-component-ci.yml@main`.
- Existing `ci.yml` component matrix already publishes `component-http`, `component-pack2flow`, `component-events2msg` to `ghcr.io/greenticai/component/<name>` (version derived from workspace package version).
- Reproduction artifact: `~/Downloads/testing (1).gtbundle` (squashfs, May 11 2026), pack `testing.gtpack` lists `component-http:0.1.0`. `greentic-pack doctor` emits `PACK_LOCK_COMPONENT_DESCRIBE_FAILED components/component-http`.

### Memory references

- `project_3point_escalation` — active P0; 3Point/Paul cannot run greentic-demo. SP-1 unblocks SP-3 which is the fix for this P0.
- `project_extension_version_scheme` — `1.2.x` is the research line for extensions; this spec extends the convention to components.
- `project_three_tier_branching` — pilot in `greentic-bundle`, propagating to tier-3+ repos. SP-1 includes the bootstrap for `components-public`.
- `project_research_promote_cadence` — 2-weekly promote cadence applies post-SP-1.
- `feedback_pr_target_research_directly` — once `research` exists, future feat PRs target it.
- `feedback_devops_team_owns_infra` — Step 1 (forward-port) flagged for maintainer review.
- `feedback_runner_host_naming` — clarifies that the user's hunch ("bundle-extensions vs runner/flow") referred semantically to component-WIT version skew, even though the named `greentic-bundle-extensions` repo is not on the data path.
- `feedback_no_invented_names_in_public` — every commit SHA, version, file path in this spec is verified against the current workspace, not assumed.
- `project_oci_stable_migration` — `:latest → :stable` migration is the SP-4 follow-up; this spec sticks with explicit `:1.2.0`.
- `project_ac_validation_mode_bug` — explains the visible end-user symptom (TierD empty render) downstream of the `describe()` failure; not fixed here.
