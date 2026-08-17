//! `calendly_events` tool domain (scheduled events) — pure HTTP-call
//! building and response normalization for Calendly scheduled event
//! operations (list/get/cancel). No WIT imports — this module is fully
//! host-testable; the actual `extension-host/http` invocation and
//! `describe()` tool metadata live in `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext::tools::issues` template: `EventOp`
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

/// Calendly scheduled event operation selected by the `operation` input
/// field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOp {
    List,
    Get,
    Cancel,
}

/// Raw `calendly_events` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct EventsInput {
    operation: EventOp,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    organization: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    min_start_time: Option<String>,
    #[serde(default)]
    max_start_time: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

/// Build the Calendly REST v2 [`HttpCall`] for a `calendly_events`
/// invocation.
///
/// Parses `args_json` into an [`EventsInput`], validates the fields
/// required by the selected [`EventOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: EventsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        EventOp::List => build_list(&input),
        EventOp::Get => build_get(&input),
        EventOp::Cancel => build_cancel(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<EventOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: EventOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &EventsInput) -> Result<HttpCall, String> {
    let (owner_key, owner_value) =
        super::require_owner_query(input.user.as_deref(), input.organization.as_deref())?;
    let mut query = vec![(owner_key, owner_value)];
    if let Some(status) = input.status.as_deref().filter(|v| !v.is_empty()) {
        query.push(("status".to_string(), status.to_string()));
    }
    if let Some(count) = input.count {
        query.push(("count".to_string(), count.to_string()));
    }
    if let Some(min_start_time) = input.min_start_time.as_deref().filter(|v| !v.is_empty()) {
        query.push(("min_start_time".to_string(), min_start_time.to_string()));
    }
    if let Some(max_start_time) = input.max_start_time.as_deref().filter(|v| !v.is_empty()) {
        query.push(("max_start_time".to_string(), max_start_time.to_string()));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: "/scheduled_events".to_string(),
        query,
        body: None,
    })
}

fn build_get(input: &EventsInput) -> Result<HttpCall, String> {
    let uuid = super::require_field(input.uuid.as_deref(), "uuid")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/scheduled_events/{uuid}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_cancel(input: &EventsInput) -> Result<HttpCall, String> {
    let uuid = super::require_field(input.uuid.as_deref(), "uuid")?;
    let body = match input.reason.as_deref().filter(|v| !v.is_empty()) {
        Some(reason) => json!({ "reason": reason }),
        None => json!({}),
    };
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/scheduled_events/{uuid}/cancellation"),
        query: Vec::new(),
        body: Some(body),
    })
}

/// Map a raw Calendly Scheduled Events response body to the compact shape
/// returned to the model, based on the [`EventOp`] that produced it.
pub fn normalize(op: EventOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        EventOp::List => normalize_list(raw),
        EventOp::Get => normalize_get(raw),
        EventOp::Cancel => normalize_cancel(raw),
    }
}

/// Map a single scheduled event resource to
/// `{uri,name,status,start_time,end_time}`, without panicking on
/// missing/absent fields.
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "uri".to_string(),
        value.get("uri").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "name".to_string(),
        value.get("name").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "status".to_string(),
        value.get("status").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "start_time".to_string(),
        value.get("start_time").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "end_time".to_string(),
        value.get("end_time").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a `GET /scheduled_events/{uuid}` response, unwrapping
/// Calendly's `{"resource": {...}}` envelope.
fn normalize_get(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid scheduled event response: {err}"))?;
    let resource = value.get("resource").unwrap_or(&value);
    Ok(record_of(resource))
}

/// Normalize a `GET /scheduled_events` list response to
/// `{total,results:[{uri,name,status,start_time,end_time}]}`, mapping the
/// `collection[]` array. `total` is `pagination.count` when present,
/// falling back to the mapped `results` length.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid scheduled events list response: {err}"))?;
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

