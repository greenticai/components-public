#!/usr/bin/env bash
# Build greentic.http extension and produce .gtxpack in ./dist/.
# Does not require gtdx. For full publish flow (with auth + signing),
# use `gtdx publish` directly or the `greenticai/greentic-designer-extension-action` CI action.
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$DIR/../.." && pwd)"

VERSION="$(jq -r .metadata.version "$DIR/describe.json")"
echo "==> Building http-extension v${VERSION} (wasm32-wasip2)..."

cd "$WORKSPACE_ROOT"
cargo component build --release --package http-extension

# cargo-component writes to wasm32-wasip1 target triple even when the WIT world is wasip2
WASM_SRC=""
for candidate in \
    "$WORKSPACE_ROOT/target/wasm32-wasip1/release/http_extension.wasm" \
    "$WORKSPACE_ROOT/target/wasm32-wasip2/release/http_extension.wasm" \
    "$DIR/target/wasm32-wasip1/release/http_extension.wasm" \
    "$DIR/target/wasm32-wasip2/release/http_extension.wasm"; do
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

PKG_NAME="greentic.http-${VERSION}.gtxpack"
PKG="dist/${PKG_NAME}"
rm -f "$PKG"

TMP_ZIP="dist/${PKG_NAME}.zip"
rm -f "$TMP_ZIP"
zip -qr "$TMP_ZIP" \
    extension.wasm \
    describe.json \
    prompts \
    schemas \
    i18n \
    $([ -d assets ] && echo assets)
mv "$TMP_ZIP" "$PKG"

rm -f extension.wasm

echo "==> Built: $DIR/$PKG"
ls -lh "$PKG"
