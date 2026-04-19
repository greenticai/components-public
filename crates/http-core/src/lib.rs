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
