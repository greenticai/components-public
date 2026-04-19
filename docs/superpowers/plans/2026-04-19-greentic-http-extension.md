# greentic.http DesignExtension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the `greentic.http` DesignExtension that teaches the Greentic Designer LLM to generate valid YGTc HTTP nodes, published to the Greentic Store via `gtdx` / the store publish action.

**Architecture:** Three crates in `components-public/crates/`: (1) extract a new pure-Rust `http-core` library holding validation/auth/URL/curl/node-builder logic; (2) slim down `component-http` to a thin WIT guest that delegates to `http-core`; (3) build a new `http-extension` WASM DesignExtension that exposes 5 LLM tools backed by `http-core`, packaged as `.gtxpack` and published to the Greentic Store.

**Tech Stack:** Rust 1.91 edition 2024, `wasm32-wasip2`, `cargo-component`, `wit-bindgen`, `greentic-ext-contract` (WIT world), `greenticai/greentic-designer-extension-action@v1` (CI publish), `oras` (GHCR for runtime component).

**Spec:** [`docs/superpowers/specs/2026-04-19-greentic-http-extension-design.md`](../specs/2026-04-19-greentic-http-extension-design.md)

**Branch:** Already on `spec/greentic-http-extension`. Sub-branches:
- Phase 1 PR lands via a branch named `refactor/extract-http-core`
- Phase 2 PR lands via a branch named `feat/http-extension`

**PR split (from spec §Refactor):**
- **PR #1 (Phase 1, Tasks 1-9)**: Extract `http-core` and refactor `component-http`. Zero behavior change. Existing tests + gtests must pass unchanged.
- **PR #2 (Phase 2, Tasks 10-24)**: New `http-extension` crate + publish pipeline. Builds on PR #1.

---

## File Structure

### Phase 1 — Refactor (PR #1)

- Create: `crates/http-core/Cargo.toml` — workspace member, no wasm deps
- Create: `crates/http-core/src/lib.rs` — public re-exports
- Create: `crates/http-core/src/config.rs` — `ComponentConfig`, `apply_answers`, config validators
- Create: `crates/http-core/src/auth.rs` — `AuthType` enum, header builders
- Create: `crates/http-core/src/url.rs` — URL validation + placeholder extraction
- Create: `crates/http-core/src/node.rs` — `YgtcNode`, `NodeBuilder`, `ComponentRef`
- Create: `crates/http-core/src/curl.rs` — curl command parser
- Create: `crates/http-core/tests/config_test.rs` — unit tests for config
- Create: `crates/http-core/tests/auth_test.rs` — unit tests for auth
- Create: `crates/http-core/tests/url_test.rs` — unit tests for URL
- Create: `crates/http-core/tests/node_test.rs` — unit tests for node builder
- Create: `crates/http-core/tests/curl_test.rs` — unit tests for curl parser
- Modify: `Cargo.toml` (workspace root) — add `crates/http-core` to members
- Modify: `crates/component-http/Cargo.toml` — add `http-core = { path = "../http-core" }` dep
- Create: `crates/component-http/src/qa.rs` — QA spec construction (reads `http-core::config` types, emits WIT types)
- Create: `crates/component-http/src/request.rs` — blocking HTTP op
- Create: `crates/component-http/src/stream.rs` — streaming HTTP op
- Modify: `crates/component-http/src/lib.rs` — reduce to Component struct + WIT exports glue; delete moved code

### Phase 2 — Extension (PR #2)

- Create: `crates/http-extension/Cargo.toml` — workspace member, wasm32-wasip2 target
- Create: `crates/http-extension/describe.json` — Greentic Store manifest
- Create: `crates/http-extension/build.sh` — build `.gtxpack`
- Create: `crates/http-extension/runtime-version.txt` — pinned `component-http` version used in generated node refs
- Create: `crates/http-extension/build.rs` — reads `runtime-version.txt`, exposes as env var
- Create: `crates/http-extension/wit/world.wit` — world importing `greentic:extension-design/*`
- Create: `crates/http-extension/src/lib.rs` — `Component` struct + `bindings::export!` macro + trait impls
- Create: `crates/http-extension/src/tools/mod.rs` — `list_tools` + `invoke_tool` dispatch
- Create: `crates/http-extension/src/tools/generate.rs` — `generate_http_node`
- Create: `crates/http-extension/src/tools/validate.rs` — `validate_http_config` helper (also called by validation interface)
- Create: `crates/http-extension/src/tools/curl_import.rs` — `curl_to_node`
- Create: `crates/http-extension/src/tools/auth_suggest.rs` — `suggest_auth`
- Create: `crates/http-extension/src/tools/card_submit.rs` — `generate_from_card_submit`
- Create: `crates/http-extension/prompts/rules.md`
- Create: `crates/http-extension/prompts/examples.md`
- Create: `crates/http-extension/schemas/http-node-v1.json`
- Create: `crates/http-extension/i18n/en.json`
- Create: `crates/http-extension/tests/tool_roundtrip.rs`
- Create: `crates/http-extension/tests/validate_content.rs`
- Create: `crates/http-extension/tests/prompt_fragments.rs`
- Modify: `Cargo.toml` (workspace root) — add `crates/http-extension` to members
- Create: `.github/workflows/publish-extension.yml` — tag-triggered publish
- Create: `ci/check_publisher_prefix.sh` — pre-flight check publisher allowed prefixes
- Create: `tests/gtests/README/08_http_extension_build.gtest`
- Create: `tests/gtests/README/09_http_extension_gtdx_install.gtest`

---

## Phase 1 — Extract http-core, slim component-http

### Task 1: Scaffold http-core crate

**Files:**
- Create: `crates/http-core/Cargo.toml`
- Create: `crates/http-core/src/lib.rs`
- Modify: `Cargo.toml` (workspace root) — add member

- [ ] **Step 1: Create `crates/http-core/Cargo.toml`**

```toml
[package]
name = "http-core"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Pure Rust logic shared by component-http runtime and http-extension design-time"
repository.workspace = true
authors.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror = "2.0"

[dev-dependencies]
serde_json.workspace = true
```

- [ ] **Step 2: Create `crates/http-core/src/lib.rs`**

```rust
//! Pure Rust logic shared by `component-http` (runtime) and `http-extension` (design-time).
//!
//! This crate contains:
//! - Config types + `apply_answers` (QA-answer merge)
//! - Auth type enum + header builders
//! - URL validation + placeholder extraction
//! - YGTc node stanza builder
//! - curl command parser
//!
//! No WASM-specific types or WIT bindings live here — they stay in the crates that
//! consume this library.

#![forbid(unsafe_code)]

pub mod auth;
pub mod config;
pub mod curl;
pub mod node;
pub mod url;

pub use auth::{AuthHeader, AuthType, build_auth_header};
pub use config::{ComponentConfig, ConfigError, apply_answers};
pub use curl::{CurlParseError, ParsedCurl, parse_curl};
pub use node::{ComponentRef, NodeBuilder, YgtcNode};
pub use url::{UrlError, extract_placeholders, validate_url};
```

- [ ] **Step 3: Stub out each module file**

Create each of these with a single `// TODO` line so the crate compiles. These are filled in Tasks 2-6.

```bash
for f in auth config curl node url; do
  echo "//! Stub — implementation in Task N of the plan." > crates/http-core/src/${f}.rs
done
```

That stub doesn't compile because `lib.rs` re-exports from empty modules. Instead, create each file with empty placeholder items:

```rust
// crates/http-core/src/auth.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthType { None }
#[derive(Debug, Clone)]
pub struct AuthHeader { pub name: String, pub value: String }
pub fn build_auth_header(_t: AuthType, _token: Option<&str>, _header: Option<&str>) -> Option<AuthHeader> { None }
```

```rust
// crates/http-core/src/config.rs
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentConfig {}
#[derive(Debug, thiserror::Error)]
pub enum ConfigError { #[error("todo")] Todo }
pub fn apply_answers(_cfg: ComponentConfig, _answers: &serde_json::Value) -> Result<ComponentConfig, ConfigError> { Ok(ComponentConfig::default()) }
```

```rust
// crates/http-core/src/curl.rs
#[derive(Debug, Clone, Default)]
pub struct ParsedCurl {}
#[derive(Debug, thiserror::Error)]
pub enum CurlParseError { #[error("todo")] Todo }
pub fn parse_curl(_cmd: &str) -> Result<ParsedCurl, CurlParseError> { Ok(ParsedCurl::default()) }
```

```rust
// crates/http-core/src/node.rs
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YgtcNode {}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRef { pub oci: String }
#[derive(Default)]
pub struct NodeBuilder {}
impl NodeBuilder { pub fn new() -> Self { Self::default() } pub fn build(self) -> YgtcNode { YgtcNode::default() } }
```

```rust
// crates/http-core/src/url.rs
#[derive(Debug, thiserror::Error)]
pub enum UrlError { #[error("todo")] Todo }
pub fn validate_url(_u: &str) -> Result<(), UrlError> { Ok(()) }
pub fn extract_placeholders(_u: &str) -> Vec<String> { Vec::new() }
```

- [ ] **Step 4: Add `crates/http-core` to workspace members**

Modify `Cargo.toml` (workspace root), add `crates/http-core` to the `[workspace] members` array (keep alphabetical ordering).

- [ ] **Step 5: Verify workspace builds**

Run: `cargo build -p http-core`
Expected: `Compiling http-core v<workspace-version> ...` then `Finished` with no errors.

- [ ] **Step 6: Commit**

```bash
git checkout -b refactor/extract-http-core
git add crates/http-core Cargo.toml
git commit -m "refactor(http-core): scaffold crate skeleton with stub modules"
```

---

### Task 2: Port ComponentConfig + apply_answers into http-core::config

**Files:**
- Modify: `crates/http-core/src/config.rs`
- Create: `crates/http-core/tests/config_test.rs`

Source lines to migrate: `crates/component-http/src/lib.rs:76-98` (ComponentConfig struct), plus the `default_*` functions and any `apply_answers` logic currently inline in the operation handlers.

- [ ] **Step 1: Write failing test `crates/http-core/tests/config_test.rs`**

```rust
use http_core::{ComponentConfig, apply_answers};
use serde_json::json;

#[test]
fn default_config_has_post_method_and_30s_timeout() {
    let cfg = ComponentConfig::default();
    assert_eq!(cfg.auth_type, "none");
    assert_eq!(cfg.api_key_header, "X-API-Key");
    assert_eq!(cfg.timeout_ms, 30000);
    assert!(cfg.base_url.is_none());
    assert!(cfg.auth_token.is_none());
    assert!(cfg.default_headers.is_none());
}

#[test]
fn apply_answers_merges_base_url_auth_and_timeout() {
    let answers = json!({
        "base_url": "https://api.example.com",
        "auth_type": "bearer",
        "auth_token": "secret:HTTP_TOKEN",
        "timeout_ms": 15000,
        "default_headers": {"X-Team": "qa"}
    });
    let cfg = apply_answers(ComponentConfig::default(), &answers).expect("apply succeeds");
    assert_eq!(cfg.base_url.as_deref(), Some("https://api.example.com"));
    assert_eq!(cfg.auth_type, "bearer");
    assert_eq!(cfg.auth_token.as_deref(), Some("secret:HTTP_TOKEN"));
    assert_eq!(cfg.timeout_ms, 15000);
    assert!(cfg.default_headers.is_some());
}

#[test]
fn apply_answers_rejects_non_positive_timeout() {
    let answers = json!({ "timeout_ms": 0 });
    let err = apply_answers(ComponentConfig::default(), &answers).expect_err("must fail");
    assert!(format!("{err}").contains("timeout_ms"));
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-core --test config_test`
Expected: FAIL — `assertion failed: cfg.auth_type == "none"` (stub default has empty String).

- [ ] **Step 3: Implement config module**

Replace `crates/http-core/src/config.rs` with:

