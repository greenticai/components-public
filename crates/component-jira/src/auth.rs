//! Pure auth-mode resolution for the Jira extension.
//!
//! Selects between Jira Basic auth (email + API token) and broker-backed
//! OAuth, and parses the OAuth broker's `get-token` return string. No WIT
//! imports — this module is fully host-testable.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;

/// Which credential source supplies the Jira `Authorization` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Static Basic-auth token from `secret://jira/api_token` (default).
    Token,
    /// Broker-backed OAuth token from `greentic:oauth-broker/broker-v1`.
    OAuth,
}

/// Jira OAuth (3LO) scopes requested from the broker. Cover issue read/write,
/// user lookups, and project administration, plus offline access for refresh.
pub const OAUTH_SCOPES: &[&str] = &[
    "read:jira-work",
    "write:jira-work",
    "read:jira-user",
    "manage:jira-project",
    "offline_access",
];

/// Map the optional `auth_mode` config value to a mode.
///
/// Only an exact `"oauth"` (case-insensitive, surrounding whitespace ignored)
/// selects OAuth. Anything else — `"basic"`, unset, empty, or an unrecognised
/// value — falls back to [`AuthMode::Token`] (safe default, backward
/// compatible).
#[must_use]
pub fn parse_auth_mode(raw: Option<&str>) -> AuthMode {
    match raw {
        Some(value) if value.trim().eq_ignore_ascii_case("oauth") => AuthMode::OAuth,
        _ => AuthMode::Token,
    }
}

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

/// Build the `application/x-www-form-urlencoded` body for the Jira OAuth
/// refresh-token grant (`POST https://auth.atlassian.com/oauth/token`).
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

/// Parse Jira's token-endpoint response from the refresh-token grant.
///
/// Success shape: `{"access_token":"…","expires_in":…,"refresh_token":"…"}`
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

#[derive(Deserialize)]
struct AccessibleResource {
    id: String,
}

/// Parse the response body of `GET
/// https://api.atlassian.com/oauth/token/accessible-resources` (Bearer the
/// OAuth access token) and return the first resource's Atlassian cloud id.
///
/// In OAuth mode Jira Cloud requests target
/// `https://api.atlassian.com/ex/jira/<cloudid>`, not `<site>.atlassian.net`;
/// this id is required to build that base URL. An empty list or malformed
/// JSON is [`AuthError::Failed`] — this response never contains token
/// material, so the raw body is safe to include in the error.
pub fn extract_cloud_id(resources_json: &str) -> Result<String, AuthError> {
    let parsed: Vec<AccessibleResource> = serde_json::from_str(resources_json).map_err(|_| {
        AuthError::Failed("accessible-resources returned malformed response".into())
    })?;
    parsed
        .into_iter()
        .next()
        .map(|resource| resource.id)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            AuthError::Failed("accessible-resources returned no accessible Jira sites".into())
        })
}

/// Standard base64 (RFC 4648) encoder. Hand-rolled to avoid a new dependency.
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[usize::try_from((n >> 18) & 63).unwrap_or(0)] as char);
        out.push(TABLE[usize::try_from((n >> 12) & 63).unwrap_or(0)] as char);
        out.push(if chunk.len() > 1 {
            TABLE[usize::try_from((n >> 6) & 63).unwrap_or(0)] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[usize::try_from(n & 63).unwrap_or(0)] as char
        } else {
            '='
        });
    }
    out
}

/// Build the HTTP Basic-auth `Authorization` header value for Jira's REST
/// API: base64 of `email:api_token`. The result is request material only —
/// never log it.
#[must_use]
pub fn basic_auth_header(email: &str, api_token: &str) -> String {
    format!(
        "Basic {}",
        base64_encode(format!("{email}:{api_token}").as_bytes())
    )
}

/// Normalize a configured Jira `site` value into the REST API base URL.
///
/// Accepts a bare site name (`"acme"`), a full host (`"acme.atlassian.net"`),
/// or a full URL with scheme (`"https://acme.atlassian.net"`); trims a
/// trailing slash. Always returns `https://<host>` — Jira Cloud does not
/// serve its REST API over plain HTTP.
#[must_use]
pub fn base_url(site: &str) -> String {
    let trimmed = site
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = if trimmed.contains('.') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.atlassian.net")
    };
    format!("https://{}", host.trim_end_matches('/'))
}

/// Base URL for the Jira Agile REST API (`/rest/agile/1.0/...` endpoints —
/// boards, sprints). Jira Cloud serves Agile from the same host as the
/// platform REST API, so this currently delegates to [`base_url`]; kept as a
/// distinct function so callers don't hardcode that assumption and a future
/// host split doesn't require call-site changes.
#[must_use]
pub fn agile_base_url(site: &str) -> String {
    base_url(site)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_selects_oauth_else_token() {
        assert_eq!(parse_auth_mode(Some("oauth")), AuthMode::OAuth);
        assert_eq!(parse_auth_mode(Some("  OAuth ")), AuthMode::OAuth);
        assert_eq!(parse_auth_mode(Some("basic")), AuthMode::Token);
        assert_eq!(parse_auth_mode(None), AuthMode::Token);
    }

    #[test]
    fn basic_header_base64_encodes_pair() {
        // "user@x.com:tok" => base64
        assert_eq!(
            basic_auth_header("user@x.com", "tok"),
            "Basic dXNlckB4LmNvbTp0b2s="
        );
    }

    #[test]
    fn base_url_accepts_bare_site_and_full_host() {
        assert_eq!(base_url("acme"), "https://acme.atlassian.net");
        assert_eq!(base_url("acme.atlassian.net"), "https://acme.atlassian.net");
        assert_eq!(
            base_url("https://acme.atlassian.net"),
            "https://acme.atlassian.net"
        );
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
        assert_eq!(
            extract_oauth_token(r#"{"access_token":"BT"}"#),
            Ok("BT".into())
        );
        assert!(matches!(
            extract_oauth_token(r#"{"error":"permission_denied"}"#),
            Err(AuthError::Denied(_))
        ));
    }

    #[test]
    fn extract_cloud_id_takes_first_resource() {
        assert_eq!(
            extract_cloud_id(r#"[{"id":"cid-1","url":"https://acme.atlassian.net"}]"#),
            Ok("cid-1".into())
        );
        assert!(matches!(extract_cloud_id("[]"), Err(AuthError::Failed(_))));
        assert!(matches!(
            extract_cloud_id("not json"),
            Err(AuthError::Failed(_))
        ));
    }
}
