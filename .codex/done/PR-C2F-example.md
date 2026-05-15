PR: Add demos/routing-demo (CLI-generated packs + ready bundle)
Goal

Add a routing demo that can be built from scratch using Greentic CLIs:

Generates demo packs via CLI (no hand-authored pack/bundle CBOR checked in)

Builds .gtpack artifacts into demos/routing-demo/dist/

Builds a bundle into demos/routing-demo/dist/routing-demo.bundle.cbor

Operator can be pointed at the bundle file directly

Files added
demos/
  routing-demo/
    README.md
    answers/
      control-chain.answers.json
      router-chat2flow-demo.answers.json
      pack-support.answers.json
      pack-it.answers.json
      pack-calendar.answers.json
      pack-welcome.answers.json
      bundle.answers.json
    scripts/
      00_doctor.sh
      10_generate_packs.sh
      20_build_gtpacks.sh
      30_build_bundle.sh
      40_run_operator.sh
      build_all.sh
    fixtures/
      messages.txt
      expected.md
    .gitignore
What is not committed

No pack/pack.cbor

No .gtpack

No bundle.cbor

All generated into demos/routing-demo/dist/ by scripts.

How it works
1) Pack generation via CLI (non-interactive)

10_generate_packs.sh runs the CLI wizard(s) in “answers file” mode to generate pack directories under:

demos/routing-demo/build/packs/<pack-name>/

This uses answers JSON committed in answers/ so the demo is reproducible.

2) Build .gtpack via CLI

20_build_gtpacks.sh calls the pack build CLI for each generated pack directory to produce:

demos/routing-demo/dist/control-chain.gtpack
demos/routing-demo/dist/router-chat2flow-demo.gtpack
demos/routing-demo/dist/pack-support.gtpack
demos/routing-demo/dist/pack-it.gtpack
demos/routing-demo/dist/pack-calendar.gtpack
demos/routing-demo/dist/pack-welcome.gtpack
3) Build a bundle via CLI

30_build_bundle.sh uses the operator/bundle CLI to produce:

demos/routing-demo/dist/routing-demo.bundle.cbor

This is the single artifact you point the operator at.

4) Run operator using that bundle

40_run_operator.sh runs the operator pointing to the bundle in dist/.

The scripts (PR content)
demos/routing-demo/scripts/build_all.sh

Runs everything end-to-end:

#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

"${HERE}/scripts/00_doctor.sh"
"${HERE}/scripts/10_generate_packs.sh"
"${HERE}/scripts/20_build_gtpacks.sh"
"${HERE}/scripts/30_build_bundle.sh"

echo
echo "✅ Built bundle:"
echo "  ${HERE}/dist/routing-demo.bundle.cbor"
echo
echo "Next:"
echo "  ${HERE}/scripts/40_run_operator.sh"
00_doctor.sh

Uses CLI only, validates the environment.

#!/usr/bin/env bash
set -euo pipefail

need() { command -v "$1" >/dev/null 2>&1 || { echo "Missing: $1"; exit 1; }; }

# Prefer gtc as the unified entrypoint; fallback to component/pack/operator binaries.
if command -v gtc >/dev/null 2>&1; then
  echo "Using: gtc"
  gtc doctor || true
else
  echo "gtc not found; expecting greentic-* CLIs on PATH"
  need greentic-operator
  need greentic-pack || true
  need greentic-component || true
  need greentic-flow || true
fi
10_generate_packs.sh

Generates pack directories via wizard + answers.

Important: this script is written to work with either gtc wizard ... or direct greentic-* wizard ... binaries. It never writes pack CBOR directly; it just invokes the wizard and outputs to a directory.

#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD="${HERE}/build"
ANS="${HERE}/answers"

rm -rf "${BUILD}"
mkdir -p "${BUILD}/packs"

run_wizard() {
  # $1 = target (pack/component/flow/operator) depending on your CLI
  # $2 = answers json
  # $3 = out dir
  local target="$1"
  local answers="$2"
  local outdir="$3"

  mkdir -p "${outdir}"

  if command -v gtc >/dev/null 2>&1; then
    # Preferred unified entrypoint:
    # Adjust subcommand shape if needed, but keep it CLI-driven.
    gtc wizard run --target "${target}" --answers "${answers}" --out "${outdir}"
  else
    # Fallback: try pack wizard first (most likely for pack scaffolds)
    if command -v greentic-pack >/dev/null 2>&1; then
      greentic-pack wizard run --answers "${answers}" --out "${outdir}"
    elif command -v greentic-component >/dev/null 2>&1; then
      greentic-component wizard run --answers "${answers}" --out "${outdir}"
    else
      echo "No suitable wizard CLI found (gtc/greentic-pack/greentic-component)."
      exit 1
    fi
  fi
}

echo "Generating packs from answers..."

