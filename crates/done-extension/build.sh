#!/usr/bin/env bash
# Build greentic.done and produce a .gtxpack in ./dist/. Does not need gtdx.
# Publishing (auth + signing) is the gtdx / extension-action flow, not this script.
#
# describe.json names the node component by an OCI digest that does not exist
# until component-done is published. This script packages a DEV pack with the
# placeholder untouched, and refuses a RELEASE (RELEASE=1) until you pass the
# published reference:
#
#   RELEASE=1 DONE_OPS_OCI_REF=oci://ghcr.io/greenticai/component/component-done@sha256:<digest> \
#             DONE_OPS_SHA256=<sha256 of the component wasm> ./build.sh
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$DIR/../.." && pwd)"
ZERO="$(printf '0%.0s' $(seq 1 64))"

VERSION="$(jq -r .metadata.version "$DIR/describe.json")"
echo "==> Building done-extension v${VERSION} (wasm32-wasip2)..."

cd "$WORKSPACE_ROOT"
cargo component build --release --target wasm32-wasip2 --package done-extension

WASM_SRC=""
for candidate in \
    "$WORKSPACE_ROOT/target/wasm32-wasip2/release/done_extension.wasm" \
    "$WORKSPACE_ROOT/target/wasm32-wasip1/release/done_extension.wasm"; do
    if [ -f "$candidate" ]; then WASM_SRC="$candidate"; break; fi
done
[ -n "$WASM_SRC" ] || { echo "ERROR: built wasm not found"; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp "$WASM_SRC" "$STAGE/extension.wasm"
WASM_SHA="$(sha256sum "$STAGE/extension.wasm" | cut -d' ' -f1)"

# The tool component's digest is ours to fill. The node component's is not.
jq --arg sha "$WASM_SHA" \
   '.runtime.components["done-tool"].sha256 = $sha
    | .runtime.components["done-tool"].gtpack.sha256 = $sha' \
   "$DIR/describe.json" > "$STAGE/describe.json"

if [ -n "${DONE_OPS_OCI_REF:-}" ]; then
    : "${DONE_OPS_SHA256:?DONE_OPS_SHA256 is required with DONE_OPS_OCI_REF}"
    jq --arg ref "$DONE_OPS_OCI_REF" --arg sha "$DONE_OPS_SHA256" \
       '.runtime.components["done-node"].oci_ref = $ref | .runtime.components["done-node"].sha256 = $sha' \
       "$STAGE/describe.json" > "$STAGE/d.json" && mv "$STAGE/d.json" "$STAGE/describe.json"
fi

if [ "${RELEASE:-0}" = "1" ] && jq -e --arg z "$ZERO" \
    '.runtime.components["done-node"].sha256 == $z' "$STAGE/describe.json" >/dev/null; then
    echo "ERROR: RELEASE=1 but component-done has no published reference (see header)." >&2
    exit 1
fi

cp -r "$DIR/i18n" "$STAGE/i18n"
[ -d "$DIR/assets" ] && cp -r "$DIR/assets" "$STAGE/assets"

mkdir -p "$DIR/dist"
PKG="$DIR/dist/greentic.done-${VERSION}.gtxpack"
rm -f "$PKG"
(cd "$STAGE" && zip -qr "$PKG" extension.wasm describe.json i18n $([ -d assets ] && echo assets))
echo "==> Built: $PKG"
ls -lh "$PKG"
