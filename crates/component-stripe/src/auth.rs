//! Pure auth resolution for the Stripe extension.
//!
//! Stripe's REST API uses a single credential shape: HTTP Bearer auth with
//! the account's secret key (`sk_live_…` / `sk_test_…`). Unlike Jira, there
//! is no OAuth mode, no base64 Basic-auth header, and no auth-mode selector.
//! No WIT imports — this module is fully host-testable.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
/// Stripe REST API base URL. Stripe does not support per-tenant hosts, so
/// this is a fixed constant (unlike Jira's per-site base URL).
pub const BASE_URL: &str = "https://api.stripe.com/v1";

/// Build the HTTP Bearer `Authorization` header value for Stripe's REST API:
/// `Bearer <secret_key>`. The secret key is trimmed of surrounding
/// whitespace before use. The result is request material only — never log
/// it or include it in an error message.
#[must_use]
pub fn bearer_header(secret_key: &str) -> String {
    format!("Bearer {}", secret_key.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_header_wraps_trimmed_key() {
        assert_eq!(bearer_header("sk_test_123"), "Bearer sk_test_123");
    }

    #[test]
    fn bearer_header_trims_surrounding_whitespace() {
        assert_eq!(bearer_header("  sk_live_abc\n"), "Bearer sk_live_abc");
    }

    #[test]
    fn bearer_header_does_not_alter_internal_content() {
        // Sanity check: only the ends are trimmed, not internal characters.
        assert_eq!(bearer_header("sk_test_a b"), "Bearer sk_test_a b");
    }

    #[test]
    fn base_url_is_fixed() {
        assert_eq!(BASE_URL, "https://api.stripe.com/v1");
    }
}
