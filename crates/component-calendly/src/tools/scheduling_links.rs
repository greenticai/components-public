//! `calendly_scheduling_links` tool domain — pure HTTP-call building and
//! response normalization for Calendly single-use scheduling link creation.
//! No WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext::tools::issues` template: `SchedulingLinkOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary. Like
//! `calendly_me`, this domain has a single operation (`create`) — the enum
//! shape is kept for consistency with the other tools' dispatch and
//! `parse_operation` wiring in `lib.rs`.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Calendly scheduling link operation selected by the `operation` input
/// field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulingLinkOp {
    Create,
}

/// Raw `calendly_scheduling_links` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct SchedulingLinksInput {
    operation: SchedulingLinkOp,
    #[serde(default)]
    event_type_uri: Option<String>,
}

/// Build the Calendly REST v2 [`HttpCall`] for a `calendly_scheduling_links`
/// invocation.
///
/// Parses `args_json` into a [`SchedulingLinksInput`], validates the fields
/// required by the selected [`SchedulingLinkOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: SchedulingLinksInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        SchedulingLinkOp::Create => build_create(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<SchedulingLinkOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: SchedulingLinkOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &SchedulingLinksInput) -> Result<HttpCall, String> {
    let owner = super::require_field(input.event_type_uri.as_deref(), "event_type_uri")?;
    Ok(HttpCall {
        method: Method::Post,
        path: "/scheduling_links".to_string(),
        query: Vec::new(),
        body: Some(json!({
            "max_event_count": 1,
            "owner": owner,
            "owner_type": "EventType"
        })),
    })
}

/// Map a raw Calendly Scheduling Links response body to the compact shape
/// returned to the model, based on the [`SchedulingLinkOp`] that produced
/// it.
pub fn normalize(op: SchedulingLinkOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        SchedulingLinkOp::Create => normalize_create(raw),
    }
}

/// Normalize a `POST /scheduling_links` response, unwrapping Calendly's
/// `{"resource": {...}}` envelope, to `{booking_url,owner,owner_type}`
/// without panicking on missing/absent fields.
fn normalize_create(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid scheduling link response: {err}"))?;
    let resource = value.get("resource").unwrap_or(&value);

    let mut out = Map::new();
    out.insert(
        "booking_url".to_string(),
        resource.get("booking_url").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "owner".to_string(),
        resource.get("owner").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "owner_type".to_string(),
        resource.get("owner_type").cloned().unwrap_or(Value::Null),
    );
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_requires_event_type_uri() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("event_type_uri"));
    }

    #[test]
    fn create_builds_post_with_owner_body() {
        let call = build_call(
            r#"{"operation":"create","event_type_uri":"https://api.calendly.com/event_types/AAAA"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/scheduling_links");
        assert!(call.query.is_empty());
        let body = call.body.unwrap();
        assert_eq!(body["max_event_count"], 1);
        assert_eq!(body["owner"], "https://api.calendly.com/event_types/AAAA");
        assert_eq!(body["owner_type"], "EventType");
    }

    #[test]
    fn normalize_create_extracts_resource_booking_url() {
        let raw = json!({
            "resource": {
                "booking_url": "https://calendly.com/d/AAAA/30min",
                "owner": "https://api.calendly.com/event_types/AAAA",
                "owner_type": "EventType"
            }
        })
        .to_string();
        let out = normalize(SchedulingLinkOp::Create, raw.as_bytes()).unwrap();
        assert_eq!(out["booking_url"], "https://calendly.com/d/AAAA/30min");
        assert_eq!(out["owner"], "https://api.calendly.com/event_types/AAAA");
        assert_eq!(out["owner_type"], "EventType");
    }

    #[test]
    fn normalize_create_handles_missing_fields_without_panicking() {
        let raw = json!({ "resource": {} }).to_string();
        let out = normalize(SchedulingLinkOp::Create, raw.as_bytes()).unwrap();
        assert_eq!(out["booking_url"], Value::Null);
        assert_eq!(out["owner"], Value::Null);
        assert_eq!(out["owner_type"], Value::Null);
    }

    #[test]
    fn normalize_create_rejects_invalid_json() {
        assert!(normalize(SchedulingLinkOp::Create, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"create","event_type_uri":"AAAA"}"#),
            Ok(SchedulingLinkOp::Create)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
