//! Pure auth-mode resolution for the Calendly extension.
//!
//! Selects between a static Bearer Personal Access Token (PAT) and
//! broker-backed OAuth, and parses the OAuth broker's `get-token` return
//! string. No WIT imports — this module is fully host-testable.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;

/// Which credential source supplies the Calendly `Authorization` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Static Bearer Personal Access Token from `secret://calendly/token`
    /// (default).
    Token,
    /// Broker-backed OAuth token from `greentic:oauth-broker/broker-v1`.
    OAuth,
}

/// OAuth scopes requested from the broker. Calendly OAuth apps have their
/// scopes configured on the app itself (via the Calendly developer
/// portal), not requested per-token — the broker/token endpoint grants
/// whatever the registered app is authorized for. A single placeholder
/// scope is passed through so the broker call always carries a non-empty
/// scope list; it has no effect on what the app is actually authorized for.
pub const OAUTH_SCOPES: &[&str] = &["default"];

/// OAuth provider id requested from the broker / used to key brokerless
/// OAuth secrets. Must match the `oauthProviders` allowlist in
/// `describe.json` (added in a later task).
pub const OAUTH_PROVIDER: &str = "calendly";

/// Calendly REST API base URL.
pub const BASE_URL: &str = "https://api.calendly.com";

/// Calendly OAuth token endpoint (authorization-code / refresh-token
/// grants).
pub const TOKEN_URL: &str = "https://auth.calendly.com/oauth/token";

/// Map the optional `auth_mode` config value to a mode.
///
/// Only an exact `"oauth"` (case-insensitive, surrounding whitespace
/// ignored) selects OAuth. Anything else — `"token"`, unset, empty, or an
/// unrecognised value — falls back to [`AuthMode::Token`] (safe default,
/// backward compatible).
#[must_use]
pub fn parse_auth_mode(raw: Option<&str>) -> AuthMode {
    match raw {
        Some(value) if value.trim().eq_ignore_ascii_case("oauth") => AuthMode::OAuth,
        _ => AuthMode::Token,
    }
}

/// Classified failure from the OAuth path, so the caller can map it to the
/// right `ExtensionError` variant without leaking any token material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// Broker refused the request (permission gate or unconfigured
    /// broker). Maps to a permission-denied surface.
    Denied(String),
    /// Broker request failed, response could not be encoded, or the return
    /// string was not the expected shape. Maps to an internal error.
    Failed(String),
}

/// Build the Calendly `Authorization` header value for token mode (a
/// static Bearer Personal Access Token from `secret://calendly/token`).
/// The result is request material only — never log it.
#[must_use]
pub fn bearer_header(token: &str) -> String {
    format!("Bearer {}", token.trim())
}

/// Build the `application/x-www-form-urlencoded` body for the Calendly
/// OAuth refresh-token grant (`POST
/// https://auth.calendly.com/oauth/token`).
///
/// Returns `grant_type=refresh_token&client_id=…&client_secret=…&refresh_token=…`
/// with each value percent-encoded. The result is request material only —
/// never log it: it contains the client secret and refresh token.
#[must_use]
pub fn build_refresh_form(client_id: &str, client_secret: &str, refresh_token: &str) -> String {
    format!(
        "grant_type=refresh_token&client_id={}&client_secret={}&refresh_token={}",
        crate::client::percent_encode(client_id),
        crate::client::percent_encode(client_secret),
        crate::client::percent_encode(refresh_token)
    )
}

#[derive(Deserialize)]
struct RefreshToken {
    #[serde(default)]
    access_token: String,
}

/// Parse Calendly's token-endpoint response from the refresh-token grant.
///
/// Success shape: `{"access_token":"…","token_type":"Bearer","expires_in":…,"refresh_token":"…"}`
/// → `Ok(access_token)` when the token is non-empty. An empty token or
/// malformed JSON → [`AuthError::Failed`]. The returned token is only ever
/// placed in the `Authorization` header and is never logged.
pub fn extract_refreshed_token(token_json: &str) -> Result<String, AuthError> {
    let parsed: RefreshToken = serde_json::from_str(token_json)
        .map_err(|_| AuthError::Failed("token endpoint returned malformed response".into()))?;

    if parsed.access_token.is_empty() {
        return Err(AuthError::Failed(
            "token endpoint returned an empty access token".into(),
        ));
    }
    Ok(parsed.access_token)
}

