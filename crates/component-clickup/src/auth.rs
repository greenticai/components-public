//! Pure auth-mode resolution for the ClickUp extension.
//!
//! Selects between a static ClickUp personal API token (default) and
//! broker-backed OAuth, and parses the OAuth broker's `get-token` return
//! string. No WIT imports — this module is fully host-testable.
//!
//! ClickUp auth is simpler than Jira's: there is no per-tenant host (every
//! tenant calls the same `api.clickup.com` origin), no HTTP Basic auth (the
//! personal token goes in `Authorization` unmodified, no `Bearer` prefix),
//! and — because ClickUp OAuth access tokens never expire and are issued
//! without a refresh token — there is no brokerless refresh path: OAuth mode
//! always either asks the broker or falls back to a previously stored
//! access token.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;

/// ClickUp REST API base URL. Unlike Jira, ClickUp has no per-tenant host —
/// every tenant calls this same origin.
pub const BASE_URL: &str = "https://api.clickup.com/api/v2";

/// Which credential source supplies the ClickUp `Authorization` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Static personal API token from `secret://clickup/token` (default).
    Token,
    /// Broker-backed (or stored) OAuth access token.
    OAuth,
}

/// ClickUp OAuth scopes requested from the broker. ClickUp does not support
/// per-request scoping — the scopes an OAuth app can grant are fixed when
/// the app is registered in the ClickUp developer console, not passed on the
/// authorize/token calls — so this is intentionally empty. Kept as a named
/// constant (rather than inlining `&[]` at the call site) so the broker call
/// site reads the same as Jira's and a future scoped API doesn't require a
/// signature change.
pub const OAUTH_SCOPES: &[&str] = &[];

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
    /// Broker refused the request (permission gate or unconfigured broker).
    /// Maps to a permission-denied surface.
    Denied(String),
    /// Broker request failed, response could not be encoded, or the return
    /// string was not the expected shape. Maps to an internal error.
    Failed(String),
}

/// Build the `Authorization` header value for ClickUp token mode: the raw
/// personal API token (`pk_...`), unmodified — ClickUp does not use the
/// `Bearer` scheme for personal tokens. The result is request material —
/// never log it.
#[must_use]
pub fn token_auth_header(token: &str) -> String {
    token.trim().to_string()
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
    fn token_auth_header_is_raw_token_no_bearer_prefix() {
        assert_eq!(token_auth_header("pk_123"), "pk_123");
    }

    #[test]
    fn extract_oauth_token_success_and_denied() {
        assert_eq!(
            extract_oauth_token(r#"{"access_token":"AT"}"#),
            Ok("AT".into())
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
