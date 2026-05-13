# SP-1 `component-http` Republish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish `component-http` to `ghcr.io/greenticai/component/component-http:1.2.0` built from current source (which exports `greentic:component@0.6.0`), and bootstrap three-tier branching (`main` → `develop` → `research`) in `components-public` as a one-time prerequisite.

**Architecture:** Single-track execution. Forward-port `origin/develop` into `origin/main` (gated by maintainer review), branch `research` from the unified base, land Cargo + workflow + verification-script changes on a feature branch off `research`, tag, publish, validate. No rebasing or parallel paths.

**Tech Stack:** Rust 1.94 / Cargo workspace, `cargo-component` for WASM build, `wasm32-wasip2` target, `oras` for OCI push, GitHub Actions (reusable workflow `greenticai/.github/.github/workflows/wasm-component-ci.yml@main` or hand-rolled fallback), `wasm-tools` for artifact inspection.

**Spec reference:** `components-public/docs/superpowers/specs/2026-05-13-component-http-republish-design.md`

**Working directory throughout:** `/home/bima-pangestu/Works/greentic/components-public/` (or its worktree).

---

## Pre-flight: Set up isolated worktree

### Task 0: Create worktree

**Files:** none yet (worktree creation only).

- [ ] **Step 1: Confirm clean state in source repo**

Run from `/home/bima-pangestu/Works/greentic/components-public`:

```bash
git status --porcelain
```

Expected: empty output (no uncommitted changes).
If non-empty: stash or commit existing work first; do NOT proceed.

- [ ] **Step 2: Fetch latest refs**

```bash
git fetch --all --prune
```

Expected: fetch output without errors. Confirms `origin/main`, `origin/develop` are current.

- [ ] **Step 3: Create worktree off `origin/main`**

```bash
WORKTREE=/tmp/sp1-component-http-$(date +%Y%m%d-%H%M%S)
git worktree add "$WORKTREE" origin/main
cd "$WORKTREE"
git status
```

Expected:
```
HEAD detached at origin/main
nothing to commit, working tree clean
```

- [ ] **Step 4: Record worktree path**

```bash
echo "$WORKTREE" > /tmp/sp1-worktree-path
cat /tmp/sp1-worktree-path
```

Subsequent tasks operate in `$WORKTREE`. Each task's first step assumes `cd "$(cat /tmp/sp1-worktree-path)"`.

- [ ] **Step 5: Verify tooling installed**

```bash
which gh git cargo cargo-component oras wasm-tools jq
```

Expected: all six paths print. If `wasm-tools` missing: `cargo install wasm-tools-cli`. If `oras` missing: install per https://oras.land/docs/installation. If `cargo-component` missing: `cargo install cargo-component --version 0.18.0`.

---

## Phase A: Three-tier bootstrap (Tasks 1-3)

### Task 1: Read reusable workflow contract before writing anything

**Why first:** The new `publish-component-http.yml` calls (or replaces) the existing reusable workflow `greenticai/.github/.github/workflows/wasm-component-ci.yml@main`. Its input contract determines whether the new workflow is thin (delegate) or fat (hand-roll). Need to read it before writing YAML.

**Files:**
- Read-only: `greenticai/.github/.github/workflows/wasm-component-ci.yml@main` (external repo)

- [ ] **Step 1: Fetch the reusable workflow source**

```bash
gh api /repos/greenticai/.github/contents/.github/workflows/wasm-component-ci.yml \
  --jq '.content' \
  | base64 -d > /tmp/wasm-component-ci.yml
cat /tmp/wasm-component-ci.yml
```

Expected: YAML printed to stdout. If 404: the workflow may live at a different path; check `gh api /repos/greenticai/.github/contents/.github/workflows`.

- [ ] **Step 2: Identify inputs, outputs, and publish behavior**

Note in `/tmp/sp1-reusable-workflow-notes.md`:

