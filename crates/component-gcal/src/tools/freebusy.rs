//! `gcal_freebusy` tool domain — pure HTTP-call building and response
//! normalization for the Google Calendar Free/Busy query. No WIT imports —
//! this module is fully host-testable; the actual `extension-host/http`
//! invocation and `describe()` tool metadata live in `lib.rs` /
//! `tool_meta.rs`.
//!
//! Follows the `component-jira-ext::tools::issues` template: `FreebusyOp`
//! (input enum, one variant today) -> `build_call` (pure request builder)
//! -> `normalize` (pure response mapper), with no WIT/host types crossing
//! the boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Google Calendar free/busy operation selected by the `operation` input
/// field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreebusyOp {
    Query,
}

/// Raw `gcal_freebusy` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct FreebusyInput {
    operation: FreebusyOp,
    #[serde(default)]
    time_min: Option<String>,
    #[serde(default)]
    time_max: Option<String>,
    #[serde(default)]
    calendar_ids: Option<Vec<String>>,
}

/// Calendar ids to query, defaulting to `["primary"]` when omitted or empty.
fn calendar_ids(input: &FreebusyInput) -> Vec<String> {
    match &input.calendar_ids {
        Some(ids) if !ids.is_empty() => ids.clone(),
        _ => vec!["primary".to_string()],
    }
}

/// Build the Google Calendar REST v3 [`HttpCall`] for a `gcal_freebusy`
/// invocation.
///
/// Parses `args_json` into a [`FreebusyInput`], validates `time_min` and
/// `time_max`, and returns the resulting request. On missing input or a
/// missing required field, returns `Err` naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: FreebusyInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        FreebusyOp::Query => build_query(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<FreebusyOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: FreebusyOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_query(input: &FreebusyInput) -> Result<HttpCall, String> {
    let time_min = super::require_field(input.time_min.as_deref(), "time_min")?.to_string();
    let time_max = super::require_field(input.time_max.as_deref(), "time_max")?.to_string();
    let items: Vec<Value> = calendar_ids(input)
        .into_iter()
        .map(|id| json!({ "id": id }))
        .collect();
    Ok(HttpCall {
        method: Method::Post,
        path: "/freeBusy".to_string(),
        query: Vec::new(),
        body: Some(json!({
            "timeMin": time_min,
            "timeMax": time_max,
            "items": items,
        })),
    })
}

/// Map a raw Google Calendar `freeBusy` response body to
/// `{results:[{calendar_id,busy:[{start,end}]}]}`.
pub fn normalize(op: FreebusyOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        FreebusyOp::Query => normalize_query(raw),
    }
}

fn normalize_query(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid freeBusy response: {err}"))?;

    let results: Vec<Value> = value
        .get("calendars")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(calendar_id, entry)| {
            let busy = entry
                .get("busy")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new()));
            let mut out = Map::new();
            out.insert(
                "calendar_id".to_string(),
                Value::String(calendar_id.clone()),
            );
            out.insert("busy".to_string(), busy);
            Value::Object(out)
        })
        .collect();

    Ok(json!({ "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Method;

    #[test]
    fn query_requires_time_min_and_time_max() {
        let err = build_call(r#"{"operation":"query"}"#).unwrap_err();
        assert!(err.contains("time_min"));

        let err =
            build_call(r#"{"operation":"query","time_min":"2026-07-01T00:00:00Z"}"#).unwrap_err();
        assert!(err.contains("time_max"));
    }

    #[test]
    fn query_defaults_calendar_ids_to_primary() {
        let call = build_call(
            r#"{"operation":"query","time_min":"2026-07-01T00:00:00Z","time_max":"2026-07-02T00:00:00Z"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/freeBusy");
        let body = call.body.unwrap();
        assert_eq!(body["timeMin"], "2026-07-01T00:00:00Z");
        assert_eq!(body["timeMax"], "2026-07-02T00:00:00Z");
        assert_eq!(body["items"], json!([{"id": "primary"}]));
    }

    #[test]
    fn query_uses_explicit_calendar_ids() {
        let call = build_call(
            r#"{"operation":"query","time_min":"2026-07-01T00:00:00Z","time_max":"2026-07-02T00:00:00Z","calendar_ids":["primary","team@example.com"]}"#,
        )
        .unwrap();
        let body = call.body.unwrap();
        assert_eq!(
            body["items"],
            json!([{"id": "primary"}, {"id": "team@example.com"}])
        );
    }

    #[test]
    fn normalize_extracts_per_calendar_busy_blocks() {
        let raw = br#"{"calendars":{"primary":{"busy":[{"start":"2026-07-01T09:00:00Z","end":"2026-07-01T10:00:00Z"}]},"team@example.com":{"busy":[]}}}"#;
        let out = normalize(FreebusyOp::Query, raw).unwrap();
        let results = out["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);

        let primary = results
            .iter()
            .find(|entry| entry["calendar_id"] == "primary")
            .expect("primary entry present");
        assert_eq!(primary["busy"][0]["start"], "2026-07-01T09:00:00Z");
        assert_eq!(primary["busy"][0]["end"], "2026-07-01T10:00:00Z");

        let team = results
            .iter()
            .find(|entry| entry["calendar_id"] == "team@example.com")
            .expect("team entry present");
        assert_eq!(team["busy"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_handles_missing_calendars_without_panicking() {
        let raw = br"{}";
        let out = normalize(FreebusyOp::Query, raw).unwrap();
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op() {
        assert_eq!(
            parse_operation(r#"{"operation":"query"}"#),
            Ok(FreebusyOp::Query)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
