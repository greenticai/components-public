//! `calendly_invitees` tool domain — pure HTTP-call building and response
//! normalization for Calendly scheduled-event invitee operations
//! (list/get). No WIT imports — this module is fully host-testable; the
//! actual `extension-host/http` invocation and `describe()` tool metadata
//! live in `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext::tools::issues` template: `InviteeOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Calendly invitee operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteeOp {
    List,
    Get,
}

/// Raw `calendly_invitees` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct InviteesInput {
    operation: InviteeOp,
    #[serde(default)]
    event_uuid: Option<String>,
    #[serde(default)]
    invitee_uuid: Option<String>,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

/// Build the Calendly REST v2 [`HttpCall`] for a `calendly_invitees`
/// invocation.
///
/// Parses `args_json` into an [`InviteesInput`], validates the fields
/// required by the selected [`InviteeOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: InviteesInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        InviteeOp::List => build_list(&input),
        InviteeOp::Get => build_get(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<InviteeOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: InviteeOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &InviteesInput) -> Result<HttpCall, String> {
    let event_uuid = super::require_field(input.event_uuid.as_deref(), "event_uuid")?;
    let mut query = Vec::new();
    if let Some(count) = input.count {
        query.push(("count".to_string(), count.to_string()));
    }
    if let Some(status) = input.status.as_deref().filter(|v| !v.is_empty()) {
        query.push(("status".to_string(), status.to_string()));
    }
    if let Some(email) = input.email.as_deref().filter(|v| !v.is_empty()) {
        query.push(("email".to_string(), email.to_string()));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/scheduled_events/{event_uuid}/invitees"),
        query,
        body: None,
    })
}

fn build_get(input: &InviteesInput) -> Result<HttpCall, String> {
    let event_uuid = super::require_field(input.event_uuid.as_deref(), "event_uuid")?;
    let invitee_uuid = super::require_field(input.invitee_uuid.as_deref(), "invitee_uuid")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/scheduled_events/{event_uuid}/invitees/{invitee_uuid}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Calendly Invitees response body to the compact shape returned
/// to the model, based on the [`InviteeOp`] that produced it.
pub fn normalize(op: InviteeOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        InviteeOp::List => normalize_list(raw),
        InviteeOp::Get => normalize_get(raw),
    }
}

/// Map a single invitee resource to `{uri,email,name,status}`, without
/// panicking on missing/absent fields.
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "uri".to_string(),
        value.get("uri").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "email".to_string(),
        value.get("email").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "name".to_string(),
        value.get("name").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "status".to_string(),
        value.get("status").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a `GET /scheduled_events/{event_uuid}/invitees/{invitee_uuid}`
/// response, unwrapping Calendly's `{"resource": {...}}` envelope.
fn normalize_get(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid invitee response: {err}"))?;
    let resource = value.get("resource").unwrap_or(&value);
    Ok(record_of(resource))
}

/// Normalize a `GET /scheduled_events/{event_uuid}/invitees` list response
/// to `{total,results:[{uri,email,name,status}]}`, mapping the
/// `collection[]` array. `total` is `pagination.count` when present,
/// falling back to the mapped `results` length.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid invitees list response: {err}"))?;
    let results: Vec<Value> = value
        .get("collection")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(record_of)
        .collect();
    let total = value
        .get("pagination")
        .and_then(|pagination| pagination.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(results.len() as u64);
    Ok(json!({ "total": total, "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_requires_event_uuid() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("event_uuid"));
    }

    #[test]
    fn list_builds_get_with_nested_path_and_filters() {
        let call = build_call(
            r#"{"operation":"list","event_uuid":"EEEE","count":10,"status":"active","email":"jane@example.com"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/scheduled_events/EEEE/invitees");
        assert!(call.query.iter().any(|(k, v)| k == "count" && v == "10"));
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "status" && v == "active")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "email" && v == "jane@example.com")
        );
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list","event_uuid":"EEEE"}"#).unwrap();
        assert!(call.query.is_empty());
    }

    #[test]
    fn get_requires_event_uuid_and_invitee_uuid() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("event_uuid"));

        let err = build_call(r#"{"operation":"get","event_uuid":"EEEE"}"#).unwrap_err();
        assert!(err.contains("invitee_uuid"));

        let call =
            build_call(r#"{"operation":"get","event_uuid":"EEEE","invitee_uuid":"IIII"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/scheduled_events/EEEE/invitees/IIII");
    }

    #[test]
    fn normalize_get_unwraps_resource_envelope() {
        let raw = json!({
            "resource": {
                "uri": "https://api.calendly.com/scheduled_events/EEEE/invitees/IIII",
                "email": "jane@example.com",
                "name": "Jane Doe",
                "status": "active"
            }
        })
        .to_string();
        let out = normalize(InviteeOp::Get, raw.as_bytes()).unwrap();
        assert_eq!(
            out["uri"],
            "https://api.calendly.com/scheduled_events/EEEE/invitees/IIII"
        );
        assert_eq!(out["email"], "jane@example.com");
        assert_eq!(out["name"], "Jane Doe");
        assert_eq!(out["status"], "active");
    }

    #[test]
    fn normalize_get_handles_missing_fields_without_panicking() {
        let raw = json!({ "resource": {} }).to_string();
        let out = normalize(InviteeOp::Get, raw.as_bytes()).unwrap();
        assert_eq!(out["uri"], Value::Null);
        assert_eq!(out["email"], Value::Null);
        assert_eq!(out["name"], Value::Null);
        assert_eq!(out["status"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_collection_and_pagination_count() {
        let raw = json!({
            "collection": [
                { "uri": "https://api.calendly.com/.../IIII1", "email": "a@example.com" },
                { "uri": "https://api.calendly.com/.../IIII2", "email": "b@example.com" }
            ],
            "pagination": { "count": 2 }
        })
        .to_string();
        let out = normalize(InviteeOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["email"], "a@example.com");
        assert_eq!(out["results"][1]["email"], "b@example.com");
    }

    #[test]
    fn normalize_list_falls_back_to_results_len_without_pagination() {
        let raw = json!({ "collection": [{ "uri": "x" }] }).to_string();
        let out = normalize(InviteeOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 1);
    }

    #[test]
    fn normalize_list_handles_empty_collection() {
        let raw = json!({ "collection": [] }).to_string();
        let out = normalize(InviteeOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"list","event_uuid":"EEEE"}"#),
            Ok(InviteeOp::List)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
