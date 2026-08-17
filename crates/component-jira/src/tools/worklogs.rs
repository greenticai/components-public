//! `jira_worklogs` tool domain — pure HTTP-call building and response
//! normalization for Jira issue worklog operations (add/list). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation lives in `lib.rs`.
//!
//! Follows the `tools::issues` template: `WorklogOp` (input enum) ->
//! `build_call` (pure request builder) -> `normalize` (pure response
//! mapper).
//!
//! Worklog comment handling: Jira REST v3 requires the worklog `comment`
//! field to be Atlassian Document Format (ADF), unlike `jira_comments`
//! `body`, which Jira accepts as either a plain string or ADF. To keep the
//! tool input ergonomic, a plain string `comment` is auto-wrapped into a
//! minimal one-paragraph ADF doc before being sent; a `comment` that is
//! already a JSON object is passed through unchanged (assumed to already be
//! a valid ADF doc), mirroring the string-or-object pattern in
//! `tools::comments`.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Jira worklog operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorklogOp {
    Add,
    List,
}

/// Raw `jira_worklogs` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct WorklogsInput {
    operation: WorklogOp,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    time_spent: Option<String>,
    #[serde(default)]
    comment: Option<Value>,
    #[serde(default)]
    started: Option<String>,
}

/// Build the Jira REST v3 [`HttpCall`] for a `jira_worklogs` invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: WorklogsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        WorklogOp::Add => build_add(&input),
        WorklogOp::List => build_list(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<WorklogOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: WorklogOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_add(input: &WorklogsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let time_spent = super::require_field(input.time_spent.as_deref(), "time_spent")?;
    let mut body = Map::new();
    body.insert("timeSpent".to_string(), json!(time_spent));
    if let Some(comment) = normalize_comment(input.comment.as_ref()) {
        body.insert("comment".to_string(), comment);
    }
    if let Some(started) = input.started.as_deref().filter(|s| !s.is_empty()) {
        body.insert("started".to_string(), json!(started));
    }
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/rest/api/3/issue/{id}/worklog"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

/// Normalize a caller-supplied worklog `comment` into the Atlassian Document
/// Format (ADF) Jira REST v3 requires for this field: a plain non-empty
/// string is wrapped into a minimal one-paragraph ADF doc; an object is
/// assumed to already be a valid ADF doc and passed through unchanged;
/// `null`, an empty string, or any other JSON shape is treated as "no
/// comment" (`None`, so the field is omitted from the request body).
fn normalize_comment(comment: Option<&Value>) -> Option<Value> {
    match comment {
        Some(Value::String(text)) if !text.is_empty() => Some(adf_doc(text)),
        Some(Value::Object(_)) => comment.cloned(),
        _ => None,
    }
}

/// Build a minimal Atlassian Document Format doc wrapping a single
/// paragraph of plain text: `{type:doc,version:1,content:[{type:paragraph,
/// content:[{type:text,text}]}]}`.
fn adf_doc(text: &str) -> Value {
    json!({
        "type": "doc",
        "version": 1,
        "content": [
            {
                "type": "paragraph",
                "content": [
                    { "type": "text", "text": text }
                ]
            }
        ]
    })
}

fn build_list(input: &WorklogsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/rest/api/3/issue/{id}/worklog"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Jira REST v3 response body to the compact shape returned to
/// the model, based on the [`WorklogOp`] that produced it.
pub fn normalize(op: WorklogOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        WorklogOp::Add => normalize_record(raw),
        WorklogOp::List => normalize_list(raw),
    }
}

/// Build the compact `{id,author,timeSpentSeconds,started}` shape from a
/// single parsed worklog JSON value. Shared by `normalize_record`
/// (single-worklog responses) and `normalize_list` (each entry of the
/// `worklogs` array).
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "author".to_string(),
        value
            .get("author")
            .and_then(|author| author.get("displayName"))
            .cloned()
            .unwrap_or(Value::Null),
    );
    out.insert(
        "timeSpentSeconds".to_string(),
        value
            .get("timeSpentSeconds")
            .cloned()
            .unwrap_or(Value::Null),
    );
    out.insert(
        "started".to_string(),
        value.get("started").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a single-worklog response (add) to
/// `{id,author,timeSpentSeconds,started}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid worklog response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/issue/{id}/worklog` list response to
/// `{total,results:[{id,author,timeSpentSeconds,started}]}`. Jira nests
/// results under `worklogs`, not `results` or `issues`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid worklog list response: {err}"))?;
    let total = value.get("total").cloned().unwrap_or(Value::Null);
    let results: Vec<Value> = value
        .get("worklogs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(record_of)
        .collect();
    Ok(json!({ "total": total, "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Method;

    #[test]
    fn add_builds_post_with_time_spent() {
        let call = build_call(r#"{"operation":"add","id":"AB-1","time_spent":"3h 30m"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/rest/api/3/issue/AB-1/worklog");
        assert_eq!(call.body.as_ref().unwrap()["timeSpent"], "3h 30m");
        assert!(call.body.as_ref().unwrap().get("comment").is_none());
        assert!(call.body.as_ref().unwrap().get("started").is_none());
    }

    #[test]
    fn add_includes_optional_comment_and_started() {
        let call = build_call(
            r#"{"operation":"add","id":"AB-1","time_spent":"1h","comment":"Investigated","started":"2026-07-01T09:00:00.000+0000"}"#,
        )
        .unwrap();
        assert_eq!(
            call.body.as_ref().unwrap()["comment"],
            json!({
                "type": "doc",
                "version": 1,
                "content": [
                    {
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": "Investigated" }]
                    }
                ]
            })
        );
        assert_eq!(
            call.body.as_ref().unwrap()["started"],
            "2026-07-01T09:00:00.000+0000"
        );
    }

    #[test]
    fn add_wraps_string_comment_into_adf() {
        let call =
            build_call(r#"{"operation":"add","id":"AB-1","time_spent":"1h","comment":"hi"}"#)
                .unwrap();
        let comment = &call.body.as_ref().unwrap()["comment"];
        assert_eq!(comment["type"], "doc");
        assert_eq!(comment["version"], 1);
        assert_eq!(comment["content"][0]["type"], "paragraph");
        assert_eq!(comment["content"][0]["content"][0]["type"], "text");
        assert_eq!(comment["content"][0]["content"][0]["text"], "hi");
    }

    #[test]
    fn add_passes_through_adf_object_comment_unchanged() {
        let call = build_call(
            r#"{"operation":"add","id":"AB-1","time_spent":"1h","comment":{"type":"doc","version":1,"content":[]}}"#,
        )
        .unwrap();
        assert_eq!(
            call.body.as_ref().unwrap()["comment"],
            json!({"type":"doc","version":1,"content":[]})
        );
    }

    #[test]
    fn add_without_comment_omits_comment_field() {
        let call = build_call(r#"{"operation":"add","id":"AB-1","time_spent":"1h"}"#).unwrap();
        assert!(call.body.as_ref().unwrap().get("comment").is_none());
    }

    #[test]
    fn add_with_empty_string_comment_omits_comment_field() {
        let call = build_call(r#"{"operation":"add","id":"AB-1","time_spent":"1h","comment":""}"#)
            .unwrap();
        assert!(call.body.as_ref().unwrap().get("comment").is_none());
    }

    #[test]
    fn add_missing_id_names_field() {
        let err = build_call(r#"{"operation":"add","time_spent":"1h"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn add_missing_time_spent_names_field() {
        let err = build_call(r#"{"operation":"add","id":"AB-1"}"#).unwrap_err();
        assert!(err.contains("time_spent"));
    }

    #[test]
    fn list_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"list","id":"AB-1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/api/3/issue/AB-1/worklog");
    }

    #[test]
    fn list_missing_id_names_field() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_add_extracts_record_shape() {
        let raw = br#"{"id":"100","author":{"displayName":"Jane Doe"},"timeSpentSeconds":3600,"started":"2026-07-01T09:00:00.000+0000"}"#;
        let out = normalize(WorklogOp::Add, raw).unwrap();
        assert_eq!(out["id"], "100");
        assert_eq!(out["author"], "Jane Doe");
        assert_eq!(out["timeSpentSeconds"], 3600);
        assert_eq!(out["started"], "2026-07-01T09:00:00.000+0000");
    }

    #[test]
    fn normalize_add_handles_missing_author() {
        let raw =
            br#"{"id":"100","timeSpentSeconds":3600,"started":"2026-07-01T09:00:00.000+0000"}"#;
        let out = normalize(WorklogOp::Add, raw).unwrap();
        assert_eq!(out["author"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_worklogs_array() {
        let raw = br#"{"total":1,"worklogs":[{"id":"100","author":{"displayName":"Jane Doe"},"timeSpentSeconds":3600,"started":"2026-07-01T09:00:00.000+0000"}]}"#;
        let out = normalize(WorklogOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "100");
        assert_eq!(out["results"][0]["author"], "Jane Doe");
        assert_eq!(out["results"][0]["timeSpentSeconds"], 3600);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"list","id":"AB-1"}"#),
            Ok(WorklogOp::List)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
