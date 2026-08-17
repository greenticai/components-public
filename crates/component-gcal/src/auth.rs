//! Pure OAuth token resolution for the Google Calendar extension.
//!
//! Google Calendar has no static API key for user calendars, so this
//! extension is OAuth-only: either a brokerless refresh against Google's
//! token endpoint (when a refresh token is configured) or a broker-backed
//! OAuth token from `greentic:oauth-broker/broker-v1`. This module parses
//! both responses. No WIT imports — fully host-testable.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
use serde::Deserialize;

/// OAuth provider id requested from the broker. Must match the
/// `oauthProviders` allowlist in describe.json.
pub const OAUTH_PROVIDER: &str = "google";

/// Google Calendar OAuth scopes requested from the broker / brokerless
/// refresh. Full read/write access to the user's calendars.
pub const OAUTH_SCOPES: &[&str] = &["https://www.googleapis.com/auth/calendar"];

/// Classified failure from the OAuth path, so the caller can map it to the right
/// `ExtensionError` variant without leaking any token material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// Broker refused the request (permission gate or unconfigured broker).
    /// Maps to a permission-denied surface.
    Denied(String),
    /// Broker request failed, response could not be encoded, or the return
    /// string was not the expected shape. Maps to an internal error.
    Failed(String),
}

/// Build the `application/x-www-form-urlencoded` body for the Google OAuth
/// refresh-token grant (`POST https://oauth2.googleapis.com/token`).
///
/// Returns `grant_type=refresh_token&client_id=…&client_secret=…&refresh_token=…`
/// with each value percent-encoded. The result is request material only — never
/// log it: it contains the client secret and refresh token.
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

/// Parse Google's token-endpoint response from the refresh-token grant.
///
/// Success shape: `{"access_token":"…","expires_in":…,"scope":"…","token_type":"Bearer"}`
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
    fn refresh_form_encodes_values() {
        let f = build_refresh_form("cid 1", "sec/ret", "rt+oken");
        assert!(f.contains("grant_type=refresh_token"));
        assert!(f.contains("client_id=cid%201"));
        assert!(f.contains("client_secret=sec%2Fret"));
        assert!(f.contains("refresh_token=rt%2Boken"));
    }

    #[test]
    fn extract_refreshed_token_success() {
        assert_eq!(
            extract_refreshed_token(r#"{"access_token":"AT"}"#),
            Ok("AT".into())
        );
    }

    #[test]
    fn extract_refreshed_token_empty_is_failed() {
        assert!(matches!(
            extract_refreshed_token(r#"{"access_token":""}"#),
            Err(AuthError::Failed(_))
        ));
    }

    #[test]
    fn extract_refreshed_token_malformed_is_failed() {
        assert!(matches!(
            extract_refreshed_token("not json"),
            Err(AuthError::Failed(_))
        ));
    }

    #[test]
    fn extract_oauth_token_success() {
        assert_eq!(
            extract_oauth_token(r#"{"access_token":"BT"}"#),
            Ok("BT".into())
        );
    }

    #[test]
    fn extract_oauth_token_permission_denied_is_denied() {
        assert!(matches!(
            extract_oauth_token(r#"{"error":"permission_denied"}"#),
            Err(AuthError::Denied(_))
        ));
    }
}
