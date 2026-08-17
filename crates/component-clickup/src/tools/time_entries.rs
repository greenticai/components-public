//! `clickup_time_entries` tool domain — pure HTTP-call building and
//! response normalization for ClickUp time-tracking operations
//! (start/stop/list/add). No WIT imports — this module is fully
//! host-testable; the actual `extension-host/http` invocation lives in
//! `lib.rs`.
//!
//! Follows the `tools::tasks` template: `TimeEntryOp` (input enum) ->
//! `build_call` (pure request builder) -> `normalize` (pure response
//! mapper).

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// ClickUp time-tracking operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeEntryOp {
    Start,
    Stop,
    List,
    Add,
}

/// Raw `clickup_time_entries` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct TimeEntriesInput {
    operation: TimeEntryOp,
    #[serde(default)]
    team_id: Option<String>,
    #[serde(default)]
    tid: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    start: Option<Value>,
    #[serde(default)]
    duration: Option<Value>,
}

/// Build the ClickUp API v2 [`HttpCall`] for a `clickup_time_entries`
/// invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: TimeEntriesInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        TimeEntryOp::Start => build_start(&input),
        TimeEntryOp::Stop => build_stop(&input),
        TimeEntryOp::List => build_list(&input),
        TimeEntryOp::Add => build_add(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<TimeEntryOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: TimeEntryOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

/// Fetch a required JSON value field, rejecting `None`. Unlike
/// [`super::require_field`], `start`/`duration` may be numbers, so the
/// value is kept as-is rather than coerced to `&str`.
fn require_value<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a Value, String> {
    value.ok_or_else(|| format!("missing required field: {name}"))
}

fn build_start(input: &TimeEntriesInput) -> Result<HttpCall, String> {
    let team_id = super::require_field(input.team_id.as_deref(), "team_id")?;
    let mut body = Map::new();
    if let Some(tid) = &input.tid {
        body.insert("tid".to_string(), Value::String(tid.clone()));
    }
    if let Some(description) = &input.description {
        body.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/team/{team_id}/time_entries/start"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_stop(input: &TimeEntriesInput) -> Result<HttpCall, String> {
    let team_id = super::require_field(input.team_id.as_deref(), "team_id")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/team/{team_id}/time_entries/stop"),
        query: Vec::new(),
        body: None,
    })
}

fn build_list(input: &TimeEntriesInput) -> Result<HttpCall, String> {
    let team_id = super::require_field(input.team_id.as_deref(), "team_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/team/{team_id}/time_entries"),
        query: Vec::new(),
        body: None,
    })
}

