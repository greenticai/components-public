# Greentic Public Components

This repository is a Cargo workspace for public Greentic components.

## Structure

- `crates/`: reusable WASM **components** and their shared support crates
  (built as `wasm32-wasip2` components, published to GHCR).
- `extensions/`: Greentic Designer **design extensions** (`.gtxpack`,
  published to the Greentic Store via the `publish-*-extension` workflows).
- `Cargo.toml`: shared workspace configuration and common dependencies
  (members glob both `crates/*` and `extensions/*`).

The two are split because they have different build targets, distribution
channels, and consumers: components are runtime artifacts, design extensions
ship descriptors/schemas the designer renders.

## Add a New Component

1. Create a new crate under `crates/`.
2. In its `Cargo.toml`, use `*.workspace = true` for shared metadata.
3. Reuse shared dependencies from `[workspace.dependencies]`.
4. Run `cargo test --workspace`.

## Add a New Design Extension

1. Create a new crate under `extensions/`.
2. Ship `describe.json`, `wit/`, and `i18n/` alongside `src/lib.rs`.
3. Add a `publish-<name>-extension.yml` workflow keyed off a distinct tag
   pattern (e.g. `<name>-ext-v*`).
4. Run `cargo test --workspace`.
