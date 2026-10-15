#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"

echo "==> cargo component build"
cargo component build --release --target wasm32-wasip2

WASM_PATH="target/wasm32-wasip2/release/greentic_jev_extension.wasm"
if [ ! -f "$WASM_PATH" ]; then
  echo "ERROR: wasm not found at $WASM_PATH" >&2
  exit 1
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

cp describe.json "$STAGE/"
cp "$WASM_PATH" "$STAGE/extension.wasm"

if command -v sha256sum >/dev/null 2>&1; then
  WASM_SHA256="$(sha256sum "$STAGE/extension.wasm" | cut -d' ' -f1)"
else
  WASM_SHA256="$(shasum -a 256 "$STAGE/extension.wasm" | cut -d' ' -f1)"
fi
sed "s/0000000000000000000000000000000000000000000000000000000000000000/${WASM_SHA256}/g" \
  "$STAGE/describe.json" > "$STAGE/describe.json.tmp"
mv "$STAGE/describe.json.tmp" "$STAGE/describe.json"

EXT_ID="$(jq -r .metadata.id "$STAGE/describe.json")"
EXT_VERSION="$(jq -r .metadata.version "$STAGE/describe.json")"
OUT="$HERE/${EXT_ID}-${EXT_VERSION}.gtxpack"
TMP_ZIP="$STAGE/../greentic_jev_$$.zip"
trap 'rm -rf "$STAGE" "$TMP_ZIP"' EXIT
(cd "$STAGE" && zip -r "$TMP_ZIP" .) > /dev/null
mv "$TMP_ZIP" "$OUT"

echo "==> built $OUT"
echo "==> size: $(du -h "$OUT" | cut -f1)"
