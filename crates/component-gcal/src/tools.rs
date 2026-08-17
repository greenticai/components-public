//! Pure, host-testable Google Calendar tool domains. Each submodule maps a
//! group of `gcal_*` tool operations to `client::HttpCall` requests and
//! normalizes the corresponding Google Calendar REST responses. No WIT
//! imports live here — the WIT `invoke_tool` dispatch in `lib.rs` calls into
//! these modules.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
pub mod calendars;
pub mod events;
pub mod freebusy;

/// Fetch a required string field, rejecting `None` and the empty string.
/// Shared by every `tools::*` domain's `build_call`.
pub(crate) fn require_field<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(format!("missing required field: {name}")),
    }
}