```bash
cat > /tmp/sp1-reusable-workflow-notes.md <<EOF
# wasm-component-ci.yml@main contract

## Inputs (from \`on.workflow_call.inputs\`)
<paste the inputs block here>

## Does it push to OCI?
<grep for "oras push" / "ghcr.io" in the file; record yes/no>

## Output artifact path
<record where it places the .wasm — e.g. target/wasm32-wasip2/release/>

## Decision
- If publishes: new workflow is thin delegate, no separate oras step
- If only builds: new workflow delegates build, then runs oras login + push
EOF
cat /tmp/sp1-reusable-workflow-notes.md
```

This drives Task 7's YAML shape.

- [ ] **Step 3: Commit the decision in writing**

No git commit here — Task 1 produces no repo changes, only a `/tmp/` note used by Task 7.

### Task 2: Promote `origin/develop` → `origin/main`

**Files:**
- Modified: any file diverging between `develop` and `main` (resolved during merge)
- Likely conflicts: `Cargo.lock`, `Cargo.toml` (workspace version), nightly dependabot bumps

- [ ] **Step 1: Inspect divergence before merging**

```bash
cd "$(cat /tmp/sp1-worktree-path)"
echo "=== develop ahead of main ==="
git log --oneline origin/main..origin/develop
echo "=== main ahead of develop ==="
git log --oneline origin/develop..origin/main
```

Expected: develop ahead-list has commits including `62f6b29 chore: force-jump dev lane to 1.1.0-dev.0 (#32)`; main ahead-list may be empty or carry recent extension tags.

Record commits to a note for the PR body:

```bash
git log --oneline origin/main..origin/develop > /tmp/sp1-promote-commits.txt
wc -l /tmp/sp1-promote-commits.txt
```

- [ ] **Step 2: Create promote branch**

```bash
BRANCH="promote/develop-to-main-$(date +%Y%m%d)"
git checkout -b "$BRANCH" origin/main
echo "$BRANCH" > /tmp/sp1-promote-branch
git branch --show-current
```

Expected: prints `promote/develop-to-main-20260513` (or current date).

- [ ] **Step 3: Merge develop with explicit conflict expectation**

```bash
git merge origin/develop --no-ff -m "promote: develop → main (1.1.0-dev.0 lane → stable)"
```

Two outcomes:

- **Clean merge:** proceed to Step 5.
- **Conflicts:** proceed to Step 4 to resolve.

- [ ] **Step 4: Resolve conflicts (only if Step 3 reported conflicts)**

```bash
git status --short | grep '^UU\|^AA\|^DD'
```

For `Cargo.lock` conflicts:

```bash
# Take develop's version (dev-lane stamping is the more recent state)
git checkout --theirs Cargo.lock
cargo update --workspace --offline 2>/dev/null || cargo update --workspace
git add Cargo.lock
```

For `Cargo.toml` workspace version conflicts (likely between `0.1.0` on main and `1.1.0-dev.0` on develop):

