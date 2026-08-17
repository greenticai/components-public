//! `calendly_availability` tool domain — pure HTTP-call building and
//! response normalization for Calendly user busy-time and availability-
//! schedule lookups (`user_busy_times`/`list_schedules`). No WIT imports —
//! this module is fully host-testable; the actual `extension-host/http`
//! invocation and `describe()` tool metadata live in `lib.rs` /
//! `tool_meta.rs`.
//!
//! Follows the `component-jira-ext::tools::issues` template:
//! `AvailabilityOp` (input enum) -> `build_call` (pure request builder) ->
//! `normalize` (pure response mapper), with no WIT/host types crossing the
//! boundary. Unlike the other list-producing Calendly domains, neither
//! response here carries a `total`/`pagination.count` in this normalized
//! shape — both are returned as `{results:[...]}`.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Calendly availability operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvailabilityOp {
    UserBusyTimes,
    ListSchedules,
}

/// Raw `calendly_availability` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct AvailabilityInput {
    operation: AvailabilityOp,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    start_time: Option<String>,
    #[serde(default)]
    end_time: Option<String>,
}

/// Build the Calendly REST v2 [`HttpCall`] for a `calendly_availability`
/// invocation.
///
/// Parses `args_json` into an [`AvailabilityInput`], validates the fields
/// required by the selected [`AvailabilityOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: AvailabilityInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        AvailabilityOp::UserBusyTimes => build_user_busy_times(&input),
        AvailabilityOp::ListSchedules => build_list_schedules(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<AvailabilityOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: AvailabilityOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_user_busy_times(input: &AvailabilityInput) -> Result<HttpCall, String> {
    let user = super::require_field(input.user.as_deref(), "user")?;
    let start_time = super::require_field(input.start_time.as_deref(), "start_time")?;
    let end_time = super::require_field(input.end_time.as_deref(), "end_time")?;
    Ok(HttpCall {
        method: Method::Get,
        path: "/user_busy_times".to_string(),
        query: vec![
            ("user".to_string(), user.to_string()),
            ("start_time".to_string(), start_time.to_string()),
            ("end_time".to_string(), end_time.to_string()),
        ],
        body: None,
    })
}

fn build_list_schedules(input: &AvailabilityInput) -> Result<HttpCall, String> {
    let user = super::require_field(input.user.as_deref(), "user")?;
    Ok(HttpCall {
        method: Method::Get,
        path: "/user_availability_schedules".to_string(),
        query: vec![("user".to_string(), user.to_string())],
        body: None,
    })
}

/// Map a raw Calendly Availability response body to the compact shape
/// returned to the model, based on the [`AvailabilityOp`] that produced it.
pub fn normalize(op: AvailabilityOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        AvailabilityOp::UserBusyTimes => normalize_user_busy_times(raw),
        AvailabilityOp::ListSchedules => normalize_list_schedules(raw),
    }
}

/// Map a single busy-time block to `{type,start_time,end_time}`, without
/// panicking on missing/absent fields.
fn busy_time_record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "type".to_string(),
        value.get("type").cloned().unwrap_or(Value::Null),
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

/// Normalize a `GET /user_busy_times` response to
/// `{results:[{type,start_time,end_time}]}`, mapping the `collection[]`
/// array.
fn normalize_user_busy_times(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid user busy times response: {err}"))?;
    let results: Vec<Value> = value
        .get("collection")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(busy_time_record_of)
        .collect();
    Ok(json!({ "results": results }))
}