/// Normalize a `POST /scheduled_events/{uuid}/cancellation` response to
/// `{ok:true, id}`. Calendly's cancellation resource does not echo the
/// scheduled event's own `uri`/id, so `id` is left `null` here; the
/// dispatch layer (`lib.rs::invoke_events`) backfills it from the request's
/// own `uuid` field.
fn normalize_cancel(raw: &[u8]) -> Result<Value, String> {
    // The cancellation response body isn't used for the normalized shape,
    // but a malformed non-empty body is still worth rejecting explicitly
    // rather than silently ignoring it.
    if !raw.is_empty() && serde_json::from_slice::<Value>(raw).is_err() {
        return Err("invalid cancellation response".to_string());
    }
    Ok(json!({ "ok": true, "id": Value::Null }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_requires_exactly_one_of_user_or_organization() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("user"));
    }

    #[test]
    fn list_builds_get_with_filters() {
        let call = build_call(
            r#"{"operation":"list","user":"https://api.calendly.com/users/u1","status":"active","count":10,"min_start_time":"2026-07-01T00:00:00Z","max_start_time":"2026-07-31T00:00:00Z"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/scheduled_events");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "user" && v == "https://api.calendly.com/users/u1")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "status" && v == "active")
        );
        assert!(call.query.iter().any(|(k, v)| k == "count" && v == "10"));
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "min_start_time" && v == "2026-07-01T00:00:00Z")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "max_start_time" && v == "2026-07-31T00:00:00Z")
        );
    }

    #[test]
    fn list_with_only_owner_has_single_query_pair() {
        let call = build_call(
            r#"{"operation":"list","organization":"https://api.calendly.com/organizations/o1"}"#,
        )
        .unwrap();
        assert_eq!(call.query.len(), 1);
    }

    #[test]
    fn get_requires_uuid() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("uuid"));
        let call = build_call(r#"{"operation":"get","uuid":"AAAA"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/scheduled_events/AAAA");
    }

    #[test]
    fn cancel_requires_uuid_and_builds_post_with_cancellation_path() {
        let err = build_call(r#"{"operation":"cancel"}"#).unwrap_err();
        assert!(err.contains("uuid"));

        let call =
            build_call(r#"{"operation":"cancel","uuid":"AAAA","reason":"conflict"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/scheduled_events/AAAA/cancellation");
        assert_eq!(call.body.as_ref().unwrap()["reason"], "conflict");
    }

    #[test]
    fn cancel_without_reason_sends_empty_body() {
        let call = build_call(r#"{"operation":"cancel","uuid":"AAAA"}"#).unwrap();
        assert_eq!(call.body, Some(json!({})));
    }

    #[test]
    fn normalize_get_unwraps_resource_envelope() {
        let raw = json!({
            "resource": {
                "uri": "https://api.calendly.com/scheduled_events/AAAA",
                "name": "30 Minute Meeting",
                "status": "active",
                "start_time": "2026-07-03T09:00:00Z",
                "end_time": "2026-07-03T09:30:00Z"
            }
        })
        .to_string();
        let out = normalize(EventOp::Get, raw.as_bytes()).unwrap();
        assert_eq!(out["uri"], "https://api.calendly.com/scheduled_events/AAAA");
        assert_eq!(out["name"], "30 Minute Meeting");
        assert_eq!(out["status"], "active");
        assert_eq!(out["start_time"], "2026-07-03T09:00:00Z");
        assert_eq!(out["end_time"], "2026-07-03T09:30:00Z");
    }

    #[test]
    fn normalize_get_handles_missing_fields_without_panicking() {
        let raw = json!({ "resource": {} }).to_string();
        let out = normalize(EventOp::Get, raw.as_bytes()).unwrap();
        assert_eq!(out["uri"], Value::Null);
        assert_eq!(out["status"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_collection_and_pagination_count() {
        let raw = json!({
            "collection": [
                { "uri": "https://api.calendly.com/scheduled_events/AAAA", "status": "active" },
                { "uri": "https://api.calendly.com/scheduled_events/BBBB", "status": "canceled" }
            ],
            "pagination": { "count": 2 }
        })
        .to_string();
        let out = normalize(EventOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["status"], "active");
        assert_eq!(out["results"][1]["status"], "canceled");
    }

    #[test]
    fn normalize_list_falls_back_to_results_len_without_pagination() {
        let raw = json!({ "collection": [{ "uri": "x" }] }).to_string();
        let out = normalize(EventOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 1);
    }

    #[test]
    fn normalize_list_handles_empty_collection() {
        let raw = json!({ "collection": [] }).to_string();
        let out = normalize(EventOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_cancel_returns_ack_with_null_id_for_backfill() {
        let out = normalize(EventOp::Cancel, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn normalize_cancel_accepts_non_empty_json_body_too() {
        let raw = json!({ "reason": "conflict", "canceled_by": "host" }).to_string();
        let out = normalize(EventOp::Cancel, raw.as_bytes()).unwrap();
        assert_eq!(out["ok"], true);
    }

    #[test]
    fn normalize_cancel_rejects_malformed_non_empty_body() {
        assert!(normalize(EventOp::Cancel, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"cancel","uuid":"AAAA"}"#),
            Ok(EventOp::Cancel)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
