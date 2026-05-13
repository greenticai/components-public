#!/usr/bin/env bash
#
# Verifies a published `component-http` OCI artifact exports the full
# compat-shim interface surface (component-descriptor/component-schema/
# component-runtime), not just the right world version. The 0.1.0
# artifact had the right world but lacked the 3 compat interfaces —
# THIS is what made it fail at greentic-pack doctor time.
#
# Usage:
#   tools/verify-component-http-publish.sh <version>
#
# Examples:
#   tools/verify-component-http-publish.sh 0.1.0   # MUST fail
#   tools/verify-component-http-publish.sh 1.2.0   # MUST pass

set -euo pipefail

VERSION="${1:?usage: $0 <version>}"
REGISTRY="ghcr.io/greenticai/component/component-http"
WORK_DIR="$(mktemp -d -t verify-component-http-XXXXXX)"
trap 'rm -rf "$WORK_DIR"' EXIT

echo "==> Pulling ${REGISTRY}:${VERSION}"
oras pull "${REGISTRY}:${VERSION}" -o "$WORK_DIR"

WASM="$(find "$WORK_DIR" -name 'component_http.wasm' | head -1)"
if [ -z "$WASM" ] || [ ! -f "$WASM" ]; then
    echo "FAIL: ${REGISTRY}:${VERSION} did not contain component_http.wasm" >&2
    exit 1
fi

echo "==> Checking required exports"
WIT="$(wasm-tools component wit "$WASM")"

MISSING=()
for INTERFACE in component-descriptor@0.6.0 component-schema@0.6.0 component-runtime@0.6.0; do
    if echo "$WIT" | grep -q "export greentic:component/${INTERFACE};"; then
        echo "OK:    exports greentic:component/${INTERFACE}"
    else
        echo "FAIL:  greentic:component/${INTERFACE} NOT exported" >&2
        MISSING+=("$INTERFACE")
    fi
done

if [ "${#MISSING[@]}" -ne 0 ]; then
    echo "" >&2
    echo "Missing ${#MISSING[@]} interface(s): ${MISSING[*]}" >&2
    echo "--- full WIT output (first 80 lines) ---" >&2
    echo "$WIT" | head -80 >&2
    exit 1
fi

echo "==> Validating wasm structure"
wasm-tools validate "$WASM"
echo "OK: wasm validates"

echo "==> SUCCESS: ${REGISTRY}:${VERSION}"
