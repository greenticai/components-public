//! `trello_comments` tool domain — pure HTTP-call building and response
//! normalization for Trello card-comment operations (add/list). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext` `tools::issues` template: `CommentOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Trello comment operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentOp {
    Add,
    List,
}

/// Raw `trello_comments` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct CommentsInput {
    operation: CommentOp,
    #[serde(default)]
    card_id: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

/// Build the Trello REST v1 [`HttpCall`] for a `trello_comments`
/// invocation.
///
/// Parses `args_json` into a [`CommentsInput`], validates the fields
/// required by the selected [`CommentOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: CommentsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        CommentOp::Add => build_add(&input),
        CommentOp::List => build_list(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
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
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    let text = super::require_field(input.text.as_deref(), "text")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/cards/{card_id}/actions/comments"),
        query: Vec::new(),
        body: Some(json!({ "text": text })),
    })
}

fn build_list(input: &CommentsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/cards/{card_id}/actions"),
        query: vec![("filter".to_string(), "commentCard".to_string())],
        body: None,
    })
}

/// Map a raw Trello REST v1 response body to the compact shape returned to
/// the model, based on the [`CommentOp`] that produced it.
pub fn normalize(op: CommentOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        CommentOp::Add => normalize_record(raw),
        CommentOp::List => normalize_list(raw),
    }
}

/// A Trello comment "action" object carries its text nested at
/// `data.text`, not at the top level.
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    let text = value
        .get("data")
        .and_then(|data| data.get("text"))
        .cloned()
        .unwrap_or(Value::Null);
    out.insert("text".to_string(), text);
    out.insert(
        "memberCreator".to_string(),
        value.get("memberCreator").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "date".to_string(),
        value.get("date").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a single comment-action response (add) to
/// `{id,text,memberCreator,date}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid comment response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/cards/{card_id}/actions` bare-array response to
/// `{total,results:[{id,text,memberCreator,date}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid comment-list response: {err}"))?;
    let results: Vec<Value> = value
        .as_array()
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
    fn add_requires_card_id_and_text() {
        assert!(build_call(r#"{"operation":"add","text":"hi"}"#).is_err());
        assert!(build_call(r#"{"operation":"add","card_id":"C1"}"#).is_err());
    }

    #[test]
    fn add_builds_post_with_text_body() {
        let call = build_call(r#"{"operation":"add","card_id":"C1","text":"looks good"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/cards/C1/actions/comments");
        assert_eq!(call.body.as_ref().unwrap()["text"], "looks good");
    }

    #[test]
    fn list_requires_card_id() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("card_id"));
    }

    #[test]
    fn list_builds_get_with_comment_card_filter() {
        let call = build_call(r#"{"operation":"list","card_id":"C1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/cards/C1/actions");
        assert_eq!(
            call.query,
            vec![("filter".to_string(), "commentCard".to_string())]
        );
        assert!(call.body.is_none());
    }

    #[test]
    fn normalize_add_extracts_text_from_nested_data() {
        let raw = br#"{"id":"A1","data":{"text":"looks good"},"memberCreator":{"id":"M1"},"date":"2026-01-01T00:00:00.000Z"}"#;
        let out = normalize(CommentOp::Add, raw).unwrap();
        assert_eq!(out["id"], "A1");
        assert_eq!(out["text"], "looks good");
        assert_eq!(out["memberCreator"]["id"], "M1");
        assert_eq!(out["date"], "2026-01-01T00:00:00.000Z");
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"A1"}"#;
        let out = normalize(CommentOp::Add, raw).unwrap();
        assert_eq!(out["text"], Value::Null);
        assert_eq!(out["memberCreator"], Value::Null);
        assert_eq!(out["date"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_bare_array() {
        let raw = br#"[{"id":"A1","data":{"text":"first"}},{"id":"A2","data":{"text":"second"}}]"#;
        let out = normalize(CommentOp::List, raw).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["text"], "first");
        assert_eq!(out["results"][1]["text"], "second");
    }

    #[test]
    fn normalize_list_handles_empty_array() {
        let out = normalize(CommentOp::List, b"[]").unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_rejects_invalid_json() {
        assert!(normalize(CommentOp::List, b"not json").is_err());
        assert!(normalize(CommentOp::Add, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"list","card_id":"C1"}"#),
            Ok(CommentOp::List)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
