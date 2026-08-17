//! `jira_comments` tool domain — pure HTTP-call building and response
//! normalization for Jira issue-comment operations (add/list/update/
//! delete). No WIT imports — this module is fully host-testable; the
//! actual `extension-host/http` invocation lives in `lib.rs`.
//!
//! Follows the `tools::issues` template: `CommentOp` (input enum) ->
//! `build_call` (pure request builder) -> `normalize` (pure response
//! mapper).
//!
//! Comment body handling: Jira REST v3 comment bodies are normally
//! Atlassian Document Format (ADF) objects, but the API also accepts a
//! plain string in some contexts. Rather than build an ADF encoder, this
//! module passes the caller-supplied `body` value straight through as the
//! `body` field of the request (mirroring how `tools::issues` passes
//! `fields` through unmodified) — the caller may supply either a plain
//! string or a full ADF object, and Jira's own body shape comes back
//! unmodified from `normalize`.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Jira comment operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentOp {
    Add,
    List,
    Update,
    Delete,
}

/// Raw `jira_comments` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct CommentsInput {
    operation: CommentOp,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    comment_id: Option<String>,
    #[serde(default)]
    body: Option<Value>,
}

/// Build the Jira REST v3 [`HttpCall`] for a `jira_comments` invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: CommentsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        CommentOp::Add => build_add(&input),
        CommentOp::List => build_list(&input),
        CommentOp::Update => build_update(&input),
        CommentOp::Delete => build_delete(&input),
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
    let id = super::require_field(input.id.as_deref(), "id")?;
    let body = input
        .body
        .clone()
        .ok_or_else(|| "missing required field: body".to_string())?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/rest/api/3/issue/{id}/comment"),
        query: Vec::new(),
        body: Some(json!({ "body": body })),
    })
}

fn build_list(input: &CommentsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/rest/api/3/issue/{id}/comment"),
        query: Vec::new(),
        body: None,
    })
}

fn build_update(input: &CommentsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let comment_id = super::require_field(input.comment_id.as_deref(), "comment_id")?;
    let body = input
        .body
        .clone()
        .ok_or_else(|| "missing required field: body".to_string())?;
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/rest/api/3/issue/{id}/comment/{comment_id}"),
        query: Vec::new(),
        body: Some(json!({ "body": body })),
    })
}