```rust
//! HTTP component configuration — ported from `component-http/src/lib.rs`.
//!
//! Keep this module serializable and stable: it is persisted by the Greentic
//! setup engine as QA answers.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_TIMEOUT_MS: u32 = 30_000;
pub const MAX_TIMEOUT_MS: u32 = 60_000;
pub const DEFAULT_API_KEY_HEADER: &str = "X-API-Key";
pub const DEFAULT_AUTH_TYPE: &str = "none";

fn default_auth_type() -> String { DEFAULT_AUTH_TYPE.to_string() }
fn default_api_key_header() -> String { DEFAULT_API_KEY_HEADER.to_string() }
fn default_timeout() -> u32 { DEFAULT_TIMEOUT_MS }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default = "default_api_key_header")]
    pub api_key_header: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u32,
    #[serde(default)]
    pub default_headers: Option<Value>,
}

impl Default for ComponentConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            auth_type: default_auth_type(),
            auth_token: None,
            api_key_header: default_api_key_header(),
            timeout_ms: default_timeout(),
            default_headers: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid base_url: {0}")]
    InvalidBaseUrl(String),
    #[error("unsupported auth_type: {0} (expected: none | bearer | api_key | basic)")]
    UnsupportedAuthType(String),
    #[error("timeout_ms must be between 1 and {}, got {0}", MAX_TIMEOUT_MS)]
    InvalidTimeout(u32),
    #[error("default_headers must be an object, got {0}")]
    InvalidHeaders(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn apply_answers(mut cfg: ComponentConfig, answers: &Value) -> Result<ComponentConfig, ConfigError> {
    if let Some(v) = answers.get("base_url").and_then(Value::as_str) {
        validate_base_url(v)?;
        cfg.base_url = Some(v.to_string());
    }
    if let Some(v) = answers.get("auth_type").and_then(Value::as_str) {
        validate_auth_type(v)?;
        cfg.auth_type = v.to_string();
    }
    if let Some(v) = answers.get("auth_token").and_then(Value::as_str) {
        cfg.auth_token = Some(v.to_string());
    }
    if let Some(v) = answers.get("api_key_header").and_then(Value::as_str) {
        cfg.api_key_header = v.to_string();
    }
    if let Some(v) = answers.get("timeout_ms").and_then(Value::as_u64) {
        let t = u32::try_from(v).map_err(|_| ConfigError::InvalidTimeout(u32::MAX))?;
        validate_timeout(t)?;
        cfg.timeout_ms = t;
    }
    if let Some(v) = answers.get("default_headers") {
        validate_headers(v)?;
        cfg.default_headers = Some(v.clone());
    }
    Ok(cfg)
}

fn validate_base_url(url: &str) -> Result<(), ConfigError> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err(ConfigError::InvalidBaseUrl(url.to_string()))
    }
}

fn validate_auth_type(t: &str) -> Result<(), ConfigError> {
    match t {
        "none" | "bearer" | "api_key" | "basic" => Ok(()),
        other => Err(ConfigError::UnsupportedAuthType(other.to_string())),
    }
}

fn validate_timeout(ms: u32) -> Result<(), ConfigError> {
    if ms == 0 || ms > MAX_TIMEOUT_MS {
        Err(ConfigError::InvalidTimeout(ms))
    } else {
        Ok(())
    }
}

fn validate_headers(v: &Value) -> Result<(), ConfigError> {
    if v.is_object() { Ok(()) } else { Err(ConfigError::InvalidHeaders(v.to_string())) }
}
```

- [ ] **Step 4: Run test, verify pass**

Run: `cargo test -p http-core --test config_test`
Expected: `test result: ok. 3 passed; 0 failed`.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p http-core --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/http-core/src/config.rs crates/http-core/tests/config_test.rs
git commit -m "refactor(http-core): port ComponentConfig + apply_answers from component-http"
```

---

### Task 3: Port auth types + header builders into http-core::auth

**Files:**
- Modify: `crates/http-core/src/auth.rs`
- Create: `crates/http-core/tests/auth_test.rs`

- [ ] **Step 1: Write failing test `crates/http-core/tests/auth_test.rs`**

```rust
use http_core::auth::{AuthType, build_auth_header};

#[test]
fn none_returns_no_header() {
    assert!(build_auth_header(AuthType::None, None, None).is_none());
}

#[test]
fn bearer_builds_authorization_header() {
    let h = build_auth_header(AuthType::Bearer, Some("abc123"), None).expect("header");
    assert_eq!(h.name, "Authorization");
    assert_eq!(h.value, "Bearer abc123");
}

#[test]
fn bearer_missing_token_returns_none() {
    assert!(build_auth_header(AuthType::Bearer, None, None).is_none());
}

#[test]
fn api_key_uses_custom_header_name() {
    let h = build_auth_header(AuthType::ApiKey, Some("k"), Some("X-My-Key")).expect("header");
    assert_eq!(h.name, "X-My-Key");
    assert_eq!(h.value, "k");
}

#[test]
fn basic_base64_encodes_user_pass() {
    let h = build_auth_header(AuthType::Basic, Some("alice:s3cret"), None).expect("header");
    assert_eq!(h.name, "Authorization");
    // base64 of "alice:s3cret" = "YWxpY2U6czNjcmV0"
    assert_eq!(h.value, "Basic YWxpY2U6czNjcmV0");
}

#[test]
fn auth_type_parses_string() {
    assert_eq!(AuthType::from_str("bearer"), Some(AuthType::Bearer));
    assert_eq!(AuthType::from_str("api_key"), Some(AuthType::ApiKey));
    assert_eq!(AuthType::from_str("nope"), None);
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-core --test auth_test`
Expected: FAIL — stub `AuthType` only has `None` variant.

- [ ] **Step 3: Implement auth module**

Replace `crates/http-core/src/auth.rs`:

```rust
//! Auth header builders. Pure in-process logic; no I/O.
//!
//! Supported auth types: none, bearer, api_key, basic.
//! OAuth2 flows are handled by a separate extension (out of scope).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    None,
    Bearer,
    ApiKey,
    Basic,
}

impl AuthType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "none"    => Some(Self::None),
            "bearer"  => Some(Self::Bearer),
            "api_key" => Some(Self::ApiKey),
            "basic"   => Some(Self::Basic),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None    => "none",
            Self::Bearer  => "bearer",
            Self::ApiKey  => "api_key",
            Self::Basic   => "basic",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthHeader {
    pub name: String,
    pub value: String,
}

pub fn build_auth_header(t: AuthType, token: Option<&str>, api_key_header: Option<&str>) -> Option<AuthHeader> {
    match t {
        AuthType::None => None,
        AuthType::Bearer => token.map(|tok| AuthHeader {
            name: "Authorization".into(),
            value: format!("Bearer {tok}"),
        }),
        AuthType::ApiKey => token.map(|tok| AuthHeader {
            name: api_key_header.unwrap_or("X-API-Key").to_string(),
            value: tok.to_string(),
        }),
        AuthType::Basic => token.map(|user_pass| AuthHeader {
            name: "Authorization".into(),
            value: format!("Basic {}", base64_encode(user_pass.as_bytes())),
        }),
    }
}

