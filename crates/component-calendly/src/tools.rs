//! Pure, host-testable Calendly tool domains. Each submodule maps a group
//! of `calendly_*` tool operations to `client::HttpCall` requests and
//! normalizes the corresponding Calendly REST responses. No WIT imports
//! live here — the WIT `invoke_tool` dispatch in `lib.rs` calls into these
//! modules.
//!
//! This module also holds the helpers shared by every `tools::*` domain's
//! `build_call`: [`require_field`] and [`require_owner_query`].

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
pub mod availability;
pub mod event_types;
pub mod events;
pub mod invitees;
pub mod me;
pub mod scheduling_links;
pub mod webhooks;

/// Fetch a required string field, rejecting `None` and the empty string.
/// Shared by every `tools::*` domain's `build_call`.
pub(crate) fn require_field<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(format!("missing required field: {name}")),
    }
}

/// Validate that exactly one of `user`/`organization` is provided, and
/// return the corresponding query pair (`("user", <uri>)` or
/// `("organization", <uri>)`).
///
/// Most Calendly list endpoints (`calendly_event_types` list,
/// `calendly_events` list) require the caller to scope the query by a user
/// URI or an organization URI — obtained beforehand via `calendly_me` — but
/// never both and never neither.
pub(crate) fn require_owner_query(
    user: Option<&str>,
    organization: Option<&str>,
) -> Result<(String, String), String> {
    let user = user.filter(|value| !value.is_empty());
    let organization = organization.filter(|value| !value.is_empty());
    match (user, organization) {
        (Some(user), None) => Ok(("user".to_string(), user.to_string())),
        (None, Some(organization)) => Ok(("organization".to_string(), organization.to_string())),
        (None, None) => Err("missing required field: user or organization".to_string()),
        (Some(_), Some(_)) => {
            Err("provide exactly one of user or organization, not both".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_field_rejects_missing_and_empty() {
        assert!(require_field(None, "id").is_err());
        assert!(require_field(Some(""), "id").is_err());
        assert_eq!(require_field(Some("AB-1"), "id"), Ok("AB-1"));
    }

    #[test]
    fn require_owner_query_accepts_exactly_one() {
        assert_eq!(
            require_owner_query(Some("https://api.calendly.com/users/u1"), None),
            Ok((
                "user".to_string(),
                "https://api.calendly.com/users/u1".to_string()
            ))
        );
        assert_eq!(
            require_owner_query(None, Some("https://api.calendly.com/organizations/o1")),
            Ok((
                "organization".to_string(),
                "https://api.calendly.com/organizations/o1".to_string()
            ))
        );
    }

    #[test]
    fn require_owner_query_rejects_neither() {
        let err = require_owner_query(None, None).unwrap_err();
        assert!(err.contains("user"));
        assert!(err.contains("organization"));
    }

    #[test]
    fn require_owner_query_rejects_both() {
        let err = require_owner_query(
            Some("https://api.calendly.com/users/u1"),
            Some("https://api.calendly.com/organizations/o1"),
        )
        .unwrap_err();
        assert!(err.contains("exactly one"));
    }

    #[test]
    fn require_owner_query_treats_empty_string_as_absent() {
        let err = require_owner_query(Some(""), Some("")).unwrap_err();
        assert!(err.contains("user"));
    }
}
