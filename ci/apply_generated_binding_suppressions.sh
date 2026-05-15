#!/usr/bin/env bash
set -euo pipefail

files=(
  crates/http-extension/src/bindings.rs
  crates/llm-generic-extension/src/bindings.rs
  crates/platform-extension/src/bindings.rs
  crates/webhook-extension/src/bindings.rs
)

for file in "${files[@]}"; do
  perl -0pi -e 's#(?m)^(\s*)// Generated WIT enum lift uses transmute after the component ABI constrains discriminants; debug builds validate explicitly\.\n\1// foxguard: ignore\[rs/transmute-usage\]\n##g' "$file"
  perl -0pi -e 's#(?m)^(\s*)return ::core::mem::transmute\(val\);#$1// Generated WIT enum lift uses transmute after the component ABI constrains discriminants; debug builds validate explicitly.\n$1// foxguard: ignore[rs/transmute-usage]\n$1return ::core::mem::transmute(val);#g' "$file"
done
