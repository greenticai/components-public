//! Pure, host-testable ClickUp tool domains. Each submodule maps a group of
//! `clickup_*` tool operations to `client::HttpCall` requests and
//! normalizes the corresponding ClickUp REST responses. No WIT imports live
//! here — the WIT `invoke_tool` dispatch in `lib.rs` calls into these
//! modules.
//!
//! Batch 1 (task B1b) added `tasks`, `spaces`, `folders`, `lists`, mirroring
//! `component-jira-ext`'s `src/tools/*.rs` layout. Batch 2 (task B1c) adds
//! `comments`, `time_entries`, `custom_fields`, `members`, completing the
//! eight ClickUp tools.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
pub mod comments;
pub mod custom_fields;
pub mod folders;
pub mod lists;
pub mod members;
pub mod spaces;
pub mod tasks;
pub mod time_entries;

/// Fetch a required string field, rejecting `None` and the empty string.
/// Shared by every `tools::*` domain's `build_call`.
pub(crate) fn require_field<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(format!("missing required field: {name}")),
    }
}
