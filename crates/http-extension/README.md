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

Local publish (no Store):
```bash
gtdx publish       # produce dist/greentic.http-0.1.0.gtxpack + install to local registry
```

Store publish via CI:
Required repo settings on `greenticai/components-public`:
- Secret `GREENTIC_STORE_TOKEN` — `gts_*` long-lived API token from the `greentic` publisher
- Variable `GREENTIC_STORE_URL` — e.g. `http://62.171.174.152:3030`

To publish a new version:
1. Bump `version` in both `describe.json` and `Cargo.toml`
2. Bump `runtime-version.txt` if this release targets a new `component-http` version
3. Commit + push to main
4. Tag: `git tag ext-v<version> && git push origin ext-v<version>`
5. The `publish-extension` workflow runs, posts the `.gtxpack` to the Store

Local build + inspect without publishing:
```bash
bash crates/http-extension/build.sh
ls -lh crates/http-extension/dist/
```

To publish from a local dev machine (bypassing CI):
```bash
cd crates/http-extension
gtdx login --registry <url>
gtdx publish --registry <url>
```

## Layout

- `describe.json` — extension manifest
- `src/lib.rs`    — WASM guest exports
- `wit/`          — WIT contract (vendored by `gtdx new`; see `.gtdx-contract.lock`)
- `i18n/en.json`  — user-facing strings
