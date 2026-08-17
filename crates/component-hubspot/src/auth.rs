//! Pure auth-mode resolution for the HubSpot extension.
//!
//! Selects between the static Private App token and broker-backed OAuth, and
//! parses the OAuth broker's `get-token` return string. No WIT imports — this
//! module is fully host-testable.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
use serde::Deserialize;

/// Which credential source supplies the HubSpot `Authorization` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Static Private App token from `secret://hubspot/access_token` (default).
    PrivateApp,
    /// Broker-backed OAuth token from `greentic:oauth-broker/broker-v1`.
    OAuth,
}

/// HubSpot CRM OAuth scopes requested from the broker. Cover the nine object
/// families the tools touch (contacts, deals, companies, tickets, notes,
/// tasks, calls, meetings, emails), read+write.
pub const OAUTH_SCOPES: &[&str] = &[
    "crm.objects.contacts.read",
    "crm.objects.contacts.write",
    "crm.objects.deals.read",
    "crm.objects.deals.write",
    "crm.objects.companies.read",
    "crm.objects.companies.write",
    "tickets",
    "crm.objects.notes.read",
    "crm.objects.notes.write",
    "crm.objects.tasks.read",
    "crm.objects.tasks.write",
    "crm.objects.calls.read",
    "crm.objects.calls.write",
    "crm.objects.meetings.read",
    "crm.objects.meetings.write",
    "crm.objects.emails.read",
    "crm.objects.emails.write",
];

/// Map the optional `auth_mode` config value to a mode.
///
/// Only an exact `"oauth"` (case-insensitive, surrounding whitespace ignored)
/// selects OAuth. Anything else — `"private_app"`, unset, empty, or an
/// unrecognised value — falls back to [`AuthMode::PrivateApp`] (safe default,
/// backward compatible).
#[must_use]
pub fn parse_auth_mode(raw: Option<&str>) -> AuthMode {
    match raw {
        Some(value) if value.trim().eq_ignore_ascii_case("oauth") => AuthMode::OAuth,
        _ => AuthMode::PrivateApp,
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

/// Percent-encode a single value for an `application/x-www-form-urlencoded`
/// body. Encodes every byte that is not in the unreserved set
/// (`A-Z a-z 0-9 - _ . ~`) as `%XX`. Used to encode OAuth client credentials
/// and the refresh token, which may contain reserved characters.
fn form_encode(value: &str) -> String {
    const UPPER_HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push('%');
                encoded.push(UPPER_HEX[(byte >> 4) as usize] as char);
                encoded.push(UPPER_HEX[(byte & 0x0f) as usize] as char);
            }
        }
    }
    encoded
}

/// Build the `application/x-www-form-urlencoded` body for the HubSpot OAuth
/// refresh-token grant (`POST https://api.hubapi.com/oauth/v1/token`).
///
/// Returns `grant_type=refresh_token&client_id=…&client_secret=…&refresh_token=…`
/// with each value percent-encoded. The result is request material only — never
/// log it: it contains the client secret and refresh token.
#[must_use]
pub fn build_refresh_form(client_id: &str, client_secret: &str, refresh_token: &str) -> String {
    format!(
        "grant_type=refresh_token&client_id={}&client_secret={}&refresh_token={}",
        form_encode(client_id),
        form_encode(client_secret),
        form_encode(refresh_token)
    )
}

#[derive(Deserialize)]
struct RefreshToken {
    #[serde(default)]
    access_token: String,
}

/// Parse HubSpot's token-endpoint response from the refresh-token grant.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_value_selects_oauth() {
        assert_eq!(parse_auth_mode(Some("oauth")), AuthMode::OAuth);
        assert_eq!(parse_auth_mode(Some("OAuth")), AuthMode::OAuth);
        assert_eq!(parse_auth_mode(Some("  OAUTH  ")), AuthMode::OAuth);
    }

    #[test]
    fn private_app_and_unknown_and_unset_select_private_app() {
        assert_eq!(parse_auth_mode(Some("private_app")), AuthMode::PrivateApp);
        assert_eq!(parse_auth_mode(Some("token")), AuthMode::PrivateApp);
        assert_eq!(parse_auth_mode(Some("")), AuthMode::PrivateApp);
        assert_eq!(parse_auth_mode(Some("   ")), AuthMode::PrivateApp);
        assert_eq!(parse_auth_mode(None), AuthMode::PrivateApp);
    }

    #[test]
    fn extracts_access_token_on_success() {
        let raw = r#"{"access_token":"AT-123","expires_at":999}"#;
        assert_eq!(extract_oauth_token(raw), Ok("AT-123".to_string()));
    }

    #[test]
    fn permission_denied_maps_to_denied() {
        let raw = r#"{"error":"permission_denied","provider":"hubspot"}"#;
        assert_eq!(
            extract_oauth_token(raw),
            Err(AuthError::Denied("permission_denied".to_string()))
        );
    }

    #[test]
    fn unconfigured_maps_to_denied() {
        let raw = r#"{"error":"oauth_broker_unconfigured"}"#;
        assert_eq!(
            extract_oauth_token(raw),
            Err(AuthError::Denied("oauth_broker_unconfigured".to_string()))
        );
    }

    #[test]
    fn broker_request_failed_maps_to_failed() {
        let raw = r#"{"error":"broker_request_failed"}"#;
        assert_eq!(
            extract_oauth_token(raw),
            Err(AuthError::Failed("broker_request_failed".to_string()))
        );
    }

    #[test]
    fn empty_token_is_failed() {
        let raw = r#"{"access_token":"","expires_at":1}"#;
        assert!(matches!(
            extract_oauth_token(raw),
            Err(AuthError::Failed(_))
        ));
    }

    #[test]
    fn malformed_json_is_failed() {
        assert!(matches!(
            extract_oauth_token("{not json"),
            Err(AuthError::Failed(_))
        ));
    }

    #[test]
    fn build_refresh_form_has_grant_and_encoded_values() {
        let form = build_refresh_form("cid 1", "sec/ret", "rt+oken");
        assert!(form.contains("grant_type=refresh_token"));
        assert!(form.contains("client_id=cid%201"));
        assert!(form.contains("client_secret=sec%2Fret"));
        assert!(form.contains("refresh_token=rt%2Boken"));
    }

    #[test]
    fn build_refresh_form_leaves_unreserved_unencoded() {
        let form = build_refresh_form("abc-123_X.Y~Z", "plain", "token");
        assert!(form.contains("client_id=abc-123_X.Y~Z"));
    }

    #[test]
    fn extract_refreshed_token_returns_access_token() {
        let raw = r#"{"access_token":"AT-refreshed","expires_in":1800,"refresh_token":"RT-new"}"#;
        assert_eq!(extract_refreshed_token(raw), Ok("AT-refreshed".to_string()));
    }

    #[test]
    fn extract_refreshed_token_empty_is_failed() {
        let raw = r#"{"access_token":"","expires_in":1800}"#;
        assert!(matches!(
            extract_refreshed_token(raw),
            Err(AuthError::Failed(_))
        ));
    }

    #[test]
    fn extract_refreshed_token_malformed_is_failed() {
        assert!(matches!(
            extract_refreshed_token("{not json"),
            Err(AuthError::Failed(_))
        ));
    }
}
