# http-extension

A Greentic Designer **design** extension.

- id: `greentic.http`
- version: `0.1.0`
- contract: `greentic:extension-design@0.1.0`

## Develop

```
gtdx dev           # watch, rebuild, and reinstall to local registry on save
```

## Publish

```
gtdx publish       # produce dist/greentic.http-0.1.0.gtxpack + install to local registry
```

## Layout

- `describe.json` — extension manifest
- `src/lib.rs`    — WASM guest exports
- `wit/`          — WIT contract (vendored by `gtdx new`; see `.gtdx-contract.lock`)
- `i18n/en.json`  — user-facing strings