fn build_delete(input: &CommentsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let comment_id = super::require_field(input.comment_id.as_deref(), "comment_id")?;
    Ok(HttpCall {
        method: Method::Delete,
        path: format!("/rest/api/3/issue/{id}/comment/{comment_id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Jira REST v3 response body to the compact shape returned to
/// the model, based on the [`CommentOp`] that produced it.
pub fn normalize(op: CommentOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        CommentOp::Add | CommentOp::Update => normalize_record(raw),
        CommentOp::List => normalize_list(raw),
        CommentOp::Delete => Ok(normalize_ack(raw)),
    }
}

fn extract_author(value: &Value) -> Value {
    value
        .get("author")
        .and_then(|author| author.get("displayName"))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Build the compact `{id,author,body,created}` shape from a single parsed
/// comment JSON value. Shared by `normalize_record` (single-comment
/// responses) and `normalize_list` (each entry of a comment page).
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert("author".to_string(), extract_author(value));
    out.insert(
        "body".to_string(),
        value.get("body").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "created".to_string(),
        value.get("created").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a single-comment response (add/update) to
/// `{id,author,body,created}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid comment response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a comment-page response to `{total,results:[{id,author,body,created}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid comment list response: {err}"))?;
    let total = value.get("total").cloned().unwrap_or(Value::Null);
    let results: Vec<Value> = value
        .get("comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(record_of)
        .collect();
    Ok(json!({ "total": total, "results": results }))
}

/// Normalize a delete response — this Jira endpoint returns `204 No
/// Content` on success, so `raw` is typically empty; `id` is only
/// recoverable if the (unusual) response body happens to echo it. `lib.rs`
/// backfills it from the request's `comment_id` when null.
fn normalize_ack(raw: &[u8]) -> Value {
    let id = if raw.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice::<Value>(raw)
            .ok()
            .and_then(|value| value.get("id").cloned())
            .unwrap_or(Value::Null)
    };
    json!({ "ok": true, "id": id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Method;

    #[test]
    fn add_builds_post_with_body() {
        let call =
            build_call(r#"{"operation":"add","id":"AB-1","body":"Looks good to me"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/rest/api/3/issue/AB-1/comment");
        assert_eq!(call.body.as_ref().unwrap()["body"], "Looks good to me");
    }

    #[test]
    fn add_missing_body_names_field() {
        let err = build_call(r#"{"operation":"add","id":"AB-1"}"#).unwrap_err();
        assert!(err.contains("body"));
    }

    #[test]
    fn add_missing_id_names_field() {
        let err = build_call(r#"{"operation":"add","body":"hi"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn add_accepts_adf_object_body() {
        let call = build_call(
            r#"{"operation":"add","id":"AB-1","body":{"type":"doc","version":1,"content":[]}}"#,
        )
        .unwrap();
        assert_eq!(call.body.as_ref().unwrap()["body"]["type"], "doc");
    }

    #[test]
    fn list_builds_get() {
        let call = build_call(r#"{"operation":"list","id":"AB-1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/api/3/issue/AB-1/comment");
        assert!(call.body.is_none());
    }

    #[test]
    fn update_builds_put_with_body() {
        let call =
            build_call(r#"{"operation":"update","id":"AB-1","comment_id":"10","body":"edited"}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/rest/api/3/issue/AB-1/comment/10");
        assert_eq!(call.body.as_ref().unwrap()["body"], "edited");
    }

    #[test]
    fn update_missing_comment_id_names_field() {
        let err = build_call(r#"{"operation":"update","id":"AB-1","body":"edited"}"#).unwrap_err();
        assert!(err.contains("comment_id"));
    }

    #[test]
    fn delete_builds_delete() {
        let call = build_call(r#"{"operation":"delete","id":"AB-1","comment_id":"10"}"#).unwrap();
        assert!(matches!(call.method, Method::Delete));
        assert_eq!(call.path, "/rest/api/3/issue/AB-1/comment/10");
    }

    #[test]
    fn delete_missing_comment_id_names_field() {
        let err = build_call(r#"{"operation":"delete","id":"AB-1"}"#).unwrap_err();
        assert!(err.contains("comment_id"));
    }

    #[test]
    fn normalize_record_extracts_author_body_created() {
        let raw = br#"{"id":"10","author":{"displayName":"Jane"},"body":"hi","created":"2026-01-01T00:00:00.000+0000"}"#;
        let out = normalize(CommentOp::Add, raw).unwrap();
        assert_eq!(out["id"], "10");
        assert_eq!(out["author"], "Jane");
        assert_eq!(out["body"], "hi");
        assert_eq!(out["created"], "2026-01-01T00:00:00.000+0000");
    }

    #[test]
    fn normalize_record_handles_missing_nested_fields() {
        let raw = br#"{"id":"10"}"#;
        let out = normalize(CommentOp::Add, raw).unwrap();
        assert_eq!(out["author"], Value::Null);
        assert_eq!(out["body"], Value::Null);
        assert_eq!(out["created"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_comment_array() {
        let raw = br#"{"total":2,"comments":[{"id":"10","author":{"displayName":"Jane"},"body":"hi","created":"t1"},{"id":"11","author":{"displayName":"Bob"},"body":"yo","created":"t2"}]}"#;
        let out = normalize(CommentOp::List, raw).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["id"], "10");
        assert_eq!(out["results"][1]["author"], "Bob");
    }

    #[test]
    fn normalize_delete_ack_handles_empty_body() {
        let out = normalize(CommentOp::Delete, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"delete","id":"AB-1","comment_id":"10"}"#),
            Ok(CommentOp::Delete)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