/// Minimal base64 encoder for the tiny auth-header use case. ~30 lines, avoids a dep.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(4 * input.len().div_ceil(3));
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0b11) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(((b1 & 0b1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(b2 & 0b111111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn base64_empty() { assert_eq!(base64_encode(b""), ""); }
    #[test] fn base64_f()     { assert_eq!(base64_encode(b"f"), "Zg=="); }
    #[test] fn base64_foobar(){ assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy"); }
    #[test] fn base64_alice() { assert_eq!(base64_encode(b"alice:s3cret"), "YWxpY2U6czNjcmV0"); }
}
```

- [ ] **Step 4: Run test, verify pass**

Run: `cargo test -p http-core --test auth_test`
Expected: `test result: ok. 6 passed; 0 failed`.

Also run: `cargo test -p http-core`
Expected: unit tests in `auth.rs` module (4 base64 tests) plus integration tests all pass.

- [ ] **Step 5: Clippy clean**

Run: `cargo clippy -p http-core --all-targets -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add crates/http-core/src/auth.rs crates/http-core/tests/auth_test.rs
git commit -m "refactor(http-core): add AuthType + build_auth_header with inline base64"
```

---

### Task 4: Port URL validation + placeholder extraction into http-core::url

**Files:**
- Modify: `crates/http-core/src/url.rs`
- Create: `crates/http-core/tests/url_test.rs`

- [ ] **Step 1: Write failing test `crates/http-core/tests/url_test.rs`**

```rust
use http_core::url::{UrlError, extract_placeholders, validate_url};

#[test]
fn accepts_http_and_https_schemes() {
    validate_url("http://example.com").unwrap();
    validate_url("https://api.example.com/v1").unwrap();
}

#[test]
fn rejects_file_scheme() {
    let err = validate_url("file:///etc/passwd").expect_err("must fail");
    assert!(matches!(err, UrlError::UnsupportedScheme(_)));
}

#[test]
fn rejects_empty_or_relative_url() {
    assert!(validate_url("").is_err());
    assert!(validate_url("/api/users").is_err());
    assert!(validate_url("example.com").is_err());
}

#[test]
fn extract_placeholders_finds_template_vars() {
    let names = extract_placeholders("/users/${user.id}/posts/${post_id}");
    assert_eq!(names, vec!["user.id".to_string(), "post_id".to_string()]);
}

#[test]
fn extract_placeholders_deduplicates() {
    let names = extract_placeholders("${a}/${b}/${a}");
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn extract_placeholders_ignores_broken_syntax() {
    assert!(extract_placeholders("no placeholders here").is_empty());
    assert!(extract_placeholders("${unclosed").is_empty());
    assert!(extract_placeholders("}${").is_empty());
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-core --test url_test`
Expected: FAIL — stub functions.

- [ ] **Step 3: Implement url module**

Replace `crates/http-core/src/url.rs`:

```rust
//! URL validation + `${placeholder}` extraction helpers.

#[derive(Debug, thiserror::Error)]
pub enum UrlError {
    #[error("empty url")]
    Empty,
    #[error("unsupported scheme: {0} (expected http:// or https://)")]
    UnsupportedScheme(String),
}

pub fn validate_url(url: &str) -> Result<(), UrlError> {
    if url.is_empty() { return Err(UrlError::Empty); }
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        let scheme = url.split("://").next().unwrap_or(url);
        Err(UrlError::UnsupportedScheme(scheme.to_string()))
    }
}

/// Extract placeholder names (between `${` and `}`) in first-occurrence order.
/// Duplicates are removed. Unclosed or malformed placeholders are ignored.
pub fn extract_placeholders(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'$' && bytes[i + 1] == b'{' {
            // find matching }
            if let Some(end_rel) = s[i + 2..].find('}') {
                let name = &s[i + 2..i + 2 + end_rel];
                if !name.is_empty() && !out.iter().any(|n| n == name) {
                    out.push(name.to_string());
                }
                i += 2 + end_rel + 1;
                continue;
            } else {
                break; // unclosed placeholder, stop scanning
            }
        }
        i += 1;
    }
    out
}
```

- [ ] **Step 4: Run test, verify pass**

Run: `cargo test -p http-core --test url_test`
Expected: `test result: ok. 6 passed`.

- [ ] **Step 5: Clippy clean**

Run: `cargo clippy -p http-core --all-targets -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add crates/http-core/src/url.rs crates/http-core/tests/url_test.rs
git commit -m "refactor(http-core): add URL validation + placeholder extractor"
```

---

### Task 5: Add YGTc NodeBuilder into http-core::node

**Files:**
- Modify: `crates/http-core/src/node.rs`
- Create: `crates/http-core/tests/node_test.rs`

This type is NEW (not ported from `component-http`). It powers tool output for Phase 2.

- [ ] **Step 1: Write failing test `crates/http-core/tests/node_test.rs`**

```rust
use http_core::{ComponentConfig, NodeBuilder};
use serde_json::json;

#[test]
fn builds_post_http_node_from_config_and_inputs() {
    let cfg = ComponentConfig {
        base_url: Some("https://api.example.com".into()),
        auth_type: "bearer".into(),
        auth_token: Some("secret:CRM_TOKEN".into()),
        timeout_ms: 15000,
        default_headers: Some(json!({"Content-Type": "application/json"})),
        ..Default::default()
    };
    let node = NodeBuilder::new("post_to_crm", "oci://ghcr.io/greenticai/component/component-http:0.1.0")
        .with_config(cfg)
        .with_input("method", "POST")
        .with_input("path", "/api/leads")
        .with_rationale("bearer auth chosen from intent")
        .build();
    let j = serde_json::to_value(&node).unwrap();

    assert_eq!(j["node_id"], "post_to_crm");
    assert_eq!(j["component"], "oci://ghcr.io/greenticai/component/component-http:0.1.0");
    assert_eq!(j["config"]["base_url"], "https://api.example.com");
    assert_eq!(j["config"]["auth_type"], "bearer");
    assert_eq!(j["config"]["timeout_ms"], 15000);
    assert_eq!(j["inputs"]["method"], "POST");
    assert_eq!(j["inputs"]["path"], "/api/leads");
    assert_eq!(j["rationale"], "bearer auth chosen from intent");
}

#[test]
fn output_is_deterministic() {
    let cfg = ComponentConfig { base_url: Some("https://x".into()), ..Default::default() };
    let n1 = NodeBuilder::new("n", "oci://r:1").with_config(cfg.clone()).build();
    let n2 = NodeBuilder::new("n", "oci://r:1").with_config(cfg).build();
    let s1 = serde_json::to_string(&n1).unwrap();
    let s2 = serde_json::to_string(&n2).unwrap();
    assert_eq!(s1, s2);
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-core --test node_test`
Expected: FAIL — stub `NodeBuilder`.

- [ ] **Step 3: Implement node module**

Replace `crates/http-core/src/node.rs`:

```rust
//! YGTc node stanza builder. Output is deterministic (sorted keys via BTreeMap).
use crate::config::ComponentConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRef {
    pub oci: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YgtcNode {
    pub node_id: String,
    pub component: String,
    pub config: ComponentConfig,
    pub inputs: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<serde_json::Value>,
}

pub struct NodeBuilder {
    node_id: String,
    component: String,
    config: ComponentConfig,
    inputs: BTreeMap<String, serde_json::Value>,
    rationale: Option<String>,
    mapping: Option<serde_json::Value>,
}

impl NodeBuilder {
    pub fn new(node_id: impl Into<String>, component_ref: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            component: component_ref.into(),
            config: ComponentConfig::default(),
            inputs: BTreeMap::new(),
            rationale: None,
            mapping: None,
        }
    }
    pub fn with_config(mut self, cfg: ComponentConfig) -> Self { self.config = cfg; self }
    pub fn with_input(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.inputs.insert(key.into(), value.into());
        self
    }
    pub fn with_rationale(mut self, r: impl Into<String>) -> Self { self.rationale = Some(r.into()); self }
    pub fn with_mapping(mut self, m: serde_json::Value) -> Self { self.mapping = Some(m); self }
    pub fn build(self) -> YgtcNode {
        YgtcNode {
            node_id: self.node_id,
            component: self.component,
            config: self.config,
            inputs: self.inputs,
            rationale: self.rationale,
            mapping: self.mapping,
        }
    }
}
```

- [ ] **Step 4: Run test, verify pass**

Run: `cargo test -p http-core --test node_test`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 5: Clippy clean**

Run: `cargo clippy -p http-core --all-targets -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add crates/http-core/src/node.rs crates/http-core/tests/node_test.rs
git commit -m "feat(http-core): add YgtcNode + NodeBuilder for design-time tools"
```

---

### Task 6: Port curl parser into http-core::curl

**Files:**
- Modify: `crates/http-core/src/curl.rs`
- Create: `crates/http-core/tests/curl_test.rs`

Curl parsing is NEW logic (no existing code in component-http). Minimal but correct for common developer copy-pastes.

- [ ] **Step 1: Write failing test `crates/http-core/tests/curl_test.rs`**

```rust
use http_core::{ParsedCurl, parse_curl};

#[test]
fn parses_post_with_headers_and_body() {
    let cmd = r#"curl -X POST https://api.example.com/users \
        -H 'Content-Type: application/json' \
        -H 'Authorization: Bearer mytoken' \
        -d '{"name":"alice"}' "#;
    let p = parse_curl(cmd).unwrap();
    assert_eq!(p.method.as_deref(), Some("POST"));
    assert_eq!(p.url.as_deref(), Some("https://api.example.com/users"));
    assert_eq!(p.headers.get("Content-Type").map(String::as_str), Some("application/json"));
    assert_eq!(p.headers.get("Authorization").map(String::as_str), Some("Bearer mytoken"));
    assert_eq!(p.body.as_deref(), Some(r#"{"name":"alice"}"#));
    assert!(p.unsupported_flags.is_empty());
}

#[test]
fn defaults_to_get_when_no_method_and_no_body() {
    let p = parse_curl("curl https://api.example.com/users").unwrap();
    assert_eq!(p.method.as_deref(), Some("GET"));
}

#[test]
fn defaults_to_post_when_body_present_and_no_method() {
    let p = parse_curl(r#"curl https://api.example.com -d 'x=y'"#).unwrap();
    assert_eq!(p.method.as_deref(), Some("POST"));
}

#[test]
fn reports_unsupported_flags() {
    let p = parse_curl(r#"curl -F 'file=@a.txt' https://api.example.com/upload"#).unwrap();
    assert!(p.unsupported_flags.contains(&"-F".to_string()));
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-core --test curl_test`
Expected: FAIL.

- [ ] **Step 3: Implement curl module**

Replace `crates/http-core/src/curl.rs`:

```rust
//! Minimal curl command parser covering the subset devs typically paste:
//! method via `-X`, URL, `-H` headers, `-d` / `--data` / `--data-raw` body.
//! Other flags are recorded in `unsupported_flags`.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct ParsedCurl {
    pub method: Option<String>,
    pub url: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub unsupported_flags: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CurlParseError {
    #[error("input is empty or does not start with `curl`")]
    NotCurl,
    #[error("failed to tokenize: {0}")]
    Tokenize(String),
}

pub fn parse_curl(cmd: &str) -> Result<ParsedCurl, CurlParseError> {
    let tokens = tokenize(cmd)?;
    let mut it = tokens.into_iter();
    match it.next().as_deref() {
        Some("curl") => {},
        _ => return Err(CurlParseError::NotCurl),
    }
    let mut p = ParsedCurl::default();
    while let Some(tok) = it.next() {
        match tok.as_str() {
            "-X" | "--request" => { p.method = it.next(); }
            "-H" | "--header" => {
                if let Some(h) = it.next() {
                    if let Some((name, val)) = h.split_once(':') {
                        p.headers.insert(name.trim().to_string(), val.trim().to_string());
                    }
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => { p.body = it.next(); }
            flag if flag.starts_with('-') => { p.unsupported_flags.push(flag.to_string()); }
            other => {
                if p.url.is_none() && (other.starts_with("http://") || other.starts_with("https://")) {
                    p.url = Some(other.to_string());
                }
            }
        }
    }
    // Method defaults
    if p.method.is_none() {
        p.method = Some(if p.body.is_some() { "POST".into() } else { "GET".into() });
    }
    Ok(p)
}

/// Shell-ish tokenizer handling `'...'`, `"..."`, and backslash-newline line continuations.
/// Not POSIX-complete, but covers standard curl copy-pastes.
fn tokenize(input: &str) -> Result<Vec<String>, CurlParseError> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = input.chars().peekable();
    let mut in_s = false;
    let mut in_d = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('\n')) => { chars.next(); }
            '\'' if !in_d => { in_s = !in_s; }
            '"' if !in_s  => { in_d = !in_d; }
            c if c.is_whitespace() && !in_s && !in_d => {
                if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
            }
            c => { cur.push(c); }
        }
    }
    if in_s || in_d { return Err(CurlParseError::Tokenize("unterminated quote".into())); }
    if !cur.is_empty() { out.push(cur); }
    Ok(out)
}
```

- [ ] **Step 4: Run test, verify pass**

Run: `cargo test -p http-core --test curl_test`
Expected: `test result: ok. 4 passed`.

- [ ] **Step 5: Clippy clean**

Run: `cargo clippy -p http-core --all-targets -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add crates/http-core/src/curl.rs crates/http-core/tests/curl_test.rs
git commit -m "feat(http-core): add minimal curl command parser for import tooling"
```

---

### Task 7: Refactor component-http to depend on http-core

**Files:**
- Modify: `crates/component-http/Cargo.toml` (add dep)
- Modify: `crates/component-http/src/lib.rs` (trim)
- Create: `crates/component-http/src/qa.rs`
- Create: `crates/component-http/src/request.rs`
- Create: `crates/component-http/src/stream.rs`

**Goal**: zero behavior change. All existing tests + gtest `07_component-http_add_to_flow.gtest` must pass unchanged.

- [ ] **Step 1: Baseline — run existing tests to confirm green before refactor**

Run: `make -C crates/component-http wasm && cargo test -p component-http --target wasm32-wasip2` (or whatever the Makefile target is for local tests).

Also run: `bash tests/run_gtest.sh tests/gtests/README/07_component-http_add_to_flow.gtest` (or equivalent driver).

Expected: all green. Save output — you'll compare against post-refactor output.

- [ ] **Step 2: Add http-core dep to component-http/Cargo.toml**

Modify `crates/component-http/Cargo.toml`, under `[dependencies]`:

```toml
http-core = { path = "../http-core" }
```

- [ ] **Step 3: Extract QA spec builder into `qa.rs`**

Identify in `crates/component-http/src/lib.rs` the ~300-line block that builds `ComponentQaSpec` / `Question` with i18n keys (this is what currently makes the file 1235 lines). Move that block verbatim into `crates/component-http/src/qa.rs`. Export one function:

```rust
// crates/component-http/src/qa.rs
// Header docstring: QA spec construction for the HTTP component.
use crate::bindings_ref::*; // keep the same WIT imports as lib.rs used before
use http_core::config::{DEFAULT_API_KEY_HEADER, DEFAULT_TIMEOUT_MS};
// ... (paste QA builder here unchanged, minus `ComponentConfig` defaults which now come from http_core)

pub fn build_qa_spec(mode: &str) -> ComponentQaSpec { /* existing body */ }
```

The exact function signatures depend on what the original lib.rs exposed. Keep them identical so `lib.rs` re-export + call sites don't change.

- [ ] **Step 4: Extract request/stream ops into dedicated files**

Move the `request` op body from `lib.rs` into `crates/component-http/src/request.rs`:

```rust
// crates/component-http/src/request.rs
use crate::bindings_ref::*;
use http_core::{AuthType, ComponentConfig, build_auth_header};

pub fn run_request(config: &ComponentConfig, input: serde_json::Value) -> Result<serde_json::Value, String> {
    // paste existing request logic here, replace inline auth/url validators
    // with calls to http_core functions.
}
```

Same for `stream` into `stream.rs`.

- [ ] **Step 5: Reduce `lib.rs` to ~100 lines**

New `crates/component-http/src/lib.rs` structure:

```rust
#![allow(unsafe_op_in_unsafe_fn)]

#[allow(warnings)]
mod bindings;
mod qa;
mod request;
mod stream;

// Re-export WIT types under a consistent path so sibling modules can `use`.
mod bindings_ref {
    pub use crate::bindings::*;
}

use bindings::exports::greentic::component_v0_6::node::Guest;
use http_core::{ComponentConfig, apply_answers};

const COMPONENT_ID: &str = "http";
const WORLD_ID: &str = "component-v0-v6-v0";
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(target_arch = "wasm32")]
#[used]
#[unsafe(link_section = ".greentic.wasi")]
static WASI_TARGET_MARKER: [u8; 13] = *b"wasm32-wasip2";

struct Component;

impl Guest for Component {
    fn describe() -> String { /* build DescribePayload using http-core config types */ }
    fn qa_spec(mode: String) -> String {
        serde_json::to_string(&qa::build_qa_spec(&mode)).unwrap()
    }
    fn apply_answers(config_json: String, answers_json: String) -> String {
        let cfg: ComponentConfig = serde_json::from_str(&config_json).unwrap_or_default();
        let answers: serde_json::Value = serde_json::from_str(&answers_json).unwrap_or(serde_json::Value::Null);
        match apply_answers(cfg, &answers) {
            Ok(new_cfg) => serde_json::json!({ "ok": true, "config": new_cfg }).to_string(),
            Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
        }
    }
    fn invoke(op: String, input_json: String, config_json: String) -> Result<String, String> {
        let cfg: ComponentConfig = serde_json::from_str(&config_json).map_err(|e| e.to_string())?;
        let input: serde_json::Value = serde_json::from_str(&input_json).map_err(|e| e.to_string())?;
        let out = match op.as_str() {
            "request" => request::run_request(&cfg, input)?,
            "stream"  => stream::run_stream(&cfg, input)?,
            other => return Err(format!("unknown op: {other}")),
        };
        Ok(out.to_string())
    }
}

bindings::export!(Component with_types_in bindings);
```

(The exact `Guest` trait method names depend on the WIT world — copy from the existing `lib.rs` verbatim to avoid signature mismatches. The structure above is the idea.)

- [ ] **Step 6: Verify no-behavior-change**

Run: `cargo build -p component-http --target wasm32-wasip2`
Expected: builds clean.

Run: `make -C crates/component-http wasm && cargo test -p component-http --target wasm32-wasip2`
Expected: identical pass count to Step 1.

Run: `bash tests/run_gtest.sh tests/gtests/README/07_component-http_add_to_flow.gtest`
Expected: passes identically to Step 1.

- [ ] **Step 7: Line count check**

Run: `wc -l crates/component-http/src/*.rs`
Expected: every file ≤ 500 lines.

- [ ] **Step 8: Clippy clean**

Run: `cargo clippy -p component-http --target wasm32-wasip2 -- -D warnings`

- [ ] **Step 9: Commit**

```bash
git add crates/component-http/
git commit -m "refactor(component-http): delegate to http-core, split into qa/request/stream modules"
```

---

### Task 8: Run full local_check.sh + verify gtest baseline

- [ ] **Step 1: Run local CI**

Run: `ci/local_check.sh`
Expected: all checks pass (fmt, clippy, tests, gtests).

- [ ] **Step 2: If any regression, investigate + fix (do not proceed until green)**

Common failure modes:
- `ComponentConfig` serialization mismatch (e.g. `default_headers` was `Option<String>` before, now `Option<Value>`) — if existing QA answers YAML depends on string form, adjust back to `Option<String>` in `http-core::config`, re-run tests.
- Missing `default_*` functions referenced by `#[serde(default = "...")]` — ensure they're still in scope after the move.

- [ ] **Step 3: Push PR #1**

```bash
git push -u origin refactor/extract-http-core
gh pr create \
  --title "refactor: extract http-core from component-http" \
  --body "$(cat <<'EOF'
## Summary
- Extract pure-Rust validation/auth/URL/curl/node-builder logic into new `http-core` crate
- Slim `component-http` to thin WIT guest that delegates to `http-core`
- Zero behavior change: existing tests + gtests pass unchanged
- Sets up Phase 2: new `http-extension` crate will reuse `http-core`

## Test plan
- [x] `cargo test -p http-core` green (config/auth/url/node/curl unit tests)
- [x] `cargo test -p component-http --target wasm32-wasip2` identical pass count
- [x] `tests/gtests/README/07_component-http_add_to_flow.gtest` passes unchanged
- [x] `ci/local_check.sh` green

Spec: `docs/superpowers/specs/2026-04-19-greentic-http-extension-design.md`
Plan: `docs/superpowers/plans/2026-04-19-greentic-http-extension.md` (Phase 1, Tasks 1-9)
EOF
)"
```

---

### Task 9: Merge PR #1 (wait for approval), checkpoint before Phase 2

- [ ] **Step 1: Wait for review + merge to main**

Do not proceed to Phase 2 until PR #1 is merged. The `refactor/extract-http-core` branch becomes the baseline for `feat/http-extension`.

- [ ] **Step 2: After merge, rebase / start Phase 2 branch**

```bash
git checkout main
git pull origin main
git checkout -b feat/http-extension
```

---

## Phase 2 — Build http-extension

### Task 10: Scaffold http-extension crate with gtdx new

**Files:**
- Create: `crates/http-extension/` (all files via `gtdx new`)
- Modify: `Cargo.toml` (workspace root) — add member

- [ ] **Step 1: Verify `gtdx` is installed**

Run: `gtdx --version`
Expected: prints version.

If missing, install from local repo: `cargo install --path ../greentic-designer-extensions/crates/greentic-ext-cli` (adjust path as needed).

- [ ] **Step 2: Scaffold via gtdx new**

```bash
cd crates
gtdx new http-extension --kind design --id greentic.http --author "Greentic" --license MIT
cd ..
```

Expected: creates `crates/http-extension/` with `Cargo.toml`, `describe.json`, `src/lib.rs`, `wit/world.wit`, `prompts/`, etc.

- [ ] **Step 3: Add to workspace**

Modify `Cargo.toml` (workspace root), add `crates/http-extension` to the members list (alphabetical).

- [ ] **Step 4: Verify build**

Run: `cargo build -p http-extension --target wasm32-wasip2`
Expected: builds clean (scaffolded skeleton compiles).

- [ ] **Step 5: Commit**

```bash
git add crates/http-extension Cargo.toml
git commit -m "feat(http-extension): scaffold DesignExtension crate via gtdx new"
```

---

### Task 11: Customize describe.json to match spec

**Files:**
- Modify: `crates/http-extension/describe.json`

- [ ] **Step 1: Set explicit version in Cargo.toml (not workspace)**

The `http-extension` version is independent of `components-public` workspace version (per spec §Versioning). Ensure `crates/http-extension/Cargo.toml` has:

```toml
[package]
name = "http-extension"
version = "0.1.0"    # explicit, NOT version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
```

If the `gtdx new` scaffold emitted `version.workspace = true`, replace with the explicit line shown above.

- [ ] **Step 2: Replace describe.json with spec-conformant content**

```json
{
  "$schema": "https://store.greentic.ai/schemas/describe-v1.json",
  "apiVersion": "greentic.ai/v1",
  "kind": "DesignExtension",
  "metadata": {
    "id": "greentic.http",
    "name": "HTTP Client",
    "version": "0.1.0",
    "summary": "Generate and validate HTTP call nodes for Greentic flows",
    "description": "Teaches the Designer LLM to generate YGTc HTTP nodes from natural language, curl commands, or card submit contexts. Wraps component-http runtime — does not make HTTP calls itself.",
    "author": { "name": "Greentic", "email": "team@greentic.ai" },
    "license": "MIT",
    "repository": "https://github.com/greenticai/components-public",
    "keywords": ["http", "api", "rest", "curl", "design-extension"]
  },
  "engine": {
    "greenticDesigner": ">=0.6.0",
    "extRuntime": "^0.1.0"
  },
  "capabilities": {
    "offered": [
      { "id": "greentic:http/node-generator", "version": "1.0.0" },
      { "id": "greentic:http/validate",       "version": "1.0.0" },
      { "id": "greentic:http/curl-import",    "version": "1.0.0" }
    ],
    "required": []
  },
  "runtime": {
    "component": "extension.wasm",
    "memoryLimitMB": 32,
    "permissions": {
      "network": [],
      "secrets": [],
      "callExtensionKinds": []
    }
  },
  "contributions": {
    "schemas": ["schemas/http-node-v1.json"],
    "prompts": ["prompts/rules.md", "prompts/examples.md"],
    "tools": [
      { "name": "generate_http_node",         "export": "greentic:extension-design/tools.invoke-tool" },
      { "name": "validate_http_config",       "export": "greentic:extension-design/validation.validate-content" },
      { "name": "curl_to_node",               "export": "greentic:extension-design/tools.invoke-tool" },
      { "name": "suggest_auth",               "export": "greentic:extension-design/tools.invoke-tool" },
      { "name": "generate_from_card_submit",  "export": "greentic:extension-design/tools.invoke-tool" }
    ]
  }
}
```

- [ ] **Step 3: Validate via gtdx**

Run: `gtdx validate --dir crates/http-extension`
Expected: `describe.json` OK.

- [ ] **Step 4: Commit**

```bash
git add crates/http-extension/Cargo.toml crates/http-extension/describe.json
git commit -m "feat(http-extension): fill describe.json with capabilities and 5 tools"
```

---

### Task 12: Wire runtime-version.txt + build.rs so OCI ref is build-baked

**Files:**
- Create: `crates/http-extension/runtime-version.txt`
- Create: `crates/http-extension/build.rs`
- Modify: `crates/http-extension/Cargo.toml`

- [ ] **Step 1: Create runtime-version.txt**

```bash
echo "0.1.0" > crates/http-extension/runtime-version.txt
```

(Replace `0.1.0` with the current published `component-http` version — check `crates/component-http/Cargo.toml` or `GET ghcr.io/greenticai/component/component-http` tag list. The version that exists at the time the extension ships.)

- [ ] **Step 2: Create `crates/http-extension/build.rs`**

```rust
use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let path = PathBuf::from(&manifest_dir).join("runtime-version.txt");
    let v = fs::read_to_string(&path).expect("runtime-version.txt missing").trim().to_string();
    println!("cargo:rustc-env=GREENTIC_HTTP_RUNTIME_VERSION={v}");
    println!("cargo:rerun-if-changed=runtime-version.txt");
}
```

- [ ] **Step 3: Declare build script in Cargo.toml**

Modify `crates/http-extension/Cargo.toml` — add `build = "build.rs"` to `[package]` section.

- [ ] **Step 4: Verify env var available in crate**

Add temporary sanity test to `src/lib.rs`:

```rust
#[test]
fn runtime_version_is_injected() {
    assert!(!env!("GREENTIC_HTTP_RUNTIME_VERSION").is_empty());
}
```

Run: `cargo test -p http-extension --target wasm32-wasip2 runtime_version_is_injected`
Expected: PASS.

Remove the test (it was a sanity check).

- [ ] **Step 5: Commit**

```bash
git add crates/http-extension/runtime-version.txt crates/http-extension/build.rs crates/http-extension/Cargo.toml
git commit -m "feat(http-extension): wire runtime-version.txt via build.rs env var"
```

---

### Task 13: Implement generate_http_node tool (TDD)

**Files:**
- Create: `crates/http-extension/src/tools/mod.rs`
- Create: `crates/http-extension/src/tools/generate.rs`
- Modify: `crates/http-extension/src/lib.rs` (wire module)
- Create: `crates/http-extension/tests/generate_tool.rs`

- [ ] **Step 1: Add http-core dep to Cargo.toml**

Modify `crates/http-extension/Cargo.toml`:

```toml
[dependencies]
serde.workspace = true
serde_json.workspace = true
http-core = { path = "../http-core" }
# (keep existing greentic-ext-contract WIT + wit-bindgen deps from scaffold)
```

- [ ] **Step 2: Write failing test `crates/http-extension/tests/generate_tool.rs`**

```rust
use http_extension::tools::generate::generate_http_node;
use serde_json::json;

#[test]
fn generates_node_with_intent_only() {
    let args = json!({
        "intent": "POST to CRM /api/leads with JSON body, bearer auth",
        "context": {
            "base_url_hint": "https://crm.example.com",
            "secret_names": ["CRM_TOKEN"]
        }
    });
    let out = generate_http_node(&args).expect("generate ok");
    let j = serde_json::from_str::<serde_json::Value>(&out).unwrap();
    assert_eq!(j["config"]["base_url"], "https://crm.example.com");
    assert_eq!(j["config"]["auth_type"], "bearer");
    assert_eq!(j["config"]["auth_token"], "secret:CRM_TOKEN");
    assert_eq!(j["inputs"]["method"], "POST");
    assert!(j["component"].as_str().unwrap().starts_with("oci://ghcr.io/greenticai/component/component-http:"));
    assert!(j["rationale"].is_string());
}

#[test]
fn falls_back_to_generic_secret_name_when_none_provided() {
    let args = json!({ "intent": "GET from api.example.com with bearer", "context": {} });
    let out = generate_http_node(&args).expect("ok");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(j["config"]["auth_token"], "secret:HTTP_TOKEN");
}

#[test]
fn rejects_missing_intent() {
    let args = json!({ "context": {} });
    let err = generate_http_node(&args).expect_err("must fail");
    assert!(err.contains("intent"));
}
```

- [ ] **Step 3: Run test, verify fail**

Run: `cargo test -p http-extension --test generate_tool`
Expected: FAIL — module doesn't exist.

- [ ] **Step 4: Create `crates/http-extension/src/tools/mod.rs`**

```rust
//! Tool dispatch layer for the HTTP DesignExtension.
pub mod generate;
// pub mod validate; — filled in Task 14
// pub mod curl_import; — filled in Task 15
// pub mod auth_suggest; — filled in Task 16
// pub mod card_submit; — filled in Task 17

pub const RUNTIME_VERSION: &str = env!("GREENTIC_HTTP_RUNTIME_VERSION");

pub fn runtime_component_ref() -> String {
    format!("oci://ghcr.io/greenticai/component/component-http:{RUNTIME_VERSION}")
}
```

- [ ] **Step 5: Implement `crates/http-extension/src/tools/generate.rs`**

```rust
//! `generate_http_node` — produce a YGTc HTTP node stanza from natural language intent.

use http_core::{ComponentConfig, NodeBuilder};
use serde_json::Value;
use super::runtime_component_ref;

pub fn generate_http_node(args: &Value) -> Result<String, String> {
    let intent = args.get("intent").and_then(Value::as_str).ok_or("missing required field: intent")?;
    let context = args.get("context").cloned().unwrap_or_else(|| serde_json::json!({}));

    let base_url = context.get("base_url_hint").and_then(Value::as_str).map(String::from)
        .or_else(|| detect_url_from_intent(intent));
    let auth_type = detect_auth_type(intent);
    let method = detect_method(intent);
    let path = detect_path(intent).unwrap_or_default();

    let secret_names: Vec<String> = context.get("secret_names")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let token = pick_token(auth_type, &secret_names);
    let node_id = derive_node_id(intent, method);

    let mut cfg = ComponentConfig {
        base_url,
        auth_type: auth_type.to_string(),
        auth_token: token,
        ..Default::default()
    };
    if matches!(method, "POST" | "PUT" | "PATCH") {
        cfg.default_headers = Some(serde_json::json!({ "Content-Type": "application/json" }));
    }

    let node = NodeBuilder::new(node_id, runtime_component_ref())
        .with_config(cfg)
        .with_input("method", method)
        .with_input("path", path)
        .with_rationale(format!("Auth={auth_type}, method={method}. Chose from intent keywords."))
        .build();

    serde_json::to_string(&node).map_err(|e| e.to_string())
}

fn detect_auth_type(intent: &str) -> &'static str {
    let s = intent.to_lowercase();
    if s.contains("bearer")   { "bearer" }
    else if s.contains("api key") || s.contains("api-key") { "api_key" }
    else if s.contains("basic auth") { "basic" }
    else { "none" }
}

fn detect_method(intent: &str) -> &'static str {
    let s = intent.to_lowercase();
    for m in ["POST", "PUT", "PATCH", "DELETE", "GET"] {
        if s.contains(&m.to_lowercase()) { return m; }
    }
    "GET"
}

fn detect_path(intent: &str) -> Option<String> {
    // find first token starting with '/'
    intent.split_whitespace().find(|t| t.starts_with('/')).map(String::from)
}

fn detect_url_from_intent(intent: &str) -> Option<String> {
    intent.split_whitespace()
        .find(|t| t.starts_with("http://") || t.starts_with("https://"))
        .map(|u| {
            // strip path to leave base
            if let Some(scheme_end) = u.find("://") {
                if let Some(path_start) = u[scheme_end + 3..].find('/') {
                    return u[..scheme_end + 3 + path_start].to_string();
                }
            }
            u.to_string()
        })
}

fn pick_token(auth_type: &str, secret_names: &[String]) -> Option<String> {
    if auth_type == "none" { return None; }
    // prefer names matching HTTP or CRM/API context
    let preferred = secret_names.iter().find(|n| {
        let u = n.to_uppercase();
        u.contains("HTTP") || u.contains("TOKEN") || u.contains("API")
    });
    match preferred {
        Some(n) => Some(format!("secret:{n}")),
        None if secret_names.is_empty() => Some("secret:HTTP_TOKEN".into()),
        None => Some(format!("secret:{}", secret_names[0])),
    }
}

fn derive_node_id(intent: &str, method: &'static str) -> String {
    // simple: method_lower + first_noun_guess — fallback to generic
    let verb = method.to_lowercase();
    let noun = intent.split_whitespace()
        .find(|t| t.len() > 3 && !t.starts_with('/') && !t.starts_with("http") && !t.to_lowercase().contains("bearer"))
        .unwrap_or("http")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    format!("{verb}_{noun}")
}
```

- [ ] **Step 6: Wire module in `crates/http-extension/src/lib.rs`**

Add at the top of `src/lib.rs`:

```rust
pub mod tools;
```

- [ ] **Step 7: Run test, verify pass**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test generate_tool`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 8: Clippy clean**

Run: `cargo clippy -p http-extension --target wasm32-wasip2 --all-targets -- -D warnings`

- [ ] **Step 9: Commit**

```bash
git add crates/http-extension/src/tools crates/http-extension/src/lib.rs crates/http-extension/tests/generate_tool.rs crates/http-extension/Cargo.toml
git commit -m "feat(http-extension): add generate_http_node tool with intent detection"
```

---

### Task 14: Implement validate_http_config tool (TDD)

**Files:**
- Modify: `crates/http-extension/src/tools/mod.rs` — expose `validate` module
- Create: `crates/http-extension/src/tools/validate.rs`
- Create: `crates/http-extension/tests/validate_tool.rs`

- [ ] **Step 1: Write failing test `crates/http-extension/tests/validate_tool.rs`**

```rust
use http_extension::tools::validate::{validate_http_config, Diagnostic, Severity};
use serde_json::json;

fn has_code(diags: &[Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| d.code == code)
}

#[test]
fn rejects_invalid_scheme() {
    let node = json!({
        "node_id": "x",
        "component": "oci://ghcr.io/greenticai/component/component-http:0.1.0",
        "config": { "base_url": "file:///etc/passwd", "auth_type": "none", "timeout_ms": 15000 },
        "inputs": {}
    });
    let (valid, diags) = validate_http_config(&node);
    assert!(!valid);
    assert!(has_code(&diags, "url:invalid-scheme"));
}

#[test]
fn warns_on_bare_token() {
    let node = json!({
        "node_id": "x",
        "component": "oci://ghcr.io/greenticai/component/component-http:0.1.0",
        "config": { "base_url": "https://x", "auth_type": "bearer", "auth_token": "rawtokenABCDEFG", "timeout_ms": 15000 },
        "inputs": {}
    });
    let (_valid, diags) = validate_http_config(&node);
    assert!(has_code(&diags, "auth:bare-token"));
    assert!(diags.iter().find(|d| d.code == "auth:bare-token").unwrap().severity == Severity::Warning);
}

#[test]
fn warns_on_excessive_timeout() {
    let node = json!({
        "config": { "base_url": "https://x", "auth_type": "none", "timeout_ms": 120000 },
        "node_id": "x", "component": "oci://r", "inputs": {}
    });
    let (_valid, diags) = validate_http_config(&node);
    assert!(has_code(&diags, "timeout:too-long"));
}

#[test]
fn valid_node_has_no_errors() {
    let node = json!({
        "node_id": "post_users",
        "component": "oci://ghcr.io/greenticai/component/component-http:0.1.0",
        "config": { "base_url": "https://api.example.com", "auth_type": "bearer", "auth_token": "secret:HTTP_TOKEN", "timeout_ms": 15000 },
        "inputs": { "method": "POST", "path": "/users" }
    });
    let (valid, diags) = validate_http_config(&node);
    assert!(valid, "diagnostics: {diags:?}");
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test validate_tool`
Expected: FAIL — module doesn't exist.

- [ ] **Step 3: Create `crates/http-extension/src/tools/validate.rs`**

```rust
//! `validate_http_config` — diagnostics for a generated YGTc HTTP node.
use http_core::{AuthType, url::validate_url};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Severity { Error, Warning, Info }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub fn validate_http_config(node: &Value) -> (bool, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let cfg = node.get("config").cloned().unwrap_or(Value::Null);

    // base_url
    match cfg.get("base_url").and_then(Value::as_str) {
        Some(u) => if let Err(e) = validate_url(u) {
            diags.push(err("url:invalid-scheme", format!("{e}"), Some("config.base_url")));
        },
        None => diags.push(err("url:missing", "base_url is required".into(), Some("config.base_url"))),
    }

    // auth_type + token
    let auth_type_str = cfg.get("auth_type").and_then(Value::as_str).unwrap_or("none");
    match AuthType::from_str(auth_type_str) {
        None => diags.push(err("auth:unsupported-type", format!("unsupported auth_type: {auth_type_str}"), Some("config.auth_type"))),
        Some(AuthType::None) => {},
        Some(_) => {
            match cfg.get("auth_token").and_then(Value::as_str) {
                None => diags.push(err("auth:missing-token", "auth_token required for this auth_type".into(), Some("config.auth_token"))),
                Some(t) if !t.starts_with("secret:") => {
                    diags.push(warn("auth:bare-token", "auth_token looks like raw token; use secret:NAME reference".into(), Some("config.auth_token")));
                }
                _ => {}
            }
        }
    }

    // timeout
    if let Some(t) = cfg.get("timeout_ms").and_then(Value::as_u64) {
        if t == 0 { diags.push(err("timeout:zero", "timeout_ms must be > 0".into(), Some("config.timeout_ms"))); }
        if t > 60_000 { diags.push(warn("timeout:too-long", format!("timeout_ms {t} exceeds recommended 60000"), Some("config.timeout_ms"))); }
    }

    let valid = !diags.iter().any(|d| d.severity == Severity::Error);
    (valid, diags)
}

fn err(code: &str, msg: String, path: Option<&str>) -> Diagnostic {
    Diagnostic { severity: Severity::Error, code: code.into(), message: msg, path: path.map(String::from) }
}
fn warn(code: &str, msg: String, path: Option<&str>) -> Diagnostic {
    Diagnostic { severity: Severity::Warning, code: code.into(), message: msg, path: path.map(String::from) }
}
```

- [ ] **Step 4: Expose from `tools/mod.rs`**

Uncomment `pub mod validate;` in `src/tools/mod.rs`.

- [ ] **Step 5: Run test, verify pass**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test validate_tool`
Expected: `test result: ok. 4 passed`.

- [ ] **Step 6: Clippy clean**

Run: `cargo clippy -p http-extension --target wasm32-wasip2 --all-targets -- -D warnings`

- [ ] **Step 7: Commit**

```bash
git add crates/http-extension/src/tools/validate.rs crates/http-extension/src/tools/mod.rs crates/http-extension/tests/validate_tool.rs
git commit -m "feat(http-extension): add validate_http_config with stable diagnostic codes"
```

---

### Task 15: Implement curl_to_node tool (TDD)

**Files:**
- Create: `crates/http-extension/src/tools/curl_import.rs`
- Modify: `crates/http-extension/src/tools/mod.rs`
- Create: `crates/http-extension/tests/curl_tool.rs`

- [ ] **Step 1: Write failing test `crates/http-extension/tests/curl_tool.rs`**

```rust
use http_extension::tools::curl_import::curl_to_node;
use serde_json::json;

#[test]
fn converts_bearer_curl_into_node() {
    let args = json!({
        "curl_cmd": "curl -X POST https://api.example.com/users -H 'Authorization: Bearer xxx' -H 'X-Team: qa' -d '{\"name\":\"alice\"}'",
        "node_id": "create_user"
    });
    let out = curl_to_node(&args).expect("ok");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(j["node_id"], "create_user");
    assert_eq!(j["config"]["auth_type"], "bearer");
    assert_eq!(j["config"]["auth_token"], "secret:HTTP_TOKEN");
    assert_eq!(j["config"]["base_url"], "https://api.example.com");
    assert_eq!(j["inputs"]["method"], "POST");
    assert_eq!(j["inputs"]["path"], "/users");
    assert_eq!(j["config"]["default_headers"]["X-Team"], "qa");
}

#[test]
fn notes_unsupported_flags() {
    let args = json!({ "curl_cmd": "curl -F 'f=@a.txt' https://api.example.com/upload", "node_id": "up" });
    let out = curl_to_node(&args).expect("ok");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    let note = j["diagnostics"].as_array().unwrap();
    assert!(note.iter().any(|d| d["code"] == "curl:unsupported-flag"));
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test curl_tool`
Expected: FAIL — module doesn't exist.

- [ ] **Step 3: Create `crates/http-extension/src/tools/curl_import.rs`**

```rust
//! `curl_to_node` — convert a curl command into a YGTc HTTP node.

use http_core::{ComponentConfig, NodeBuilder, parse_curl};
use serde_json::{Value, json};
use super::runtime_component_ref;

pub fn curl_to_node(args: &Value) -> Result<String, String> {
    let cmd = args.get("curl_cmd").and_then(Value::as_str).ok_or("missing required field: curl_cmd")?;
    let node_id = args.get("node_id").and_then(Value::as_str).unwrap_or("http_call").to_string();

    let parsed = parse_curl(cmd).map_err(|e| format!("parse error: {e}"))?;
    let url = parsed.url.clone().ok_or("curl command did not contain a URL")?;

    let (base_url, path) = split_url(&url);

    let mut auth_type = "none";
    let mut auth_token: Option<String> = None;
    let mut remaining_headers = serde_json::Map::new();
    for (k, v) in &parsed.headers {
        if k.eq_ignore_ascii_case("Authorization") {
            if let Some(tok) = v.strip_prefix("Bearer ") {
                auth_type = "bearer";
                auth_token = Some(format!("secret:HTTP_TOKEN"));
                let _ = tok; // raw token dropped — user must replace
            } else if v.starts_with("Basic ") {
                auth_type = "basic";
                auth_token = Some("secret:HTTP_BASIC".into());
            }
        } else {
            remaining_headers.insert(k.clone(), json!(v));
        }
    }

    let cfg = ComponentConfig {
        base_url: Some(base_url),
        auth_type: auth_type.into(),
        auth_token,
        default_headers: if remaining_headers.is_empty() { None } else { Some(Value::Object(remaining_headers)) },
        ..Default::default()
    };

    let method = parsed.method.clone().unwrap_or_else(|| "GET".into());

    let mut builder = NodeBuilder::new(node_id, runtime_component_ref())
        .with_config(cfg)
        .with_input("method", method)
        .with_input("path", path);
    if let Some(body) = parsed.body.as_ref() {
        builder = builder.with_input("body_template", body.clone());
    }
    let node = builder
        .with_rationale(format!("Imported from curl. {} header(s), {} unsupported flag(s).",
            parsed.headers.len(),
            parsed.unsupported_flags.len()))
        .build();

    let mut out = serde_json::to_value(&node).map_err(|e| e.to_string())?;

    // Attach diagnostics array for unsupported flags + bare-token note
    let mut diags = Vec::new();
    if auth_token.as_deref() == Some("secret:HTTP_TOKEN") {
        diags.push(json!({"severity":"warning","code":"auth:bare-token-rewritten","message":"raw bearer token replaced with secret:HTTP_TOKEN — create this secret in the Operator"}));
    }
    for flag in &parsed.unsupported_flags {
        diags.push(json!({"severity":"info","code":"curl:unsupported-flag","message":format!("flag {flag} not mapped to HTTP node config")}));
    }
    if !diags.is_empty() { out["diagnostics"] = Value::Array(diags); }

    serde_json::to_string(&out).map_err(|e| e.to_string())
}

fn split_url(url: &str) -> (String, String) {
    if let Some(scheme_end) = url.find("://") {
        if let Some(path_start) = url[scheme_end + 3..].find('/') {
            let base = url[..scheme_end + 3 + path_start].to_string();
            let path = url[scheme_end + 3 + path_start..].to_string();
            return (base, path);
        }
    }
    (url.to_string(), "/".to_string())
}
```

- [ ] **Step 4: Expose from mod.rs**

Uncomment `pub mod curl_import;` in `src/tools/mod.rs`.

- [ ] **Step 5: Run test, verify pass**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test curl_tool`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 6: Clippy clean**

Run: `cargo clippy -p http-extension --target wasm32-wasip2 --all-targets -- -D warnings`

- [ ] **Step 7: Commit**

```bash
git add crates/http-extension/src/tools/curl_import.rs crates/http-extension/src/tools/mod.rs crates/http-extension/tests/curl_tool.rs
git commit -m "feat(http-extension): add curl_to_node with bearer rewrite + diagnostics"
```

---

### Task 16: Implement suggest_auth tool (TDD)

**Files:**
- Create: `crates/http-extension/src/tools/auth_suggest.rs`
- Modify: `crates/http-extension/src/tools/mod.rs`
- Create: `crates/http-extension/tests/auth_suggest_tool.rs`

- [ ] **Step 1: Write failing test**

```rust
// crates/http-extension/tests/auth_suggest_tool.rs
use http_extension::tools::auth_suggest::suggest_auth;
use serde_json::json;

#[test]
fn github_api_suggests_bearer_pat() {
    let args = json!({ "api_description": "GitHub REST API v3" });
    let out = suggest_auth(&args).expect("ok");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(j["auth_type"], "bearer");
    assert_eq!(j["confidence"], "high");
    assert_eq!(j["default_headers"]["Accept"], "application/vnd.github+json");
}

#[test]
fn openai_suggests_bearer() {
    let args = json!({ "api_description": "OpenAI API chat completions" });
    let out = suggest_auth(&args).expect("ok");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(j["auth_type"], "bearer");
    assert_eq!(j["confidence"], "high");
}

#[test]
fn unknown_api_returns_low_confidence() {
    let args = json!({ "api_description": "internal custom ERP system" });
    let out = suggest_auth(&args).expect("ok");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(j["auth_type"], "unknown");
    assert_eq!(j["confidence"], "low");
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test auth_suggest_tool`
Expected: FAIL.

- [ ] **Step 3: Create `crates/http-extension/src/tools/auth_suggest.rs`**

```rust
//! `suggest_auth` — recommend auth configuration for a known or described API.

use serde_json::{Value, json};

struct Pattern {
    keywords: &'static [&'static str],
    auth_type: &'static str,
    token_name: &'static str,
    api_key_header: Option<&'static str>,
    default_headers: Option<fn() -> Value>,
    rationale: &'static str,
}

const PATTERNS: &[Pattern] = &[
    Pattern {
        keywords: &["github"],
        auth_type: "bearer",
        token_name: "GITHUB_TOKEN",
        api_key_header: None,
        default_headers: Some(|| json!({"Accept": "application/vnd.github+json"})),
        rationale: "GitHub REST v3 uses Personal Access Tokens via Bearer auth. Accept header for version pinning.",
    },
    Pattern {
        keywords: &["openai", "anthropic", "cohere"],
        auth_type: "bearer",
        token_name: "LLM_API_TOKEN",
        api_key_header: None,
        default_headers: None,
        rationale: "Major LLM APIs use Bearer token auth.",
    },
    Pattern {
        keywords: &["airtable"],
        auth_type: "bearer",
        token_name: "AIRTABLE_TOKEN",
        api_key_header: None,
        default_headers: None,
        rationale: "Airtable uses Bearer Personal Access Tokens.",
    },
    Pattern {
        keywords: &["slack", "discord"],
        auth_type: "bearer",
        token_name: "WEBHOOK_TOKEN",
        api_key_header: None,
        default_headers: None,
        rationale: "Chat platform REST APIs use Bearer tokens.",
    },
];

pub fn suggest_auth(args: &Value) -> Result<String, String> {
    let desc = args.get("api_description").and_then(Value::as_str)
        .ok_or("missing required field: api_description")?;
    let lower = desc.to_lowercase();
    for pat in PATTERNS {
        if pat.keywords.iter().any(|k| lower.contains(k)) {
            let out = json!({
                "auth_type": pat.auth_type,
                "auth_token": format!("secret:{}", pat.token_name),
                "api_key_header": pat.api_key_header,
                "default_headers": pat.default_headers.map(|f| f()).unwrap_or(Value::Null),
                "rationale": pat.rationale,
                "confidence": "high"
            });
            return serde_json::to_string(&out).map_err(|e| e.to_string());
        }
    }
    // Unknown → low confidence fallback
    let out = json!({
        "auth_type": "unknown",
        "auth_token": null,
        "api_key_header": null,
        "default_headers": null,
        "rationale": "API not in known-patterns list; ask user which auth type to use.",
        "confidence": "low"
    });
    serde_json::to_string(&out).map_err(|e| e.to_string())
}
```

- [ ] **Step 4: Expose from mod.rs**

Uncomment `pub mod auth_suggest;` in `src/tools/mod.rs`.

- [ ] **Step 5: Run test, verify pass**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test auth_suggest_tool`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 6: Clippy clean**

Run: `cargo clippy -p http-extension --target wasm32-wasip2 --all-targets -- -D warnings`

- [ ] **Step 7: Commit**

```bash
git add crates/http-extension/src/tools/auth_suggest.rs crates/http-extension/src/tools/mod.rs crates/http-extension/tests/auth_suggest_tool.rs
git commit -m "feat(http-extension): add suggest_auth with known-API pattern matching"
```

---

### Task 17: Implement generate_from_card_submit tool (TDD)

**Files:**
- Create: `crates/http-extension/src/tools/card_submit.rs`
- Modify: `crates/http-extension/src/tools/mod.rs`
- Create: `crates/http-extension/tests/card_submit_tool.rs`

- [ ] **Step 1: Write failing test `crates/http-extension/tests/card_submit_tool.rs`**

```rust
use http_extension::tools::card_submit::generate_from_card_submit;
use serde_json::json;

#[test]
fn maps_input_text_fields_into_body_template() {
    let card = json!({
        "type": "AdaptiveCard",
        "version": "1.6",
        "body": [
            { "type": "Input.Text", "id": "subject", "label": "Subject" },
            { "type": "Input.Text", "id": "description", "label": "Description" },
            { "type": "Input.ChoiceSet", "id": "priority", "choices": [{"title":"High","value":"high"}] }
        ]
    });
    let args = json!({
        "card_schema": card,
        "api_intent": "POST to /api/tickets with form fields as JSON",
        "node_id": "submit_ticket"
    });
    let out = generate_from_card_submit(&args).expect("ok");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(j["node_id"], "submit_ticket");
    assert_eq!(j["inputs"]["method"], "POST");
    assert_eq!(j["inputs"]["path"], "/api/tickets");
    let tpl = j["inputs"]["body_template"].as_str().unwrap();
    assert!(tpl.contains("${submit.subject}"));
    assert!(tpl.contains("${submit.priority}"));
    assert!(tpl.contains("${submit.description}"));

    let map = &j["mapping"]["card_to_body"];
    assert_eq!(map["submit.subject"], "body.subject");
}

#[test]
fn reports_unmapped_fields_when_names_differ() {
    let card = json!({
        "type": "AdaptiveCard",
        "body": [
            { "type": "Input.Text", "id": "internal_note" }
        ]
    });
    let args = json!({
        "card_schema": card,
        "api_intent": "POST to /api/tickets",
        "node_id": "submit"
    });
    let out = generate_from_card_submit(&args).expect("ok");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    let unmapped = j["mapping"]["unmapped_card_fields"].as_array().unwrap();
    assert!(unmapped.iter().any(|v| v == "submit.internal_note"));
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test card_submit_tool`
Expected: FAIL.

- [ ] **Step 3: Create `crates/http-extension/src/tools/card_submit.rs`**

```rust
//! `generate_from_card_submit` — map Adaptive Card Input.* ids into HTTP body template.

use http_core::{ComponentConfig, NodeBuilder};
use serde_json::{Value, json};
use super::runtime_component_ref;

pub fn generate_from_card_submit(args: &Value) -> Result<String, String> {
    let card = args.get("card_schema").ok_or("missing required field: card_schema")?;
    let intent = args.get("api_intent").and_then(Value::as_str).ok_or("missing required field: api_intent")?;
    let node_id = args.get("node_id").and_then(Value::as_str).unwrap_or("http_call").to_string();

    let input_ids = extract_input_ids(card);
    let (method, path) = parse_intent(intent);

    // Build JSON body template mapping each Input.id to ${submit.<id>}
    let mut body_obj: Vec<(String, String)> = input_ids.iter()
        .map(|id| (id.clone(), format!("${{submit.{id}}}")))
        .collect();
    body_obj.sort_by(|a, b| a.0.cmp(&b.0));
    let mut parts = Vec::new();
    for (k, v) in &body_obj {
        parts.push(format!(r#""{k}":"{v}""#));
    }
    let body_template = format!("{{{}}}", parts.join(","));

    // Mapping summary
    let mut card_to_body = serde_json::Map::new();
    let mut unmapped = Vec::new();
    for id in &input_ids {
        // heuristic: unmapped if id looks non-API-ish (contains "internal", "note", "debug")
        let low = id.to_lowercase();
        if ["internal", "debug", "note_private"].iter().any(|k| low.contains(k)) {
            unmapped.push(format!("submit.{id}"));
        } else {
            card_to_body.insert(format!("submit.{id}"), json!(format!("body.{id}")));
        }
    }

    let cfg = ComponentConfig {
        base_url: Some("https://api.example.com".into()),
        auth_type: "bearer".into(),
        auth_token: Some("secret:HTTP_TOKEN".into()),
        default_headers: Some(json!({"Content-Type": "application/json"})),
        ..Default::default()
    };

    let node = NodeBuilder::new(node_id, runtime_component_ref())
        .with_config(cfg)
        .with_input("method", method)
        .with_input("path", path)
        .with_input("body_template", body_template)
        .with_mapping(json!({
            "card_to_body": Value::Object(card_to_body),
            "unmapped_card_fields": unmapped
        }))
        .with_rationale(format!("Mapped {} card Input.* fields to body template.", input_ids.len()))
        .build();

    serde_json::to_string(&node).map_err(|e| e.to_string())
}

fn extract_input_ids(card: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(card, &mut out);
    out
}
fn walk(v: &Value, out: &mut Vec<String>) {
    if let Some(arr) = v.as_array() { for item in arr { walk(item, out); } return; }
    if let Some(obj) = v.as_object() {
        let is_input = obj.get("type").and_then(Value::as_str).map(|s| s.starts_with("Input.")).unwrap_or(false);
        if is_input {
            if let Some(id) = obj.get("id").and_then(Value::as_str) {
                out.push(id.to_string());
            }
        }
        for (_, child) in obj { walk(child, out); }
    }
}

fn parse_intent(intent: &str) -> (&'static str, String) {
    let lower = intent.to_lowercase();
    let method = if lower.contains("post") { "POST" }
        else if lower.contains("put") { "PUT" }
        else if lower.contains("patch") { "PATCH" }
        else { "POST" };
    let path = intent.split_whitespace()
        .find(|t| t.starts_with('/'))
        .map(String::from)
        .unwrap_or_else(|| "/".into());
    (method, path)
}
```

- [ ] **Step 4: Expose from mod.rs**

Uncomment `pub mod card_submit;` in `src/tools/mod.rs`.

- [ ] **Step 5: Run test, verify pass**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test card_submit_tool`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 6: Clippy clean**

Run: `cargo clippy -p http-extension --target wasm32-wasip2 --all-targets -- -D warnings`

- [ ] **Step 7: Commit**

```bash
git add crates/http-extension/src/tools/card_submit.rs crates/http-extension/src/tools/mod.rs crates/http-extension/tests/card_submit_tool.rs
git commit -m "feat(http-extension): add generate_from_card_submit mapping Input.* ids to body template"
```

---

### Task 18: Wire list_tools + invoke_tool WIT dispatch

**Files:**
- Modify: `crates/http-extension/src/lib.rs`
- Modify: `crates/http-extension/src/tools/mod.rs`
- Create: `crates/http-extension/tests/tool_dispatch.rs`

- [ ] **Step 1: Write failing dispatch test**

```rust
// crates/http-extension/tests/tool_dispatch.rs
use http_extension::tools::{invoke_tool, list_tools};

#[test]
fn list_tools_returns_five_definitions() {
    let tools = list_tools();
    let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names.len(), 5);
    for expected in ["generate_http_node", "validate_http_config", "curl_to_node", "suggest_auth", "generate_from_card_submit"] {
        assert!(names.contains(&expected), "missing tool: {expected}");
    }
    // each tool has valid JSON Schema
    for t in &tools {
        let _: serde_json::Value = serde_json::from_str(&t.input_schema_json).unwrap_or_else(|_| panic!("bad schema: {}", t.name));
    }
}

#[test]
fn invoke_unknown_tool_returns_error() {
    let err = invoke_tool("nope", "{}").expect_err("must fail");
    assert!(err.contains("unknown tool"));
}

#[test]
fn invoke_generate_http_node_roundtrips() {
    let args = r#"{"intent":"GET from https://x/a","context":{}}"#;
    let out = invoke_tool("generate_http_node", args).expect("ok");
    assert!(out.contains("\"component\""));
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test tool_dispatch`
Expected: FAIL.

- [ ] **Step 3: Implement `list_tools` + `invoke_tool` in `src/tools/mod.rs`**

Append to `crates/http-extension/src/tools/mod.rs`:

```rust
use serde_json::Value;

pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema_json: String,
    pub output_schema_json: Option<String>,
}

pub fn list_tools() -> Vec<ToolDef> {
    let defs = [
        ("generate_http_node",
         "Generate a YGTc HTTP node from natural language intent",
         r#"{"type":"object","properties":{"intent":{"type":"string"},"context":{"type":"object"}},"required":["intent"]}"#),
        ("validate_http_config",
         "Validate a generated YGTc HTTP node — returns diagnostics",
         r#"{"type":"object","properties":{"node":{"type":"object"}},"required":["node"]}"#),
        ("curl_to_node",
         "Convert a curl command into a YGTc HTTP node",
         r#"{"type":"object","properties":{"curl_cmd":{"type":"string"},"node_id":{"type":"string"}},"required":["curl_cmd"]}"#),
        ("suggest_auth",
         "Recommend auth configuration from a natural-language API description",
         r#"{"type":"object","properties":{"api_description":{"type":"string"}},"required":["api_description"]}"#),
        ("generate_from_card_submit",
         "Generate an HTTP node that maps Adaptive Card submit fields to an API body",
         r#"{"type":"object","properties":{"card_schema":{"type":"object"},"api_intent":{"type":"string"},"node_id":{"type":"string"}},"required":["card_schema","api_intent"]}"#),
    ];
    defs.iter().map(|(n, d, s)| ToolDef {
        name: (*n).into(),
        description: (*d).into(),
        input_schema_json: (*s).into(),
        output_schema_json: None,
    }).collect()
}

pub fn invoke_tool(name: &str, args_json: &str) -> Result<String, String> {
    let args: Value = serde_json::from_str(args_json).map_err(|e| format!("args json: {e}"))?;
    match name {
        "generate_http_node"        => generate::generate_http_node(&args),
        "validate_http_config"      => {
            let node = args.get("node").cloned().unwrap_or(Value::Null);
            let (valid, diags) = validate::validate_http_config(&node);
            Ok(serde_json::to_string(&serde_json::json!({"valid": valid, "diagnostics": diags})).unwrap())
        }
        "curl_to_node"              => curl_import::curl_to_node(&args),
        "suggest_auth"              => auth_suggest::suggest_auth(&args),
        "generate_from_card_submit" => card_submit::generate_from_card_submit(&args),
        other => Err(format!("unknown tool: {other}")),
    }
}
```

- [ ] **Step 4: Run test, verify pass**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test tool_dispatch`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: Wire WIT guest impl in `src/lib.rs`**

Replace the scaffolded `impl tools::Guest for Component` body in `src/lib.rs`:

```rust
impl bindings::exports::greentic::extension_design::tools::Guest for Component {
    fn list_tools() -> Vec<bindings::exports::greentic::extension_design::tools::ToolDefinition> {
        crate::tools::list_tools().into_iter().map(|t| bindings::exports::greentic::extension_design::tools::ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema_json: t.input_schema_json,
            output_schema_json: t.output_schema_json,
        }).collect()
    }

    fn invoke_tool(name: String, args_json: String) -> Result<String, bindings::greentic::extension_base::types::ExtensionError> {
        crate::tools::invoke_tool(&name, &args_json).map_err(|e|
            bindings::greentic::extension_base::types::ExtensionError::InvalidInput(e))
    }
}
```

(Exact WIT paths should match the ones emitted by `wit-bindgen` for your world — copy from the adaptive-card-extension `lib.rs` if unsure; they're the same interfaces.)

- [ ] **Step 6: Wire validation::Guest for validate_content**

```rust
impl bindings::exports::greentic::extension_design::validation::Guest for Component {
    fn validate_content(content_type: String, content_json: String)
        -> bindings::exports::greentic::extension_design::validation::ValidateResult
    {
        if content_type != "http-node" {
            return bindings::exports::greentic::extension_design::validation::ValidateResult {
                valid: false,
                diagnostics: vec![bindings::greentic::extension_base::types::Diagnostic {
                    severity: bindings::greentic::extension_base::types::Severity::Error,
                    code: "unsupported-content-type".into(),
                    message: format!("this extension handles 'http-node', got '{content_type}'"),
                    path: None,
                }],
            };
        }
        let node: serde_json::Value = match serde_json::from_str(&content_json) {
            Ok(v) => v, Err(e) => return bindings::exports::greentic::extension_design::validation::ValidateResult {
                valid: false,
                diagnostics: vec![bindings::greentic::extension_base::types::Diagnostic {
                    severity: bindings::greentic::extension_base::types::Severity::Error,
                    code: "json-parse".into(),
                    message: e.to_string(),
                    path: None,
                }],
            }
        };
        let (valid, diags) = crate::tools::validate::validate_http_config(&node);
        let wit_diags = diags.into_iter().map(|d| bindings::greentic::extension_base::types::Diagnostic {
            severity: match d.severity {
                crate::tools::validate::Severity::Error => bindings::greentic::extension_base::types::Severity::Error,
                crate::tools::validate::Severity::Warning => bindings::greentic::extension_base::types::Severity::Warning,
                crate::tools::validate::Severity::Info => bindings::greentic::extension_base::types::Severity::Info,
            },
            code: d.code,
            message: d.message,
            path: d.path,
        }).collect();
        bindings::exports::greentic::extension_design::validation::ValidateResult { valid, diagnostics: wit_diags }
    }
}
```

- [ ] **Step 7: Build WASM to verify all WIT impls are covered**

Run: `cargo component build -p http-extension --release --target wasm32-wasip2`
Expected: builds clean. If `wit-bindgen` reports missing impl for any interface (`manifest`, `lifecycle`, `prompting`, `knowledge`), go to Task 19/20 to fill them.

- [ ] **Step 8: Commit**

```bash
git add crates/http-extension/src/lib.rs crates/http-extension/src/tools/mod.rs crates/http-extension/tests/tool_dispatch.rs
git commit -m "feat(http-extension): wire list_tools/invoke_tool + validation dispatch"
```

---

### Task 19: Add prompting (rules.md + examples.md) and knowledge interface stubs

**Files:**
- Modify: `crates/http-extension/src/lib.rs`
- Create: `crates/http-extension/prompts/rules.md`
- Create: `crates/http-extension/prompts/examples.md`
- Create: `crates/http-extension/tests/prompt_fragments.rs`

- [ ] **Step 1: Write failing prompt test**

```rust
// crates/http-extension/tests/prompt_fragments.rs
#[cfg(target_arch = "wasm32")]
mod wasm_only {
    // For pure-rust host test we cannot call WIT impls directly without the harness.
    // Instead, we verify the raw markdown files exist and have the expected keys.
}

#[test]
fn rules_md_mentions_secret_refs_and_timeout_defaults() {
    let s = include_str!("../prompts/rules.md");
    assert!(s.contains("secret:"));
    assert!(s.contains("15"));
    assert!(s.contains("60"));
}

#[test]
fn examples_md_has_at_least_three_samples() {
    let s = include_str!("../prompts/examples.md");
    let sample_count = s.matches("### Example").count();
    assert!(sample_count >= 3, "expected >= 3 examples, got {sample_count}");
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test prompt_fragments`
Expected: FAIL — markdown files don't exist yet.

- [ ] **Step 3: Write `crates/http-extension/prompts/rules.md`**

```markdown
# Greentic HTTP Extension — LLM Rules

When generating HTTP call nodes for Greentic flows, follow these rules.

## Security

- **NEVER** emit a raw bearer token, API key, or password in `config.auth_token`. Always use a `secret:NAME` reference. If the user pastes a raw token (e.g. via `curl_to_node`), rewrite it as `secret:HTTP_TOKEN` and warn the user to create that secret.
- **NEVER** include user PII in the node `rationale` field — the rationale is visible in flow diffs.
- Prefer HTTPS base URLs. Warn when a user requests `http://` to a non-localhost host.

## Defaults

- `timeout_ms` defaults to **15000** (15 s). Never exceed **60000** (60 s) unless the user explicitly asks for a long-poll scenario.
- For POST/PUT/PATCH with a body, **always** set `Content-Type: application/json` unless the user specifies a different content type.
- `node_id` must be `snake_case` and start with the HTTP verb (e.g. `post_to_crm`, `get_user_profile`).

## Auth type selection

- Bearer tokens: most modern REST APIs (GitHub, Slack, Airtable, OpenAI) — use `auth_type: bearer`.
- API keys in custom headers: OpenAI (legacy), some SaaS — use `auth_type: api_key` with `api_key_header`.
- Basic auth: legacy intranet, some enterprise APIs — use `auth_type: basic`, token format `user:password`.
- Unknown API: call `suggest_auth` first. If it returns `confidence: low`, ask the user.

## Tool selection

- User describes intent in natural language → `generate_http_node`
- User pastes a `curl` command → `curl_to_node`
- User is wiring a step after an Adaptive Card submit → `generate_from_card_submit`
- After ANY generation: call `validate_http_config` and act on diagnostics before returning to the user.

## Diagnostic codes (from validate_http_config)

- `url:invalid-scheme` — reject non-http(s) URL
- `url:missing` — base_url required
- `auth:unsupported-type` — auth_type not in `{none, bearer, api_key, basic}`
- `auth:missing-token` — auth_type is not `none` but `auth_token` is empty
- `auth:bare-token` — `auth_token` does not start with `secret:` — tell user to use secret reference
- `timeout:zero` / `timeout:too-long` — timeout sanity checks
- `curl:unsupported-flag` — curl command used a flag (e.g. `-F`, `--data-urlencode`) that is not mapped 1:1 to HTTP node config
```

- [ ] **Step 4: Write `crates/http-extension/prompts/examples.md`**

```markdown
# Examples — intent → HTTP node

### Example 1 — Simple POST with bearer auth

**User intent**: *"Post new leads to my CRM at crm.example.com, bearer auth"*

**Generated node**:
```json
{
  "node_id": "post_to_crm",
  "component": "oci://ghcr.io/greenticai/component/component-http:0.1.0",
  "config": {
    "base_url": "https://crm.example.com",
    "auth_type": "bearer",
    "auth_token": "secret:CRM_TOKEN",
    "timeout_ms": 15000,
    "default_headers": { "Content-Type": "application/json" }
  },
  "inputs": { "method": "POST", "path": "/api/leads" },
  "rationale": "Bearer auth from intent keyword; CRM_TOKEN inferred."
}
```

### Example 2 — curl command import

**User**: pastes `curl -X POST https://api.github.com/repos/x/y/issues -H 'Authorization: Bearer xxx' -d '{"title":"bug"}'`

**Action**: call `curl_to_node`. The raw token `xxx` is replaced with `secret:HTTP_TOKEN`; a warning diagnostic is attached.

### Example 3 — Post-card-submit ticket creation

**User flow**: card with `Input.Text` fields `subject` / `description` / `priority` → wants to send to `/api/tickets`.

**Action**: call `generate_from_card_submit`. Body template auto-generated as `{"subject":"${submit.subject}","description":"${submit.description}","priority":"${submit.priority}"}`.

### Example 4 — Unknown API requires clarification

**User intent**: *"Call our internal ERP over HTTP"*

**Action**: call `suggest_auth`. If `confidence: low`, ask user: "What auth does this API use — Bearer token, API key in header, Basic auth, or no auth?"
```

- [ ] **Step 5: Wire prompting + knowledge into `src/lib.rs`**

Add to `src/lib.rs`:

```rust
const PROMPT_RULES: &str = include_str!("../prompts/rules.md");
const PROMPT_EXAMPLES: &str = include_str!("../prompts/examples.md");

impl bindings::exports::greentic::extension_design::prompting::Guest for Component {
    fn system_prompt_fragments() -> Vec<bindings::exports::greentic::extension_design::prompting::PromptFragment> {
        use bindings::exports::greentic::extension_design::prompting::PromptFragment;
        vec![
            PromptFragment { section: "rules".into(),    content_markdown: PROMPT_RULES.into(),    priority: 100 },
            PromptFragment { section: "examples".into(), content_markdown: PROMPT_EXAMPLES.into(), priority: 50 },
        ]
    }
}

impl bindings::exports::greentic::extension_design::knowledge::Guest for Component {
    fn list_entries(_filter: Option<String>) -> Vec<bindings::exports::greentic::extension_design::knowledge::EntrySummary> { vec![] }
    fn get_entry(id: String) -> Result<bindings::exports::greentic::extension_design::knowledge::Entry, bindings::greentic::extension_base::types::ExtensionError> {
        Err(bindings::greentic::extension_base::types::ExtensionError::InvalidInput(format!("no entry: {id}")))
    }
    fn suggest_entries(_query: String, _limit: u32) -> Vec<bindings::exports::greentic::extension_design::knowledge::EntrySummary> { vec![] }
}
```

- [ ] **Step 6: Wire manifest + lifecycle**

```rust
impl bindings::exports::greentic::extension_base::manifest::Guest for Component {
    fn get_identity() -> bindings::greentic::extension_base::types::ExtensionIdentity {
        bindings::greentic::extension_base::types::ExtensionIdentity {
            id: "greentic.http".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            kind: bindings::greentic::extension_base::types::Kind::Design,
        }
    }
    fn get_offered() -> Vec<bindings::greentic::extension_base::types::CapabilityRef> {
        use bindings::greentic::extension_base::types::CapabilityRef;
        vec![
            CapabilityRef { id: "greentic:http/node-generator".into(), version: "1.0.0".into() },
            CapabilityRef { id: "greentic:http/validate".into(),       version: "1.0.0".into() },
            CapabilityRef { id: "greentic:http/curl-import".into(),    version: "1.0.0".into() },
        ]
    }
    fn get_required() -> Vec<bindings::greentic::extension_base::types::CapabilityRef> { vec![] }
}

impl bindings::exports::greentic::extension_base::lifecycle::Guest for Component {
    fn init(_config_json: String) -> Result<(), bindings::greentic::extension_base::types::ExtensionError> { Ok(()) }
    fn shutdown() {}
}
```

- [ ] **Step 7: Run test, verify pass**

Run: `cargo test -p http-extension --target wasm32-wasip2 --test prompt_fragments`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 8: Build WASM**

Run: `cargo component build -p http-extension --release --target wasm32-wasip2`
Expected: builds clean, no unimplemented WIT exports.

- [ ] **Step 9: Commit**

```bash
git add crates/http-extension/prompts crates/http-extension/src/lib.rs crates/http-extension/tests/prompt_fragments.rs
git commit -m "feat(http-extension): add LLM prompt rules + examples + manifest/lifecycle/prompting/knowledge impls"
```

---

### Task 20: Add JSON schema for http-node + i18n stub

**Files:**
- Create: `crates/http-extension/schemas/http-node-v1.json`
- Create: `crates/http-extension/i18n/en.json`

- [ ] **Step 1: Write `crates/http-extension/schemas/http-node-v1.json`**

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://greentic.ai/schemas/http-node-v1.json",
  "title": "Greentic HTTP Node",
  "type": "object",
  "required": ["node_id", "component", "config", "inputs"],
  "properties": {
    "node_id": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$" },
    "component": { "type": "string", "pattern": "^oci://" },
    "config": {
      "type": "object",
      "required": ["base_url", "auth_type", "timeout_ms"],
      "properties": {
        "base_url": { "type": "string", "pattern": "^https?://" },
        "auth_type": { "enum": ["none", "bearer", "api_key", "basic"] },
        "auth_token": { "type": ["string", "null"] },
        "api_key_header": { "type": "string" },
        "timeout_ms": { "type": "integer", "minimum": 1, "maximum": 60000 },
        "default_headers": { "type": ["object", "null"] }
      }
    },
    "inputs": {
      "type": "object",
      "properties": {
        "method": { "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"] },
        "path": { "type": "string" },
        "body_template": { "type": "string" }
      }
    },
    "rationale": { "type": "string" },
    "mapping": { "type": "object" },
    "diagnostics": { "type": "array" }
  }
}
```

- [ ] **Step 2: Write `crates/http-extension/i18n/en.json`**

```json
{
  "extension.name": "HTTP Client",
  "extension.summary": "Generate and validate HTTP call nodes for Greentic flows",
  "tool.generate_http_node.title": "Generate HTTP Node",
  "tool.validate_http_config.title": "Validate HTTP Node",
  "tool.curl_to_node.title": "Import from curl",
  "tool.suggest_auth.title": "Suggest Auth",
  "tool.generate_from_card_submit.title": "Generate from Card Submit"
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/http-extension/schemas crates/http-extension/i18n
git commit -m "feat(http-extension): add http-node-v1 JSON schema + en.json stub"
```

---

### Task 21: Write build.sh to produce .gtxpack

**Files:**
- Create: `crates/http-extension/build.sh`

- [ ] **Step 1: Write `crates/http-extension/build.sh`**

```bash
#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

VERSION="$(jq -r .metadata.version describe.json)"

echo "==> Building http-extension v$VERSION (wasm32-wasip2)..."
cargo component build --release --target wasm32-wasip2 --manifest-path Cargo.toml

WASM_SRC=""
for candidate in \
    "../../target/wasm32-wasip2/release/http_extension.wasm" \
    "target/wasm32-wasip2/release/http_extension.wasm"; do
    if [ -f "$candidate" ]; then WASM_SRC="$candidate"; break; fi
done
if [ -z "$WASM_SRC" ]; then echo "ERROR: could not locate built wasm"; exit 1; fi

cp "$WASM_SRC" extension.wasm

PKG="greentic.http-${VERSION}.gtxpack"
rm -f "$PKG"
zip -r "$PKG" \
    extension.wasm \
    describe.json \
    prompts/ \
    schemas/ \
    i18n/

echo "==> Built: $DIR/$PKG"
ls -lh "$PKG"
```

- [ ] **Step 2: Make executable + smoke test**

```bash
chmod +x crates/http-extension/build.sh
bash crates/http-extension/build.sh
```

Expected: prints `Built: crates/http-extension/greentic.http-0.1.0.gtxpack`.

- [ ] **Step 3: Validate the .gtxpack with gtdx**

Run: `gtdx validate --pack crates/http-extension/greentic.http-0.1.0.gtxpack`
Expected: "valid".

- [ ] **Step 4: Add .gitignore entry**

Append to `crates/http-extension/.gitignore`:

```
extension.wasm
*.gtxpack
```

- [ ] **Step 5: Commit**

```bash
git add crates/http-extension/build.sh crates/http-extension/.gitignore
git commit -m "feat(http-extension): add build.sh that produces validated .gtxpack"
```

---

### Task 22: Add publish-extension.yml GitHub Actions workflow

**Files:**
- Create: `.github/workflows/publish-extension.yml`

- [ ] **Step 1: Write workflow**

```yaml
# .github/workflows/publish-extension.yml
name: Publish HTTP Extension

on:
  push:
    tags: ['ext-v*']
  workflow_dispatch:
    inputs:
      version:
        description: 'Extension version to publish (must match describe.json)'
        required: true

jobs:
  publish:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@v4
      - name: Verify publisher prefix
        run: bash ci/check_publisher_prefix.sh greentic greentic.http
        env:
          GREENTIC_STORE_URL: ${{ vars.GREENTIC_STORE_URL }}
          GREENTIC_STORE_TOKEN: ${{ secrets.GREENTIC_STORE_TOKEN }}
      - name: Publish via designer-extension-action
        uses: greenticai/greentic-designer-extension-action@v1
        with:
          extension-dir: crates/http-extension
          version-from-tag: ${{ github.event_name == 'push' }}
        env:
          GREENTIC_STORE_URL: ${{ vars.GREENTIC_STORE_URL }}
          GREENTIC_STORE_TOKEN: ${{ secrets.GREENTIC_STORE_TOKEN }}
```

- [ ] **Step 2: Document required secrets/vars**

Append to `crates/http-extension/README.md` (create if missing) a section:

```markdown
## Publishing

Required repo settings on `greenticai/components-public`:
- Secret `GREENTIC_STORE_TOKEN` — `gts_*` long-lived API token from the `greentic` publisher
- Variable `GREENTIC_STORE_URL` — e.g. `http://62.171.174.152:3030`

To publish a new version:
1. Bump `version` in both `describe.json` and `Cargo.toml`
2. Commit + push to main
3. Tag `git tag ext-v<version> && git push origin ext-v<version>`
4. The `publish-extension` workflow runs, posts the `.gtxpack` to the Store
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/publish-extension.yml crates/http-extension/README.md
git commit -m "ci(http-extension): add tag-triggered Store publish workflow"
```

---

### Task 23: Pre-flight publisher prefix check script

**Files:**
- Create: `ci/check_publisher_prefix.sh`

- [ ] **Step 1: Write the script**

```bash
#!/usr/bin/env bash
# Usage: check_publisher_prefix.sh <publisher-name> <expected-prefix>
# Exit 0 if the publisher's allowed_prefixes covers expected-prefix (exact or wildcard).
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
    # exact or wildcard (e.g. 'greentic.' matches 'greentic.http')
    if [ "$p" = "$PREFIX" ] || [[ "$PREFIX" == $p* ]]; then
        MATCH=1; break
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
```

- [ ] **Step 2: Make executable**

```bash
chmod +x ci/check_publisher_prefix.sh
```

- [ ] **Step 3: Smoke test (dry-run against known publisher)**

If you can reach the Store from the dev machine:
```bash
GREENTIC_STORE_URL=http://62.171.174.152:3030 \
GREENTIC_STORE_TOKEN=<your-token> \
ci/check_publisher_prefix.sh greentic greentic.http
```
Expected: either "OK" or a clear "ERROR" with admin-action instructions.

- [ ] **Step 4: Commit**

```bash
git add ci/check_publisher_prefix.sh
git commit -m "ci: add publisher prefix pre-flight check for extension publish"
```

---

### Task 24: Integration gtests

**Files:**
- Create: `tests/gtests/README/08_http_extension_build.gtest`
- Create: `tests/gtests/README/09_http_extension_gtdx_install.gtest`

- [ ] **Step 1: Write `tests/gtests/README/08_http_extension_build.gtest`**

```
#SET EXT_DIR=${WORK_DIR}/../crates/http-extension
#SET OUT=${WORK_DIR}/ext-out
#RUN mkdir -p ${OUT}
#RUN bash ${EXT_DIR}/build.sh
#EXPECT_EXIT 0
#RUN ls ${EXT_DIR}/greentic.http-*.gtxpack
#EXPECT_EXIT 0
#RUN gtdx validate --pack $(ls ${EXT_DIR}/greentic.http-*.gtxpack | head -1)
#EXPECT_EXIT 0
#SAVE_ARTIFACT ${EXT_DIR}/greentic.http-0.1.0.gtxpack
```

- [ ] **Step 2: Write `tests/gtests/README/09_http_extension_gtdx_install.gtest`**

This test is **optional on CI** — it only runs if a local Store mock is available (the live Store is not reachable from hermetic CI). Guard via env var:

```
#SET EXT_DIR=${WORK_DIR}/../crates/http-extension
#SKIP_IF GREENTIC_STORE_URL=
#RUN bash ${EXT_DIR}/build.sh
#EXPECT_EXIT 0
#RUN gtdx install --pack $(ls ${EXT_DIR}/greentic.http-*.gtxpack | head -1)
#EXPECT_EXIT 0
#RUN gtdx list | grep -q '^greentic.http '
#EXPECT_EXIT 0
#RUN gtdx uninstall greentic.http
#EXPECT_EXIT 0
```

- [ ] **Step 3: Run locally**

```bash
bash tests/run_gtest.sh tests/gtests/README/08_http_extension_build.gtest
```

Expected: passes.

- [ ] **Step 4: Commit**

```bash
git add tests/gtests/README/08_http_extension_build.gtest tests/gtests/README/09_http_extension_gtdx_install.gtest
git commit -m "test: integration gtests for http-extension build + gtdx install"
```

---

### Task 25: Run local_check.sh, open PR #2

- [ ] **Step 1: Run local CI**

Run: `ci/local_check.sh`
Expected: fmt, clippy, tests, gtests all green.

- [ ] **Step 2: Push + open PR**

```bash
git push -u origin feat/http-extension
gh pr create \
  --title "feat: greentic.http DesignExtension" \
  --body "$(cat <<'EOF'
## Summary
- New `http-extension` crate in `components-public/crates/`
- 5 LLM tools: generate_http_node, validate_http_config, curl_to_node, suggest_auth, generate_from_card_submit
- Builds `greentic.http-<version>.gtxpack`
- Tag-triggered publish workflow (`ext-v*`) via `greenticai/greentic-designer-extension-action@v1`
- Pre-flight publisher prefix check in `ci/check_publisher_prefix.sh`

## Test plan
- [x] All tool unit tests pass (`cargo test -p http-extension --target wasm32-wasip2`)
- [x] `build.sh` produces validated `.gtxpack`
- [x] `gtdx validate` passes
- [x] Integration gtest 08 (build+validate) passes
- [ ] Integration gtest 09 (install) — run manually against Store before merge
- [x] Publisher prefix verified for `greentic` → `greentic.http`
- [x] `ci/local_check.sh` green

## Publish checklist (after merge)
1. Verify `greentic` publisher has `allowed_prefixes` covering `greentic.http` (use `ci/check_publisher_prefix.sh`)
2. `git tag ext-v0.1.0 && git push origin ext-v0.1.0`
3. Watch `publish-extension` workflow
4. Verify artifact in Store: `curl $GREENTIC_STORE_URL/api/v1/extensions/greentic.http`

Spec: `docs/superpowers/specs/2026-04-19-greentic-http-extension-design.md`
Plan: `docs/superpowers/plans/2026-04-19-greentic-http-extension.md` (Phase 2, Tasks 10-25)
EOF
)"
```

---

## Post-merge Release Checklist (Tasks 26+)

After PR #2 merges to main:

- [ ] Verify `greentic` publisher `allowed_prefixes` covers `greentic.http` (run `ci/check_publisher_prefix.sh greentic greentic.http`). If missing, request admin addition before tagging.
- [ ] Tag and publish v0.1.0:
  ```bash
  git checkout main && git pull
  git tag ext-v0.1.0
  git push origin ext-v0.1.0
  ```
- [ ] Watch the `publish-extension` workflow; on success, verify:
  ```bash
  curl -s "$GREENTIC_STORE_URL/api/v1/extensions?kind=design&name=greentic.http" | jq
  ```
  Expected: `latestVersion` is `0.1.0`, `kind` is `DesignExtension`.
- [ ] Smoke test the published extension from a fresh Designer session: `gtdx install greentic.http@0.1.0`, then generate a sample HTTP node via LLM and verify it passes `validate_http_config`.
- [ ] Update `greentic-docs` (user-facing site) with a new page under Components referencing the extension and linking to the spec.
