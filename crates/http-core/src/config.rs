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

fn default_auth_type() -> String {
    DEFAULT_AUTH_TYPE.to_string()
}
fn default_api_key_header() -> String {
    DEFAULT_API_KEY_HEADER.to_string()
}
fn default_timeout() -> u32 {
    DEFAULT_TIMEOUT_MS
}

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
    #[error("timeout_ms must be between 1 and {max}, got {value}", max = MAX_TIMEOUT_MS)]
    InvalidTimeout { value: u32 },
    #[error("default_headers must be an object, got {0}")]
    InvalidHeaders(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn apply_answers(
    mut cfg: ComponentConfig,
    answers: &Value,
) -> Result<ComponentConfig, ConfigError> {
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
        let t = u32::try_from(v).map_err(|_| ConfigError::InvalidTimeout { value: u32::MAX })?;
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
        Err(ConfigError::InvalidTimeout { value: ms })
    } else {
        Ok(())
    }
}

fn validate_headers(v: &Value) -> Result<(), ConfigError> {
    if v.is_object() {
        Ok(())
    } else {
        Err(ConfigError::InvalidHeaders(v.to_string()))
    }
}