#[derive(Deserialize)]
struct BrokerToken {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    error: Option<String>,
}

/// Parse the broker `get-token` return string.
///
/// Success shape (from the broker host): `{"access_token":"…","expires_at":<unix>}`
/// → `Ok(token)` when the token is non-empty. Error shapes:
/// `{"error":"permission_denied"|"oauth_broker_unconfigured"|"broker_request_failed"|"encode_failed"}`.
/// `permission_denied` / `oauth_broker_unconfigured` → [`AuthError::Denied`];
/// every other code, malformed JSON, or an empty token → [`AuthError::Failed`].
pub fn extract_oauth_token(broker_json: &str) -> Result<String, AuthError> {
    let parsed: BrokerToken = serde_json::from_str(broker_json)
        .map_err(|_| AuthError::Failed("broker returned malformed response".into()))?;

    if let Some(code) = parsed.error.as_deref() {
        return Err(match code {
            "permission_denied" | "oauth_broker_unconfigured" => {
                AuthError::Denied(code.to_string())
            }
            other => AuthError::Failed(other.to_string()),
        });
    }

    if parsed.access_token.is_empty() {
        return Err(AuthError::Failed(
            "broker returned an empty access token".into(),
        ));
    }
    Ok(parsed.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_selects_oauth_else_token() {
        assert_eq!(parse_auth_mode(Some("oauth")), AuthMode::OAuth);
        assert_eq!(parse_auth_mode(Some("  OAuth ")), AuthMode::OAuth);
        assert_eq!(parse_auth_mode(Some("token")), AuthMode::Token);
        assert_eq!(parse_auth_mode(None), AuthMode::Token);
    }

    #[test]
    fn bearer_header_wraps_and_trims_token() {
        assert_eq!(bearer_header("tok"), "Bearer tok");
        assert_eq!(bearer_header("  tok  "), "Bearer tok");
    }

    #[test]
    fn refresh_form_encodes_values() {
        let f = build_refresh_form("cid 1", "sec/ret", "rt+oken");
        assert!(f.contains("grant_type=refresh_token"));
        assert!(f.contains("client_id=cid%201"));
        assert!(f.contains("client_secret=sec%2Fret"));
        assert!(f.contains("refresh_token=rt%2Boken"));
    }

    #[test]
    fn extract_tokens_success_and_failure() {
        assert_eq!(
            extract_refreshed_token(r#"{"access_token":"AT"}"#),
            Ok("AT".into())
        );
        assert!(matches!(
            extract_refreshed_token(r#"{"access_token":""}"#),
            Err(AuthError::Failed(_))
        ));
        assert!(matches!(
            extract_refreshed_token("not json"),
            Err(AuthError::Failed(_))
        ));

        assert_eq!(
            extract_oauth_token(r#"{"access_token":"BT"}"#),
            Ok("BT".into())
        );
        assert!(matches!(
            extract_oauth_token(r#"{"error":"permission_denied"}"#),
            Err(AuthError::Denied(_))
        ));
        assert!(matches!(
            extract_oauth_token(r#"{"error":"oauth_broker_unconfigured"}"#),
            Err(AuthError::Denied(_))
        ));
        assert!(matches!(
            extract_oauth_token(r#"{"error":"broker_request_failed"}"#),
            Err(AuthError::Failed(_))
        ));
        assert!(matches!(
            extract_oauth_token(r#"{"access_token":""}"#),
            Err(AuthError::Failed(_))
        ));
        assert!(matches!(
            extract_oauth_token("not json"),
            Err(AuthError::Failed(_))
        ));
    }
}
