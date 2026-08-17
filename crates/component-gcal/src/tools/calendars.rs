//! `gcal_calendars` tool domain — pure HTTP-call building and response
//! normalization for Google Calendar calendar-list operations (list/get/
//! create). No WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext::tools::issues` template: `CalendarOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Google Calendar calendar-list operation selected by the `operation`
/// input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarOp {
    List,
    Get,
    Create,
}

/// Raw `gcal_calendars` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct CalendarsInput {
    operation: CalendarOp,
    #[serde(default)]
    calendar_id: Option<String>,
    #[serde(default)]
    summary: Option<String>,
}

/// Calendar id to address, defaulting to `"primary"` when omitted.
fn calendar_id(input: &CalendarsInput) -> String {
    input
        .calendar_id
        .clone()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| "primary".to_string())
}

/// Build the Google Calendar REST v3 [`HttpCall`] for a `gcal_calendars`
/// invocation.
///
/// Parses `args_json` into a [`CalendarsInput`], validates the fields
/// required by the selected [`CalendarOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: CalendarsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        CalendarOp::List => build_list(),
        CalendarOp::Get => build_get(&input),
        CalendarOp::Create => build_create(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<CalendarOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: CalendarOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

// `Result` return kept for uniformity with the other `build_*` helpers this
// module's `build_call` dispatch matches on (some of which do fail).
#[allow(clippy::unnecessary_wraps)]
fn build_list() -> Result<HttpCall, String> {
    Ok(HttpCall {
        method: Method::Get,
        path: "/users/me/calendarList".to_string(),
        query: Vec::new(),
        body: None,
    })
}

// `Result` return kept for uniformity with the other `build_*` helpers this
// module's `build_call` dispatch matches on (some of which do fail);
// `calendar_id` always defaults to `"primary"`, so this one never does.
#[allow(clippy::unnecessary_wraps)]
fn build_get(input: &CalendarsInput) -> Result<HttpCall, String> {
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/calendars/{}", calendar_id(input)),
        query: Vec::new(),
        body: None,
    })
}

fn build_create(input: &CalendarsInput) -> Result<HttpCall, String> {
    let summary = super::require_field(input.summary.as_deref(), "summary")?.to_string();
    Ok(HttpCall {
        method: Method::Post,
        path: "/calendars".to_string(),
        query: Vec::new(),
        body: Some(json!({ "summary": summary })),
    })
}

/// Map a raw Google Calendar calendars/calendarList response body to the
/// compact shape returned to the model, based on the [`CalendarOp`] that
/// produced it.
pub fn normalize(op: CalendarOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        CalendarOp::List => normalize_list(raw),
        CalendarOp::Get | CalendarOp::Create => normalize_record(raw),
    }
}

/// Map a single calendar/calendarListEntry resource to
/// `{id,summary,timeZone?,accessRole?}`, without panicking on missing
/// fields.
fn map_calendar_record(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "summary".to_string(),
        value.get("summary").cloned().unwrap_or(Value::Null),
    );
    if let Some(time_zone) = value.get("timeZone") {
        out.insert("timeZone".to_string(), time_zone.clone());
    }
    if let Some(access_role) = value.get("accessRole") {
        out.insert("accessRole".to_string(), access_role.clone());
    }
    Value::Object(out)
}

fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid calendar response: {err}"))?;
    Ok(map_calendar_record(&value))
}

/// Normalize a `calendarList.list` response to
/// `{total,results:[{id,summary,timeZone?,accessRole?}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid calendarList response: {err}"))?;
    let results: Vec<Value> = value
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(map_calendar_record)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Method;

    #[test]
    fn list_builds_get_on_users_me_calendar_list() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/users/me/calendarList");
        assert!(call.query.is_empty());
        assert!(call.body.is_none());
    }

    #[test]
    fn get_defaults_to_primary_calendar() {
        let call = build_call(r#"{"operation":"get"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/calendars/primary");
    }

    #[test]
    fn get_uses_explicit_calendar_id() {
        let call = build_call(r#"{"operation":"get","calendar_id":"team@example.com"}"#).unwrap();
        assert_eq!(call.path, "/calendars/team@example.com");
    }

    #[test]
    fn create_requires_summary() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("summary"));
        let call = build_call(r#"{"operation":"create","summary":"Team Calendar"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/calendars");
        assert_eq!(call.body.as_ref().unwrap()["summary"], "Team Calendar");
    }

    #[test]
    fn normalize_record_extracts_fields_without_panicking() {
        let raw =
            br#"{"id":"primary","summary":"Bima","timeZone":"Asia/Jakarta","accessRole":"owner"}"#;
        let out = normalize(CalendarOp::Get, raw).unwrap();
        assert_eq!(out["id"], "primary");
        assert_eq!(out["summary"], "Bima");
        assert_eq!(out["timeZone"], "Asia/Jakarta");
        assert_eq!(out["accessRole"], "owner");
    }

    #[test]
    fn normalize_record_handles_missing_optional_fields() {
        let raw = br#"{"id":"primary"}"#;
        let out = normalize(CalendarOp::Create, raw).unwrap();
        assert_eq!(out["summary"], Value::Null);
        assert!(out.get("timeZone").is_none());
        assert!(out.get("accessRole").is_none());
    }

    #[test]
    fn normalize_list_maps_items() {
        let raw = br#"{"items":[{"id":"primary","summary":"Bima"},{"id":"team@example.com","summary":"Team"}]}"#;
        let out = normalize(CalendarOp::List, raw).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["id"], "primary");
        assert_eq!(out["results"][1]["summary"], "Team");
    }

    #[test]
    fn normalize_list_handles_empty_items() {
        let raw = br#"{"items":[]}"#;
        let out = normalize(CalendarOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","calendar_id":"primary"}"#),
            Ok(CalendarOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