/// Map a single availability schedule to `{uri,name,default}`, without
/// panicking on missing/absent fields.
fn schedule_record_of(value: &Value) -> Value {
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
        "default".to_string(),
        value.get("default").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a `GET /user_availability_schedules` response to
/// `{results:[{uri,name,default}]}`, mapping the `collection[]` array.
fn normalize_list_schedules(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid user availability schedules response: {err}"))?;
    let results: Vec<Value> = value
        .get("collection")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(schedule_record_of)
        .collect();
    Ok(json!({ "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_busy_times_requires_user_start_time_and_end_time() {
        let err = build_call(r#"{"operation":"user_busy_times"}"#).unwrap_err();
        assert!(err.contains("user"));

        let err = build_call(
            r#"{"operation":"user_busy_times","user":"https://api.calendly.com/users/u1"}"#,
        )
        .unwrap_err();
        assert!(err.contains("start_time"));

        let err = build_call(
            r#"{"operation":"user_busy_times","user":"https://api.calendly.com/users/u1","start_time":"2026-07-01T00:00:00Z"}"#,
        )
        .unwrap_err();
        assert!(err.contains("end_time"));
    }

    #[test]
    fn user_busy_times_builds_get_with_all_query_params() {
        let call = build_call(
            r#"{"operation":"user_busy_times","user":"https://api.calendly.com/users/u1","start_time":"2026-07-01T00:00:00Z","end_time":"2026-07-02T00:00:00Z"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/user_busy_times");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "user" && v == "https://api.calendly.com/users/u1")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "start_time" && v == "2026-07-01T00:00:00Z")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "end_time" && v == "2026-07-02T00:00:00Z")
        );
        assert_eq!(call.query.len(), 3);
        assert!(call.body.is_none());
    }

    #[test]
    fn list_schedules_requires_user() {
        let err = build_call(r#"{"operation":"list_schedules"}"#).unwrap_err();
        assert!(err.contains("user"));
    }

    #[test]
    fn list_schedules_builds_get_with_user_query() {
        let call = build_call(
            r#"{"operation":"list_schedules","user":"https://api.calendly.com/users/u1"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/user_availability_schedules");
        assert_eq!(
            call.query,
            vec![(
                "user".to_string(),
                "https://api.calendly.com/users/u1".to_string()
            )]
        );
    }

    #[test]
    fn normalize_user_busy_times_maps_collection() {
        let raw = json!({
            "collection": [
                { "type": "calendly", "start_time": "2026-07-01T09:00:00Z", "end_time": "2026-07-01T09:30:00Z" },
                { "type": "external", "start_time": "2026-07-01T10:00:00Z", "end_time": "2026-07-01T10:30:00Z" }
            ]
        })
        .to_string();
        let out = normalize(AvailabilityOp::UserBusyTimes, raw.as_bytes()).unwrap();
        assert_eq!(out["results"][0]["type"], "calendly");
        assert_eq!(out["results"][1]["type"], "external");
        assert_eq!(out["results"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn normalize_user_busy_times_handles_empty_collection() {
        let raw = json!({ "collection": [] }).to_string();
        let out = normalize(AvailabilityOp::UserBusyTimes, raw.as_bytes()).unwrap();
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_list_schedules_maps_collection() {
        let raw = json!({
            "collection": [
                { "uri": "https://api.calendly.com/user_availability_schedules/AAAA", "name": "Working hours", "default": true }
            ]
        })
        .to_string();
        let out = normalize(AvailabilityOp::ListSchedules, raw.as_bytes()).unwrap();
        assert_eq!(
            out["results"][0]["uri"],
            "https://api.calendly.com/user_availability_schedules/AAAA"
        );
        assert_eq!(out["results"][0]["name"], "Working hours");
        assert_eq!(out["results"][0]["default"], true);
    }

    #[test]
    fn normalize_list_schedules_handles_missing_fields_without_panicking() {
        let raw = json!({ "collection": [{}] }).to_string();
        let out = normalize(AvailabilityOp::ListSchedules, raw.as_bytes()).unwrap();
        assert_eq!(out["results"][0]["uri"], Value::Null);
        assert_eq!(out["results"][0]["name"], Value::Null);
        assert_eq!(out["results"][0]["default"], Value::Null);
    }

    #[test]
    fn normalize_rejects_invalid_json() {
        assert!(normalize(AvailabilityOp::UserBusyTimes, b"not json").is_err());
        assert!(normalize(AvailabilityOp::ListSchedules, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"list_schedules","user":"AAAA"}"#),
            Ok(AvailabilityOp::ListSchedules)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