fn build_add(input: &TimeEntriesInput) -> Result<HttpCall, String> {
    let team_id = super::require_field(input.team_id.as_deref(), "team_id")?;
    let start = require_value(input.start.as_ref(), "start")?;
    let duration = require_value(input.duration.as_ref(), "duration")?;
    let mut body = Map::new();
    body.insert("start".to_string(), start.clone());
    body.insert("duration".to_string(), duration.clone());
    if let Some(tid) = &input.tid {
        body.insert("tid".to_string(), Value::String(tid.clone()));
    }
    if let Some(description) = &input.description {
        body.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/team/{team_id}/time_entries"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

/// Map a raw ClickUp API v2 response body to the compact shape returned to
/// the model, based on the [`TimeEntryOp`] that produced it.
pub fn normalize(op: TimeEntryOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        TimeEntryOp::List => normalize_list(raw),
        TimeEntryOp::Start | TimeEntryOp::Add => normalize_record(raw),
        TimeEntryOp::Stop => Ok(normalize_ack(raw)),
    }
}

/// Best-effort task id extraction: a top-level `task_id`, else
/// `task.id`, else the raw `tid` field — ClickUp's time-entry payloads use
/// all three shapes depending on the endpoint.
fn extract_task_id(value: &Value) -> Option<Value> {
    value
        .get("task_id")
        .cloned()
        .or_else(|| value.get("task").and_then(|task| task.get("id")).cloned())
        .or_else(|| value.get("tid").cloned())
}

/// Build the compact `{id,task_id?,start,duration}` shape from a single
/// parsed time-entry JSON value. Shared by [`normalize_record`]
/// (single-entry responses) and [`normalize_list`] (each entry of a time
/// entry page).
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    if let Some(task_id) = extract_task_id(value) {
        out.insert("task_id".to_string(), task_id);
    }
    out.insert(
        "start".to_string(),
        value.get("start").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "duration".to_string(),
        value.get("duration").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a single-time-entry response (start/add) to
/// `{id,task_id?,start,duration}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid time entry response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/team/{team_id}/time_entries` response to
/// `{total,results:[{id,task_id?,start,duration}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid time entry list response: {err}"))?;
    let results: Vec<Value> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(record_of)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

/// Normalize a stop response. ClickUp's stop endpoint may return the
/// stopped entry either bare or wrapped in `{data:{...}}`; `id` is
/// recovered from either shape, and stays `null` if neither is present.
fn normalize_ack(raw: &[u8]) -> Value {
    let id = if raw.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice::<Value>(raw)
            .ok()
            .and_then(|value| {
                value
                    .get("id")
                    .cloned()
                    .or_else(|| value.get("data").and_then(|data| data.get("id")).cloned())
            })
            .unwrap_or(Value::Null)
    };
    json!({ "ok": true, "id": id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Method;

    #[test]
    fn start_requires_team_id() {
        let err = build_call(r#"{"operation":"start"}"#).unwrap_err();
        assert!(err.contains("team_id"));
    }

    #[test]
    fn start_builds_post_with_optional_tid_and_description() {
        let call = build_call(
            r#"{"operation":"start","team_id":"1","tid":"9hz","description":"working"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/team/1/time_entries/start");
        assert_eq!(call.body.as_ref().unwrap()["tid"], "9hz");
        assert_eq!(call.body.as_ref().unwrap()["description"], "working");
    }

    #[test]
    fn start_body_empty_when_optional_fields_absent() {
        let call = build_call(r#"{"operation":"start","team_id":"1"}"#).unwrap();
        assert_eq!(call.body.as_ref().unwrap(), &json!({}));
    }

    #[test]
    fn stop_requires_team_id() {
        let err = build_call(r#"{"operation":"stop"}"#).unwrap_err();
        assert!(err.contains("team_id"));
    }

    #[test]
    fn stop_builds_post_with_no_body() {
        let call = build_call(r#"{"operation":"stop","team_id":"1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/team/1/time_entries/stop");
        assert!(call.body.is_none());
    }

    #[test]
    fn list_requires_team_id() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("team_id"));
    }

    #[test]
    fn list_builds_get_with_team_path() {
        let call = build_call(r#"{"operation":"list","team_id":"1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/team/1/time_entries");
    }

    #[test]
    fn add_requires_team_id_start_and_duration() {
        assert!(build_call(r#"{"operation":"add","start":1,"duration":2}"#).is_err());
        let err = build_call(r#"{"operation":"add","team_id":"1","duration":2}"#).unwrap_err();
        assert!(err.contains("start"));
        let err = build_call(r#"{"operation":"add","team_id":"1","start":1}"#).unwrap_err();
        assert!(err.contains("duration"));
    }

    #[test]
    fn add_builds_post_with_start_duration_and_optional_fields() {
        let call = build_call(
            r#"{"operation":"add","team_id":"1","start":1567780450202,"duration":1200000,"tid":"9hz"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/team/1/time_entries");
        assert_eq!(call.body.as_ref().unwrap()["start"], 1_567_780_450_202i64);
        assert_eq!(call.body.as_ref().unwrap()["duration"], 1_200_000);
        assert_eq!(call.body.as_ref().unwrap()["tid"], "9hz");
    }

    #[test]
    fn normalize_start_extracts_id_task_id_start_duration() {
        let raw = br#"{"id":"te1","task":{"id":"9hz"},"start":"1","duration":"2"}"#;
        let out = normalize(TimeEntryOp::Start, raw).unwrap();
        assert_eq!(out["id"], "te1");
        assert_eq!(out["task_id"], "9hz");
        assert_eq!(out["start"], "1");
        assert_eq!(out["duration"], "2");
    }

    #[test]
    fn normalize_record_omits_task_id_when_absent() {
        let raw = br#"{"id":"te1","start":"1","duration":"2"}"#;
        let out = normalize(TimeEntryOp::Add, raw).unwrap();
        assert!(out.get("task_id").is_none());
    }

    #[test]
    fn normalize_list_maps_data_array() {
        let raw = br#"{"data":[{"id":"te1","tid":"9hz","start":"1","duration":"2"}]}"#;
        let out = normalize(TimeEntryOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "te1");
        assert_eq!(out["results"][0]["task_id"], "9hz");
    }

    #[test]
    fn normalize_list_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(TimeEntryOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_stop_ack_handles_empty_body() {
        let out = normalize(TimeEntryOp::Stop, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn normalize_stop_ack_recovers_id_from_bare_or_wrapped_body() {
        let out = normalize(TimeEntryOp::Stop, br#"{"id":"te1"}"#).unwrap();
        assert_eq!(out["id"], "te1");

        let out = normalize(TimeEntryOp::Stop, br#"{"data":{"id":"te2"}}"#).unwrap();
        assert_eq!(out["id"], "te2");
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"stop","team_id":"1"}"#),
            Ok(TimeEntryOp::Stop)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
