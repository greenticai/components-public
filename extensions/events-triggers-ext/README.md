# events-triggers-ext

Greentic design extension that ships `nodeType` descriptors for event-based trigger nodes (timer, SMS, email) so they appear as first-class trigger primitives in the Greentic Designer palette.

## Building

```bash
cargo component build --release --target wasm32-wasip2 --package events-triggers-ext
```

Or using the convenience script (requires `describe.json` and `jq`):

```bash
./build.sh
```

## Extension metadata

- **Package:** `greentic:events-triggers`
- **Kind:** Design (nodeTypes-only, no tools)
- **Offered capability:** `greentic:events-triggers/trigger-nodes`
