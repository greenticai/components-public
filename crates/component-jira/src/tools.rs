//! Pure, host-testable Jira tool domains. Each submodule maps a group of
//! `jira_*` tool operations to `client::HttpCall` requests and normalizes
//! the corresponding Jira REST responses. No WIT imports live here — the
//! WIT `invoke_tool` dispatch in `lib.rs` calls into these modules.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
pub mod attachments;
pub mod boards;
pub mod comments;
pub mod issues;
pub mod projects;
pub mod sprints;
pub mod users;
pub mod worklogs;

/// Fetch a required string field, rejecting `None` and the empty string.
/// Shared by every `tools::*` domain's `build_call`.
pub(crate) fn require_field<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(format!("missing required field: {name}")),
    }
}
