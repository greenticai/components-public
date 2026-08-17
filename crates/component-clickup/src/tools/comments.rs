//! `clickup_comments` tool domain — pure HTTP-call building and response
//! normalization for ClickUp comment operations (add/list/update). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation lives in `lib.rs`.
//!
//! Follows the `tools::tasks` template: `CommentOp` (input enum) ->
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

/// ClickUp comment operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentOp {
    Add,
    List,
    Update,
}

/// Raw `clickup_comments` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct CommentsInput {
    operation: CommentOp,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    comment_id: Option<String>,
    #[serde(default)]
    comment_text: Option<String>,
    #[serde(default)]
    notify_all: Option<bool>,
    #[serde(default)]
    resolved: Option<bool>,
}

/// Build the ClickUp API v2 [`HttpCall`] for a `clickup_comments`
/// invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: CommentsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        CommentOp::Add => build_add(&input),
        CommentOp::List => build_list(&input),
        CommentOp::Update => build_update(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<CommentOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: CommentOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_add(input: &CommentsInput) -> Result<HttpCall, String> {
    let task_id = super::require_field(input.task_id.as_deref(), "task_id")?;
    let comment_text = super::require_field(input.comment_text.as_deref(), "comment_text")?;
    let mut body = Map::new();
    body.insert(
        "comment_text".to_string(),
        Value::String(comment_text.to_string()),
    );
    if let Some(notify_all) = input.notify_all {
        body.insert("notify_all".to_string(), Value::Bool(notify_all));
    }
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/task/{task_id}/comment"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_list(input: &CommentsInput) -> Result<HttpCall, String> {
    let task_id = super::require_field(input.task_id.as_deref(), "task_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/task/{task_id}/comment"),
        query: Vec::new(),
        body: None,
    })
}

fn build_update(input: &CommentsInput) -> Result<HttpCall, String> {
    let comment_id = super::require_field(input.comment_id.as_deref(), "comment_id")?;
    let comment_text = super::require_field(input.comment_text.as_deref(), "comment_text")?;
    let mut body = Map::new();
    body.insert(
        "comment_text".to_string(),
        Value::String(comment_text.to_string()),
    );
    if let Some(resolved) = input.resolved {
        body.insert("resolved".to_string(), Value::Bool(resolved));
    }
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/comment/{comment_id}"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

/// Map a raw ClickUp API v2 response body to the compact shape returned to
/// the model, based on the [`CommentOp`] that produced it.
pub fn normalize(op: CommentOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        CommentOp::List => normalize_list(raw),
        CommentOp::Add | CommentOp::Update => normalize_record(raw),
    }
}

/// Build the compact `{id,user,comment_text,date}` shape from a single
/// parsed comment JSON value. Shared by [`normalize_record`] (single-comment
/// responses) and [`normalize_list`] (each entry of a comment page).
fn record_of(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "user": value.get("user").cloned().unwrap_or(Value::Null),
        "comment_text": value.get("comment_text").cloned().unwrap_or(Value::Null),
        "date": value.get("date").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-comment response (add/update) to
/// `{id,user,comment_text,date}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid comment response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/task/{task_id}/comment` response to
/// `{total,results:[{id,user,comment_text,date}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid comment list response: {err}"))?;
    let results: Vec<Value> = value
        .get("comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(record_of)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Method;

    #[test]
    fn add_requires_task_id() {
        let err = build_call(r#"{"operation":"add","comment_text":"hi"}"#).unwrap_err();
        assert!(err.contains("task_id"));
    }

    #[test]
    fn add_requires_comment_text() {
        let err = build_call(r#"{"operation":"add","task_id":"9hz"}"#).unwrap_err();
        assert!(err.contains("comment_text"));
    }

    #[test]
    fn add_builds_post_with_comment_text_and_notify_all() {
        let call = build_call(
            r#"{"operation":"add","task_id":"9hz","comment_text":"hi","notify_all":true}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/task/9hz/comment");
        assert_eq!(call.body.as_ref().unwrap()["comment_text"], "hi");
        assert_eq!(call.body.as_ref().unwrap()["notify_all"], true);
    }

    #[test]
    fn add_omits_notify_all_when_absent() {
        let call =
            build_call(r#"{"operation":"add","task_id":"9hz","comment_text":"hi"}"#).unwrap();
        assert!(call.body.as_ref().unwrap().get("notify_all").is_none());
    }

    #[test]
    fn list_requires_task_id() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("task_id"));
    }

    #[test]
    fn list_builds_get_with_task_path() {
        let call = build_call(r#"{"operation":"list","task_id":"9hz"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/task/9hz/comment");
    }

    #[test]
    fn update_requires_comment_id_and_comment_text() {
        assert!(build_call(r#"{"operation":"update","comment_text":"x"}"#).is_err());
        let err = build_call(r#"{"operation":"update","comment_id":"abc"}"#).unwrap_err();
        assert!(err.contains("comment_text"));
    }

    #[test]
    fn update_builds_put_with_comment_text_and_resolved() {
        let call = build_call(
            r#"{"operation":"update","comment_id":"abc","comment_text":"edited","resolved":true}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/comment/abc");
        assert_eq!(call.body.as_ref().unwrap()["comment_text"], "edited");
        assert_eq!(call.body.as_ref().unwrap()["resolved"], true);
    }

    #[test]
    fn normalize_add_extracts_id_user_comment_text_date() {
        let raw = br#"{"id":"cmt1","user":{"id":1,"username":"bob"},"comment_text":"hi","date":"1234567890"}"#;
        let out = normalize(CommentOp::Add, raw).unwrap();
        assert_eq!(out["id"], "cmt1");
        assert_eq!(out["user"]["username"], "bob");
        assert_eq!(out["comment_text"], "hi");
        assert_eq!(out["date"], "1234567890");
    }

    #[test]
    fn normalize_record_defensive_on_missing_fields() {
        let raw = br#"{"id":"cmt1"}"#;
        let out = normalize(CommentOp::Update, raw).unwrap();
        assert_eq!(out["user"], Value::Null);
        assert_eq!(out["comment_text"], Value::Null);
        assert_eq!(out["date"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_comments_array() {
        let raw = br#"{"comments":[{"id":"cmt1","user":{"username":"bob"},"comment_text":"hi","date":"1"}]}"#;
        let out = normalize(CommentOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "cmt1");
        assert_eq!(out["results"][0]["comment_text"], "hi");
    }

    #[test]
    fn normalize_list_handles_empty_comments() {
        let raw = br#"{"comments":[]}"#;
        let out = normalize(CommentOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"list","task_id":"9hz"}"#),
            Ok(CommentOp::List)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
