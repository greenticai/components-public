//! `calendly_event_types` tool domain — pure HTTP-call building and
//! response normalization for Calendly event type operations (list/get). No
//! WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext::tools::issues` template: `EventTypeOp`
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

/// Calendly event type operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventTypeOp {
    List,
    Get,
}

/// Raw `calendly_event_types` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct EventTypesInput {
    operation: EventTypeOp,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    organization: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    count: Option<u32>,
}

/// Build the Calendly REST v2 [`HttpCall`] for a `calendly_event_types`
/// invocation.
///
/// Parses `args_json` into an [`EventTypesInput`], validates the fields
/// required by the selected [`EventTypeOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: EventTypesInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        EventTypeOp::List => build_list(&input),
        EventTypeOp::Get => build_get(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<EventTypeOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: EventTypeOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &EventTypesInput) -> Result<HttpCall, String> {
    let (owner_key, owner_value) =
        super::require_owner_query(input.user.as_deref(), input.organization.as_deref())?;
    let mut query = vec![(owner_key, owner_value)];
    if let Some(active) = input.active {
        query.push(("active".to_string(), active.to_string()));
    }
    if let Some(count) = input.count {
        query.push(("count".to_string(), count.to_string()));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: "/event_types".to_string(),
        query,
        body: None,
    })
}

fn build_get(input: &EventTypesInput) -> Result<HttpCall, String> {
    let uuid = super::require_field(input.uuid.as_deref(), "uuid")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/event_types/{uuid}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Calendly Event Types response body to the compact shape
/// returned to the model, based on the [`EventTypeOp`] that produced it.
pub fn normalize(op: EventTypeOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        EventTypeOp::List => normalize_list(raw),
        EventTypeOp::Get => normalize_get(raw),
    }
}

/// Map a single event type resource to
/// `{uri,name,duration,active,scheduling_url}`, without panicking on
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
        "duration".to_string(),
        value.get("duration").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "active".to_string(),
        value.get("active").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "scheduling_url".to_string(),
        value.get("scheduling_url").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a `GET /event_types/{uuid}` response, unwrapping Calendly's
/// `{"resource": {...}}` envelope.
fn normalize_get(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid event type response: {err}"))?;
    let resource = value.get("resource").unwrap_or(&value);
    Ok(record_of(resource))
}

/// Normalize a `GET /event_types` list response to
/// `{total,results:[{uri,name,duration,active,scheduling_url}]}`, mapping
/// the `collection[]` array. `total` is `pagination.count` when present,
/// falling back to the mapped `results` length.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid event types list response: {err}"))?;
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
    fn list_requires_exactly_one_of_user_or_organization() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("user"));

        let err = build_call(
            r#"{"operation":"list","user":"https://api.calendly.com/users/u1","organization":"https://api.calendly.com/organizations/o1"}"#,
        )
        .unwrap_err();
        assert!(err.contains("exactly one"));
    }

    #[test]
    fn list_builds_get_with_user_query() {
        let call = build_call(
            r#"{"operation":"list","user":"https://api.calendly.com/users/u1","active":true,"count":5}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/event_types");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "user" && v == "https://api.calendly.com/users/u1")
        );
        assert!(call.query.iter().any(|(k, v)| k == "active" && v == "true"));
        assert!(call.query.iter().any(|(k, v)| k == "count" && v == "5"));
        assert!(!call.query.iter().any(|(k, _)| k == "organization"));
    }

    #[test]
    fn list_builds_get_with_organization_query() {
        let call = build_call(
            r#"{"operation":"list","organization":"https://api.calendly.com/organizations/o1"}"#,
        )
        .unwrap();
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "organization"
                    && v == "https://api.calendly.com/organizations/o1")
        );
    }

    #[test]
    fn get_requires_uuid() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("uuid"));
        let call = build_call(r#"{"operation":"get","uuid":"AAAA"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/event_types/AAAA");
        assert!(call.query.is_empty());
    }

    #[test]
    fn normalize_get_unwraps_resource_envelope() {
        let raw = json!({
            "resource": {
                "uri": "https://api.calendly.com/event_types/AAAA",
                "name": "30 Minute Meeting",
                "duration": 30,
                "active": true,
                "scheduling_url": "https://calendly.com/jane/30min"
            }
        })
        .to_string();
        let out = normalize(EventTypeOp::Get, raw.as_bytes()).unwrap();
        assert_eq!(out["uri"], "https://api.calendly.com/event_types/AAAA");
        assert_eq!(out["name"], "30 Minute Meeting");
        assert_eq!(out["duration"], 30);
        assert_eq!(out["active"], true);
        assert_eq!(out["scheduling_url"], "https://calendly.com/jane/30min");
    }

    #[test]
    fn normalize_list_maps_collection_and_pagination_count() {
        let raw = json!({
            "collection": [
                { "uri": "https://api.calendly.com/event_types/AAAA", "name": "30 Minute Meeting" },
                { "uri": "https://api.calendly.com/event_types/BBBB", "name": "60 Minute Meeting" }
            ],
            "pagination": { "count": 2 }
        })
        .to_string();
        let out = normalize(EventTypeOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["name"], "30 Minute Meeting");
        assert_eq!(out["results"][1]["name"], "60 Minute Meeting");
    }

    #[test]
    fn normalize_list_falls_back_to_results_len_without_pagination() {
        let raw = json!({
            "collection": [{ "uri": "https://api.calendly.com/event_types/AAAA" }]
        })
        .to_string();
        let out = normalize(EventTypeOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 1);
    }

    #[test]
    fn normalize_list_handles_empty_collection() {
        let raw = json!({ "collection": [], "pagination": { "count": 0 } }).to_string();
        let out = normalize(EventTypeOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn record_of_handles_missing_fields_without_panicking() {
        let out = record_of(&json!({}));
        assert_eq!(out["uri"], Value::Null);
        assert_eq!(out["name"], Value::Null);
        assert_eq!(out["duration"], Value::Null);
        assert_eq!(out["active"], Value::Null);
        assert_eq!(out["scheduling_url"], Value::Null);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","uuid":"AAAA"}"#),
            Ok(EventTypeOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