Edit `Cargo.toml` manually so `[workspace.package].version = "1.1.0-dev.0"` (take develop's value). Then:

```bash
git add Cargo.toml
```

For source-level conflicts: inspect each with `git diff --conflict diff3 <file>`, resolve preserving develop's intent unless main has a hotfix not present on develop (check via `git log origin/main -- <file>`).

After all conflicts resolved:

```bash
git status --short
```

Expected: no `^U`-prefixed lines. Then continue the merge:

```bash
git commit --no-edit
```

- [ ] **Step 5: Run local CI before pushing**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked 2>&1 | tail -20
```

Expected: all three commands exit 0. If `cargo test` is slow or fails on network-dependent tests, run `cargo test --workspace --locked --offline` to confirm the failure is environmental, not logical.

If clippy or tests fail with errors that exist on `origin/develop` HEAD too (verified by checking out `origin/develop` separately and running same), the failure is pre-existing — note it in the PR body but proceed.

- [ ] **Step 6: Push promote branch**

```bash
git push -u origin "$BRANCH"
```

Expected: success; remote prints PR creation URL.

- [ ] **Step 7: Open PR `<promote-branch>` → main**

```bash
gh pr create \
  --base main \
  --head "$BRANCH" \
  --title "promote: develop → main (1.1.0-dev.0 lane → stable)" \
  --body "$(cat <<EOF
## Why

Prerequisite for SP-1: bootstrap three-tier branching (\`main\` → \`develop\` → \`research\`) in this repo. Need \`main\` and \`develop\` aligned before branching \`research\` off the unified base.

Per memory \`project_three_tier_branching\` (pilot in greentic-bundle, propagating to tier-3+ repos) and \`project_research_promote_cadence\` (research → develop → main promotion is the canonical direction; this one-time inverse merge unblocks the bootstrap).

## What

Merges \`origin/develop\` into \`origin/main\` via a no-ff merge commit. Commits being promoted:

\`\`\`
$(cat /tmp/sp1-promote-commits.txt)
\`\`\`

## Test plan

- [x] \`cargo fmt --all -- --check\` passes locally
- [x] \`cargo clippy --workspace --all-targets -- -D warnings\` passes locally
- [x] \`cargo test --workspace --locked\` passes locally (or pre-existing failures documented)
- [ ] CI green on this PR

## Risk

Cargo.lock and workspace version conflicts resolved by taking develop's (more recent) state. No code-level semantic changes — develop has been forward-port-synced from main periodically.

## References

\`project_three_tier_branching\`, \`project_research_promote_cadence\`, \`feedback_devops_team_owns_infra\` (maintainer review explicitly requested).
EOF
)"
```

Expected: PR URL printed. Record it:

```bash
gh pr view --json url -q .url > /tmp/sp1-promote-pr-url
cat /tmp/sp1-promote-pr-url
```

- [ ] **Step 8: Gate — wait for maintainer review and merge**

This step does not auto-complete. The implementer (or supervising session) waits for:

1. CI to report green on the PR
2. Maintainer review approval
3. PR merge

To check status programmatically without polling:

```bash
gh pr view "$(cat /tmp/sp1-promote-pr-url)" --json state,mergeable,reviewDecision,statusCheckRollup
```

Expected for proceed: `state: MERGED`. If `state: OPEN` or `CLOSED` (without merge), this plan is blocked at Task 2 until resolved.

**Do NOT proceed to Task 3 until merged.**

### Task 3: Bootstrap `research` branch

**Files:** none modified; branch creation only.

- [ ] **Step 1: Fetch the merged main**

```bash
cd "$(cat /tmp/sp1-worktree-path)"
git fetch origin
git log origin/main --oneline -n 3
```

Expected: top commit is the promote merge commit from Task 2.

- [ ] **Step 2: Create research branch from current `origin/main`**

```bash
git checkout -b research origin/main
git log --oneline -n 1
```

Expected: HEAD points at the same commit as `origin/main`.

- [ ] **Step 3: Push research to origin**

```bash
git push -u origin research
```

Expected: `[new branch] research -> research`.

- [ ] **Step 4: Verify**

```bash
git ls-remote origin research
git ls-remote origin main
```

Expected: both commands print the same SHA. **Done criteria D2 satisfied here.**

---

## Phase B: Code changes on research (Tasks 4-8)

### Task 4: Create feature branch off research

**Files:** none modified; branch creation only.

- [ ] **Step 1: Create the feature branch**

```bash
cd "$(cat /tmp/sp1-worktree-path)"
git fetch origin
git checkout -b feat/publish-component-http-1.2.0 origin/research
git branch --show-current
```

Expected: prints `feat/publish-component-http-1.2.0`.

### Task 5: Bump `component-http` to 1.2.0 (per-crate override)

**Files:**
- Modify: `crates/component-http/Cargo.toml:3` (one line change)
- Modify: `Cargo.lock` (auto-regenerated)

- [ ] **Step 1: Read the current Cargo.toml to confirm starting state**

```bash
head -15 crates/component-http/Cargo.toml
```

Expected: line 3 is `version.workspace = true`.

- [ ] **Step 2: Apply the version override**

Edit `crates/component-http/Cargo.toml`. Replace line 3:

```diff
 [package]
 name = "component-http"
-version.workspace = true
+version = "1.2.0"
 edition.workspace = true
 license.workspace = true
```

Verify the edit:

```bash
grep -n '^version' crates/component-http/Cargo.toml
```

Expected: `3:version = "1.2.0"`.

- [ ] **Step 3: Regenerate Cargo.lock entries**

```bash
cargo update -p component-http
grep -A 2 '^name = "component-http"$' Cargo.lock | head -10
```

Expected: `version = "1.2.0"` appears in the matched block.

- [ ] **Step 4: Build to confirm no breakage**

```bash
cargo build -p component-http --target wasm32-wasip2 --release 2>&1 | tail -20
```

Expected: `Compiling component-http v1.2.0 ...` then `Finished release [optimized]`. Build success.

If the target is not installed: `rustup target add wasm32-wasip2` then retry.

- [ ] **Step 5: Verify output wasm file exists**

```bash
ls -la target/wasm32-wasip2/release/component_http.wasm
```

Expected: file present, non-zero size.

- [ ] **Step 6: Inspect exported world on the freshly built artifact**

```bash
wasm-tools component wit target/wasm32-wasip2/release/component_http.wasm \
  | grep -E 'package greentic:component'
```

Expected: prints `package greentic:component@0.6.0` (confirms source already on 0.6.0 — sanity check for the spec's premise).

- [ ] **Step 7: Commit**

```bash
git add crates/component-http/Cargo.toml Cargo.lock
git commit -m "feat(component-http): bump to 1.2.0 via per-crate version override

Pins component-http independently of workspace version so future
bumps to this WASM component don't drag every other crate. Aligns
with extension publish policy (1.2.x research line, per
project_extension_version_scheme) and unblocks the dedicated publish
workflow added in the next commit."
```

### Task 6: Add `publish-component-http.yml` workflow

**Files:**
- Create: `.github/workflows/publish-component-http.yml`

**Branching based on Task 1 reusable-workflow read:**

If `wasm-component-ci.yml@main` publishes to OCI end-to-end, use the **thin delegate** variant (Step 1A). Otherwise, use the **hand-rolled** variant (Step 1B). Pick ONE.

- [ ] **Step 1A: Thin delegate (use ONLY if reusable workflow publishes to OCI)**

Create `.github/workflows/publish-component-http.yml` with content:

```yaml
name: Publish Component HTTP

# Publishes ghcr.io/greenticai/component/component-http on `component-http-v*` tag pushes.
# Tag pattern is distinct so component-http ships independently from other components
# under crates/. Version published = tag name with the leading `component-http-v` stripped.

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
    uses: greenticai/.github/.github/workflows/wasm-component-ci.yml@main
    with:
      wasm-build-args: "--target wasm32-wasip2 -p component-http"
      validate-component: false
      # The four inputs below (component-name, publish, version-source, tag-prefix) are
      # *guesses* based on common reusable-workflow conventions. The authoritative input
      # names live in /tmp/sp1-reusable-workflow-notes.md (filled by Task 1 Step 2).
      # Before committing this file, rename any input here that does not appear in that
      # notes file to the actual input name. If the reusable workflow exposes no
      # equivalent of one of these inputs, drop the line — do not invent inputs.
      component-name: component-http
      publish: true
      version-source: tag
      tag-prefix: component-http-v
    secrets: inherit
```

Before committing this file, cross-check every input against `/tmp/sp1-reusable-workflow-notes.md`. After push, run `gh workflow view publish-component-http.yml --yaml` and confirm GitHub parses it without warnings about unknown inputs.

- [ ] **Step 1B: Hand-rolled (use ONLY if reusable workflow does not publish)**

Create `.github/workflows/publish-component-http.yml` with content:

```yaml
name: Publish Component HTTP

# Publishes ghcr.io/greenticai/component/component-http on `component-http-v*` tag pushes.
# Tag pattern is distinct so component-http ships independently from other components
# under crates/. Version published = tag name with the leading `component-http-v` stripped.

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
          echo "==> Publishing component-http@${VERSION}"
          echo "version=${VERSION}" >> "$GITHUB_OUTPUT"

      - name: Install Rust toolchain (defer to repo rust-toolchain.toml)
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-wasip2

      - name: Install cargo-component
        run: cargo install cargo-component --version 0.18.0 --locked

      - name: Build component
        run: cargo component build -p component-http --release --target wasm32-wasip2 --locked

      - name: Setup oras
        uses: oras-project/setup-oras@v1

      - name: Login to GHCR
        run: echo "${{ secrets.GITHUB_TOKEN }}" | oras login ghcr.io -u ${{ github.actor }} --password-stdin

      - name: Push component to OCI
        run: |
          set -euo pipefail
          ARTIFACT="target/wasm32-wasip2/release/component_http.wasm"
          test -f "$ARTIFACT"
          oras push \
            "ghcr.io/greenticai/component/component-http:${{ steps.ver.outputs.version }}" \
            "${ARTIFACT}:application/wasm"

      - name: Verify push by pulling back
        run: |
          set -euo pipefail
          mkdir -p /tmp/verify
          oras pull "ghcr.io/greenticai/component/component-http:${{ steps.ver.outputs.version }}" -o /tmp/verify
          test -f /tmp/verify/component_http.wasm
```

- [ ] **Step 2: Validate YAML syntax locally**

```bash
yamllint .github/workflows/publish-component-http.yml || true
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/publish-component-http.yml'))" \
  && echo "YAML parse OK"
```

Expected: `YAML parse OK`. `yamllint` warnings are OK; errors block.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/publish-component-http.yml
git commit -m "feat(ci): add publish-component-http workflow

Triggers on \`component-http-v*\` tag pushes (mirror of
publish-webhook-extension.yml pattern). Pushes the built wasm to
ghcr.io/greenticai/component/component-http:<version> where <version>
is the tag with the \`component-http-v\` prefix stripped.

Closes the gap where component-http was only republished via the
ci.yml matrix tied to workspace version — preventing it from following
the same 1.2.x research-line policy as extensions."
```

### Task 7: Write verification script (TDD)

**Files:**
- Create: `tools/verify-component-http-publish.sh`

**TDD framing:** Script must FAIL when run against the stale `:0.1.0` artifact and PASS when run against the new `:1.2.0` artifact. Writing the failing-case check first locks in the contract.

- [ ] **Step 1: Confirm `tools/` exists or create it**

```bash
mkdir -p tools
ls tools/
```

- [ ] **Step 2: Write the script with explicit failure-on-stale check**

Create `tools/verify-component-http-publish.sh`:

```bash
#!/usr/bin/env bash
#
# Verifies a published `component-http` OCI artifact:
#   1. Pulls the artifact for the given version
#   2. Confirms the exported world is `greentic:component@0.6.0`
#   3. Validates the wasm structure
#
# Usage:
#   tools/verify-component-http-publish.sh <version>
#
# Examples:
#   tools/verify-component-http-publish.sh 0.1.0   # MUST fail (stale artifact)
#   tools/verify-component-http-publish.sh 1.2.0   # MUST pass (new artifact)

set -euo pipefail

VERSION="${1:?usage: $0 <version>}"
REGISTRY="ghcr.io/greenticai/component/component-http"
WORK_DIR="$(mktemp -d -t verify-component-http-XXXXXX)"
trap 'rm -rf "$WORK_DIR"' EXIT

echo "==> Pulling ${REGISTRY}:${VERSION}"
oras pull "${REGISTRY}:${VERSION}" -o "$WORK_DIR"

WASM="$WORK_DIR/component_http.wasm"
if [ ! -f "$WASM" ]; then
    echo "FAIL: ${REGISTRY}:${VERSION} did not contain component_http.wasm" >&2
    exit 1
fi

echo "==> Checking exported world"
WIT=$(wasm-tools component wit "$WASM")
if echo "$WIT" | grep -q 'package greentic:component@0.6.0'; then
    echo "OK: exports greentic:component@0.6.0"
else
    echo "FAIL: artifact does not export greentic:component@0.6.0" >&2
    echo "--- actual WIT ---" >&2
    echo "$WIT" | head -40 >&2
    exit 1
fi

echo "==> Validating wasm structure"
wasm-tools validate "$WASM"
echo "OK: wasm validates"

echo "==> SUCCESS: ${REGISTRY}:${VERSION}"
```

- [ ] **Step 3: Make executable**

```bash
chmod +x tools/verify-component-http-publish.sh
ls -la tools/verify-component-http-publish.sh
```

Expected: file is executable (`-rwxr-xr-x`).

- [ ] **Step 4: Run against the stale `:0.1.0` artifact — MUST fail**

```bash
tools/verify-component-http-publish.sh 0.1.0
echo "Exit code: $?"
```

Expected: exit code **non-zero** with stderr message `FAIL: artifact does not export greentic:component@0.6.0`. This proves the script catches the bug.

If the script PASSES on 0.1.0: either the 0.1.0 artifact has already been republished (which would invalidate this spec's premise — investigate before proceeding) or the script's check is incorrect (review the grep pattern).

- [ ] **Step 5: Run against `:1.2.0` — MUST fail with "not found" for now**

```bash
tools/verify-component-http-publish.sh 1.2.0
echo "Exit code: $?"
```

Expected: exit code **non-zero**. Either `oras pull` reports `manifest unknown` (because `:1.2.0` hasn't been published yet — Task 10 publishes it) or the script's exit traces an earlier failure. This is the **failing test** that Task 10 makes pass.

- [ ] **Step 6: Commit**

```bash
git add tools/verify-component-http-publish.sh
git commit -m "test(component-http): add OCI publish verification script

Pulls the published artifact for a given version, asserts the exported
world is greentic:component@0.6.0, and validates wasm structure.

Designed as a TDD harness: fails on the stale :0.1.0 artifact, passes
on the new :1.2.0 artifact once published. Used to gate done-criterion
D9 in the SP-1 design spec."
```

### Task 8: Push feature branch and open PR

**Files:** no file edits; PR creation.

- [ ] **Step 1: Push the feature branch**

```bash
git push -u origin feat/publish-component-http-1.2.0
```

Expected: success.

- [ ] **Step 2: Confirm PR CI passes on push**

```bash
gh pr checks "feat/publish-component-http-1.2.0" 2>&1 || true
```

If no PR yet, this is expected to fail; proceed to Step 3.

- [ ] **Step 3: Open PR `feat/publish-component-http-1.2.0` → research**

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
- Add `tools/verify-component-http-publish.sh` — TDD harness, fails on stale `:0.1.0`, will pass on new `:1.2.0` after tag.

## Test plan

- [x] Local: `cargo build -p component-http --target wasm32-wasip2 --release` OK
- [x] Local: `wasm-tools component wit` reports `package greentic:component@0.6.0` on freshly built wasm
- [x] Local: `tools/verify-component-http-publish.sh 0.1.0` fails (catches the stale artifact)
- [ ] PR CI green
- [ ] Post-merge on `research`: push `component-http-v1.2.0` tag → workflow run SUCCESS
- [ ] `tools/verify-component-http-publish.sh 1.2.0` passes after publish

## References

`project_3point_escalation`, `project_extension_version_scheme`, `project_oci_stable_migration`.

Spec: `docs/superpowers/specs/2026-05-13-component-http-republish-design.md`
EOF
)"
gh pr view --json url -q .url > /tmp/sp1-feat-pr-url
cat /tmp/sp1-feat-pr-url
```

Expected: PR URL printed and recorded.

- [ ] **Step 4: Gate — wait for PR review and merge to research**

```bash
gh pr view "$(cat /tmp/sp1-feat-pr-url)" --json state,mergeable,reviewDecision,statusCheckRollup
```

Expected for proceed: `state: MERGED`. **Do NOT proceed to Task 9 until merged.**

---

## Phase C: Tag, publish, validate (Tasks 9-10)

### Task 9: Push the publish tag

**Files:** none modified; tag creation.

- [ ] **Step 1: Update research and find the merge commit**

```bash
cd "$(cat /tmp/sp1-worktree-path)"
git fetch origin
git checkout research
git pull --ff-only origin research
git log --oneline -n 5
```

Expected: top commit is the merge from Task 8.

- [ ] **Step 2: Capture the merge SHA**

```bash
MERGE_SHA=$(git rev-parse HEAD)
echo "$MERGE_SHA" > /tmp/sp1-merge-sha
echo "merge commit: $MERGE_SHA"
```

- [ ] **Step 3: Create the tag on the merge commit**

```bash
git tag component-http-v1.2.0 "$MERGE_SHA"
git tag --list 'component-http-v*'
```

Expected: prints `component-http-v1.2.0`.

- [ ] **Step 4: Push the tag**

```bash
git push origin component-http-v1.2.0
```

Expected: `[new tag] component-http-v1.2.0 -> component-http-v1.2.0`.

If permission denied: maintainer must push the tag instead. Provide them the SHA from `/tmp/sp1-merge-sha`.

- [ ] **Step 5: Confirm the workflow run started**

Wait ~30 seconds, then:

```bash
gh run list -w "Publish Component HTTP" --limit 3
```

Expected: at least one run with status `in_progress` or `queued` triggered by `push` event on `component-http-v1.2.0`.

### Task 10: Validate the published artifact (closes D6-D9)

**Files:** none modified; validation only.

- [ ] **Step 1: Watch the publish workflow to completion**

```bash
RUN_ID=$(gh run list -w "Publish Component HTTP" --limit 1 --json databaseId -q '.[0].databaseId')
gh run watch "$RUN_ID"
```

Expected: workflow completes with `completed success` (or similar success indicator).

If failure: download logs with `gh run view "$RUN_ID" --log-failed` and diagnose. Common issues:

- `oras login` failure → `GITHUB_TOKEN` lacks `packages:write` (check workflow `permissions:` block).
- `cargo component` not found → toolchain version mismatch; verify `rust-toolchain.toml` install step.
- Reusable workflow input rejection → check input names in `wasm-component-ci.yml@main` match Step 1A's `with:` block; switch to Step 1B if needed.

**Done criterion D6 satisfied here.**

- [ ] **Step 2: Verify the artifact pulls (D7)**

```bash
mkdir -p /tmp/sp1-verify
oras pull ghcr.io/greenticai/component/component-http:1.2.0 -o /tmp/sp1-verify
ls -la /tmp/sp1-verify/component_http.wasm
```

Expected: file present, non-zero size. **Done criterion D7 satisfied.**

- [ ] **Step 3: Verify the exported world (D8)**

```bash
wasm-tools component wit /tmp/sp1-verify/component_http.wasm | grep 'package greentic:component@0.6.0'
```

Expected: matches. **Done criterion D8 satisfied.**

- [ ] **Step 4: Run the verification script — must PASS now (D9)**

```bash
tools/verify-component-http-publish.sh 1.2.0
echo "Exit code: $?"
```

Expected: exit code `0`, final line `==> SUCCESS: ghcr.io/greenticai/component/component-http:1.2.0`.

- [ ] **Step 5: Confirm the regression: same script still fails on `:0.1.0`**

```bash
tools/verify-component-http-publish.sh 0.1.0
echo "Exit code: $?"
```

Expected: exit code non-zero. Proves `:1.2.0` is a genuine fix not a side effect of script change.

- [ ] **Step 6: Anti-regression spot-check on neighboring components**

```bash
for comp in component-pack2flow component-events2msg; do
    echo "=== $comp ==="
    oras manifest fetch "ghcr.io/greenticai/component/$comp:latest" \
        --descriptor 2>&1 | head -5 || echo "  (not present at :latest)"
done
```

Expected: each neighboring component's `:latest` digest is unchanged from pre-SP-1 baseline (compare manually if a baseline was captured before Task 2). If any digest changed unexpectedly, investigate the ci.yml matrix — possible double-publish or side effect.

- [ ] **Step 7: Confirm full done criteria checklist**

Print and verify each:

```bash
echo "D1: main = post-promote SHA"
git ls-remote origin main
echo "expected: same SHA as $(cat /tmp/sp1-merge-sha) or fast-forward of promote PR merge"

echo "D2: research branch exists"
git ls-remote origin research

echo "D3: component-http/Cargo.toml has version 1.2.0 on research"
git show origin/research:crates/component-http/Cargo.toml | grep '^version'

echo "D4: publish-component-http.yml exists on research"
git show origin/research:.github/workflows/publish-component-http.yml | head -5

echo "D5: tag pushed"
git ls-remote origin --tags | grep component-http-v1.2.0

echo "D6, D7, D8, D9: see Steps 1-4 of this Task"
```

Expected: every line produces evidence. All nine D-criteria green = SP-1 done.

- [ ] **Step 8: Record completion**

```bash
cat > /tmp/sp1-completion.md <<EOF
# SP-1 component-http republish — DONE

- Promote PR: $(cat /tmp/sp1-promote-pr-url)
- Feat PR:    $(cat /tmp/sp1-feat-pr-url)
- Tag:        component-http-v1.2.0 @ $(cat /tmp/sp1-merge-sha)
- OCI:        ghcr.io/greenticai/component/component-http:1.2.0
- Workflow:   $(gh run list -w "Publish Component HTTP" --limit 1 --json databaseId -q '.[0].databaseId')
- Completed:  $(date -Iseconds)

Unblocks:
- SP-2 (greentic-flow frequent-components.json :latest → :1.2.0)
- SP-3 (greentic-demo bundle rebuild)
EOF
cat /tmp/sp1-completion.md
```

This file documents the close. Surface it to the requesting party (post in the relevant tracking issue, etc.).

---

## Post-execution: worktree cleanup

### Task 11: Tear down worktree

**Files:** worktree path removed.

- [ ] **Step 1: Confirm no uncommitted state in worktree**

```bash
cd "$(cat /tmp/sp1-worktree-path)"
git status --porcelain
```

Expected: empty.

- [ ] **Step 2: Return to main repo and remove worktree**

```bash
cd /home/bima-pangestu/Works/greentic/components-public
git worktree remove "$(cat /tmp/sp1-worktree-path)"
git worktree list
```

Expected: removed entry no longer listed.

- [ ] **Step 3: Clean up /tmp notes**

```bash
rm -f /tmp/sp1-worktree-path /tmp/sp1-promote-branch /tmp/sp1-promote-commits.txt \
      /tmp/sp1-promote-pr-url /tmp/sp1-feat-pr-url /tmp/sp1-merge-sha \
      /tmp/sp1-reusable-workflow-notes.md /tmp/wasm-component-ci.yml \
      /tmp/sp1-completion.md
rm -rf /tmp/sp1-verify /tmp/verify
```

(Optionally keep `/tmp/sp1-completion.md` — that is the artifact of record.)

---

## References

**Spec:** `components-public/docs/superpowers/specs/2026-05-13-component-http-republish-design.md`
**Spec PR:** https://github.com/greenticai/components-public/pull/47

**Memory references:** `project_3point_escalation`, `project_extension_version_scheme`, `project_three_tier_branching`, `project_research_promote_cadence`, `feedback_pr_target_research_directly`, `feedback_devops_team_owns_infra`, `feedback_no_invented_names_in_public`, `feedback_always_use_worktree`, `feedback_no_claude_attribution`, `project_oci_stable_migration`, `project_ac_validation_mode_bug`.
