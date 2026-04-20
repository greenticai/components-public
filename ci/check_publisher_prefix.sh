#!/usr/bin/env bash
# Usage: check_publisher_prefix.sh <publisher-name> <expected-prefix>
# Exit 0 if the publisher's allowed_prefixes covers expected-prefix.
# Exit 1 otherwise, with a message suggesting admin action.
set -euo pipefail

PUBLISHER="${1:-}"
PREFIX="${2:-}"
if [ -z "$PUBLISHER" ] || [ -z "$PREFIX" ]; then
    echo "usage: $0 <publisher> <prefix>"
    exit 2
fi

URL="${GREENTIC_STORE_URL:?GREENTIC_STORE_URL not set}"
TOKEN="${GREENTIC_STORE_TOKEN:?GREENTIC_STORE_TOKEN not set}"

RESP="$(curl -sS -H "Authorization: Bearer $TOKEN" "$URL/api/v1/publishers/$PUBLISHER")"
ALLOWED="$(echo "$RESP" | jq -r '.allowed_prefixes[]' 2>/dev/null || true)"

if [ -z "$ALLOWED" ]; then
    echo "ERROR: could not read allowed_prefixes for publisher '$PUBLISHER':"
    echo "$RESP"
    exit 1
fi

MATCH=0
while IFS= read -r p; do
    # Exact match or wildcard (e.g. "greentic." matches "greentic.http")
    if [ "$p" = "$PREFIX" ] || [[ "$PREFIX" == $p* ]]; then
        MATCH=1
        break
    fi
done <<< "$ALLOWED"

if [ $MATCH -eq 1 ]; then
    echo "OK: publisher '$PUBLISHER' is allowed to publish '$PREFIX'"
    exit 0
fi

cat <<EOF
ERROR: publisher '$PUBLISHER' is NOT allowed to publish '$PREFIX'.
Current allowed_prefixes:
$ALLOWED

Ask an admin to add '$PREFIX' to the publisher via the admin API, e.g.:
  curl -X POST "$URL/api/v1/admin/publishers/$PUBLISHER/prefixes" \\
       -H "Authorization: Bearer \$ADMIN_TOKEN" \\
       -d '{"prefix":"$PREFIX"}'
EOF
exit 1
