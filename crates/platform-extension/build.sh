#!/usr/bin/env bash
# Build greentic.platform-bootstrap extension and produce .gtxpack in ./dist/.
# Does not require gtdx. For full publish flow (with auth + signing),
# use `gtdx publish` directly or the `greenticai/greentic-designer-extension-action` CI action.
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$DIR/../.." && pwd)"

VERSION="$(jq -r .metadata.version "$DIR/describe.json")"
echo "==> Building platform-extension v${VERSION} (wasm32-wasip2)..."

cd "$WORKSPACE_ROOT"
cargo component build --release --package platform-extension

# cargo-component writes to wasm32-wasip1 target triple even when the WIT world is wasip2
WASM_SRC=""
for candidate in \
    "$WORKSPACE_ROOT/target/wasm32-wasip1/release/platform_extension.wasm" \
    "$WORKSPACE_ROOT/target/wasm32-wasip2/release/platform_extension.wasm" \
    "$DIR/target/wasm32-wasip1/release/platform_extension.wasm" \
    "$DIR/target/wasm32-wasip2/release/platform_extension.wasm"; do
    if [ -f "$candidate" ]; then
        WASM_SRC="$candidate"
        break
    fi
done
if [ -z "$WASM_SRC" ]; then
    echo "ERROR: built wasm not found in any expected target path"
    exit 1
fi

cd "$DIR"
mkdir -p dist
cp "$WASM_SRC" extension.wasm

PKG_NAME="greentic.platform-bootstrap-${VERSION}.gtxpack"
PKG="dist/${PKG_NAME}"
rm -f "$PKG"

TMP_ZIP="dist/${PKG_NAME}.zip"
rm -f "$TMP_ZIP"
# This extension ships pure metadata: only describe.json + extension.wasm
# go into the .gtxpack — no prompts, schemas, assets, or i18n.
zip -qr "$TMP_ZIP" extension.wasm describe.json
mv "$TMP_ZIP" "$PKG"

rm -f extension.wasm

echo "==> Built: $DIR/$PKG"
ls -lh "$PKG"
