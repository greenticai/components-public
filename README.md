# Greentic Public Components

This repository is a Cargo workspace for public Greentic components.

## Structure

- `crates/`: independent component crates
- `Cargo.toml`: shared workspace configuration and common dependencies

## Add a New Component

1. Create a new crate under `crates/`.
2. In its `Cargo.toml`, use `*.workspace = true` for shared metadata.
3. Reuse shared dependencies from `[workspace.dependencies]`.
4. Run `cargo test --workspace`.
