//! Pure auth resolution for the Trello extension.
//!
//! Trello has a single auth mode: every request authenticates via `key`
//! and `token` **query parameters** — there is no `Authorization` header
//! and no OAuth broker involved. No WIT imports — this module is fully
//! host-testable.
//!
//! # Security
//!
//! Because the token travels in the URL query string, the fully-built
//! request URL (and these auth pairs) are secret material. Callers MUST
//! NEVER place the full URL, the query string, or these pairs into a log
//! line or an `ExtensionError`. See the safety note at the `trello_send`
//! call site in `lib.rs`, which builds the URL for the fetch call but maps
//! errors using only the response status and body.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
/// Trello REST API base URL (v1).
pub const BASE_URL: &str = "https://api.trello.com/1";

/// Build the `key`/`token` query pairs Trello requires on every request:
/// `?key=<api_key>&token=<token>`.
///
/// The returned pairs are request material only — never log them or place
/// them in an error string.
#[must_use]
pub fn auth_query(api_key: &str, token: &str) -> Vec<(String, String)> {
    vec![
        ("key".to_string(), api_key.to_string()),
        ("token".to_string(), token.to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_query_returns_key_and_token_pairs_in_order() {
        assert_eq!(
            auth_query("k", "t"),
            vec![
                ("key".to_string(), "k".to_string()),
                ("token".to_string(), "t".to_string()),
            ]
        );
    }

    #[test]
    fn auth_query_carries_the_values_verbatim() {
        let pairs = auth_query("my-api-key", "my-secret-token");
        assert_eq!(pairs[0], ("key".to_string(), "my-api-key".to_string()));
        assert_eq!(
            pairs[1],
            ("token".to_string(), "my-secret-token".to_string())
        );
    }
}