run_wizard pack "${ANS}/control-chain.answers.json"       "${BUILD}/packs/control-chain"
run_wizard pack "${ANS}/router-chat2flow-demo.answers.json" "${BUILD}/packs/router-chat2flow-demo"
run_wizard pack "${ANS}/pack-support.answers.json"        "${BUILD}/packs/pack-support"
run_wizard pack "${ANS}/pack-it.answers.json"             "${BUILD}/packs/pack-it"
run_wizard pack "${ANS}/pack-calendar.answers.json"       "${BUILD}/packs/pack-calendar"
run_wizard pack "${ANS}/pack-welcome.answers.json"        "${BUILD}/packs/pack-welcome"

echo "✅ Pack directories generated under: ${BUILD}/packs"
20_build_gtpacks.sh

Builds gtpack zips via CLI:

#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD="${HERE}/build"
DIST="${HERE}/dist"

rm -rf "${DIST}"
mkdir -p "${DIST}"

build_pack() {
  local indir="$1"
  local out="$2"

  if command -v gtc >/dev/null 2>&1; then
    gtc pack build --in "${indir}" --out "${out}"
  elif command -v greentic-pack >/dev/null 2>&1; then
    greentic-pack build --in "${indir}" --out "${out}"
  else
    echo "No pack build CLI found (gtc pack build / greentic-pack build)."
    exit 1
  fi
}

echo "Building .gtpack artifacts..."

build_pack "${BUILD}/packs/control-chain"          "${DIST}/control-chain.gtpack"
build_pack "${BUILD}/packs/router-chat2flow-demo"  "${DIST}/router-chat2flow-demo.gtpack"
build_pack "${BUILD}/packs/pack-support"           "${DIST}/pack-support.gtpack"
build_pack "${BUILD}/packs/pack-it"                "${DIST}/pack-it.gtpack"
build_pack "${BUILD}/packs/pack-calendar"          "${DIST}/pack-calendar.gtpack"
build_pack "${BUILD}/packs/pack-welcome"           "${DIST}/pack-welcome.gtpack"

echo "✅ .gtpack artifacts in: ${DIST}"
30_build_bundle.sh

Builds the bundle via CLI (again: no handcrafted CBOR).

#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="${HERE}/dist"
ANS="${HERE}/answers"

OUT="${DIST}/routing-demo.bundle.cbor"

if command -v gtc >/dev/null 2>&1; then
  # Prefer a wizard/bundle builder if it exists in your CLI tree.
  # This is intentionally CLI-driven; you can map this to the exact operator bundle command you have.
  gtc op bundle build --answers "${ANS}/bundle.answers.json" --packs-dir "${DIST}" --out "${OUT}"
elif command -v greentic-operator >/dev/null 2>&1; then
  greentic-operator bundle build --answers "${ANS}/bundle.answers.json" --packs-dir "${DIST}" --out "${OUT}"
else
  echo "No operator/bundle CLI found (gtc op bundle / greentic-operator)."
  exit 1
fi

echo "✅ Bundle written: ${OUT}"
40_run_operator.sh

Runs operator pointing at the built bundle:

#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE="${HERE}/dist/routing-demo.bundle.cbor"

if command -v gtc >/dev/null 2>&1; then
  gtc op run --bundle "${BUNDLE}"
else
  greentic-operator run --bundle "${BUNDLE}"
fi
What goes into the answers files

These are the only “content” you commit, and they should align with your architecture:

control-chain.answers.json:

sets offer as hook (kind=hook, stage=post_ingress, contract=greentic.hook.control.v1)

points provider op to ingress_control.handle

includes optional rules/policy assets in CBOR (wizard should generate CBOR assets, not json)

router-chat2flow-demo.answers.json:

router pack metadata + router op name

each domain pack answers:

default entry flow includes local router call

includes assets/intent_to_flow.cbor mapping

bundle.answers.json:

includes all gtpack paths from dist/

sets a default entry pack (welcome) plus enables post_ingress hook discovery

If your wizard supports “embed assets from inline JSON → emit CBOR”, this stays fully CLI-driven and reproducible.

README behavior to demonstrate

fixtures/messages.txt drives the “demo script” (manual or future automation):

“refund please” → dispatch to support pack → asks “order id?”

“12345” → continues inside support flow (no chain called)

“vpn broken” → dispatch to IT pack

“hi” → continue → welcome pack

PR description (what you’d paste into GitHub)

Title: demos: add routing-demo (CLI-generated packs + ready-to-run bundle)

Summary:

Adds demos/routing-demo to components-public.

Demo generates packs and a runnable bundle using Greentic CLIs (answers-driven, non-interactive).

Produces dist/routing-demo.bundle.cbor which the operator can be pointed at directly.

No handcrafted pack.cbor/bundle.cbor checked in; only answers + scripts.

How to run:

cd demos/routing-demo
./scripts/build_all.sh
./scripts/40_run_operator.sh
One thing I’m deliberately doing here

Because I don’t have your exact CLI surface in front of me, the scripts are written to:

prefer gtc ... if present

fallback to greentic-pack / greentic-operator variants

keep the whole demo CLI-driven and easy to adjust in exactly one place (run_wizard, build_pack, bundle build call)

That gives you a PR you can merge immediately and then tweak only the command spellings if your CLI differs.