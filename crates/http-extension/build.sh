#!/usr/bin/env bash
# Build greentic.http extension and produce .gtxpack in ./dist/.
# Run `gtdx publish` (without --dry-run) to upload to a registry.
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$DIR/../.." && pwd)"
cd "$DIR"

VERSION="$(jq -r .metadata.version describe.json)"
echo "==> Building http-extension v${VERSION} (wasm32-wasip2)..."

# Build workspace from root so WASM is created in workspace target directory
cd "$WORKSPACE_ROOT"
cargo component build --release --package http-extension

# Copy WASM to local target directory for gtdx to find
mkdir -p "$DIR/target/wasm32-wasip1/release"
cp target/wasm32-wasip1/release/http_extension.wasm "$DIR/target/wasm32-wasip1/release/" 2>/dev/null || true

# Publish from crate directory
cd "$DIR"
gtdx publish --dry-run --dist ./dist

# Look for the produced .gtxpack (may be publish-staging.gtxpack or greentic.http-VERSION.gtxpack)
PKG="$(find ./dist -maxdepth 1 -type f -name '*.gtxpack' | sort | head -n1)"

if [ -z "${PKG:-}" ] || [ ! -f "${PKG}" ]; then
    echo "ERROR: gtdx publish --dry-run did not produce a .gtxpack in ./dist/"
    ls -la ./dist/ || true
    exit 1
fi

echo "==> Built: ${PKG}"
ls -lh "${PKG}"
