//! Pure, host-testable Trello tool domains. Each submodule maps a group of
//! `trello_*` tool operations to `client::HttpCall` requests and normalizes
//! the corresponding Trello REST responses. No WIT imports live here — the
//! WIT `invoke_tool` dispatch in `lib.rs` calls into these modules.
//!
//! Batch 1 domains: cards, lists, boards, checklists. Batch 2 domains:
//! labels, comments, attachments, members.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
pub mod attachments;
pub mod boards;
pub mod cards;
pub mod checklists;
pub mod comments;
pub mod labels;
pub mod lists;
pub mod members;

/// Fetch a required string field, rejecting `None` and the empty string.
/// Shared by every `tools::*` domain's `build_call`.
pub(crate) fn require_field<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(format!("missing required field: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_field_rejects_missing_and_empty() {
        assert_eq!(require_field(Some("x"), "name"), Ok("x"));
        assert!(require_field(None, "name").is_err());
        assert!(require_field(Some(""), "name").is_err());
    }
}
