//! `trello_boards` tool domain — pure HTTP-call building and response
//! normalization for Trello board operations (list/get/create). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext` `tools::issues` template: `BoardOp`
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

/// Trello board operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardOp {
    List,
    Get,
    Create,
}

/// Raw `trello_boards` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct BoardsInput {
    operation: BoardOp,
    #[serde(default)]
    board_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// Build the Trello REST v1 [`HttpCall`] for a `trello_boards` invocation.
///
/// Parses `args_json` into a [`BoardsInput`], validates the fields required
/// by the selected [`BoardOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the
/// field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: BoardsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        BoardOp::List => Ok(build_list(&input)),
        BoardOp::Get => build_get(&input),
        BoardOp::Create => Ok(build_create(&input)),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<BoardOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: BoardOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(_input: &BoardsInput) -> HttpCall {
    HttpCall {
        method: Method::Get,
        path: "/members/me/boards".to_string(),
        query: Vec::new(),
        body: None,
    }
}

fn build_get(input: &BoardsInput) -> Result<HttpCall, String> {
    let board_id = super::require_field(input.board_id.as_deref(), "board_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/boards/{board_id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_create(input: &BoardsInput) -> HttpCall {
    let mut body = Map::new();
    if let Some(name) = &input.name {
        body.insert("name".to_string(), json!(name));
    }
    HttpCall {
        method: Method::Post,
        path: "/boards".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    }
}

/// Map a raw Trello REST v1 response body to the compact shape returned to
/// the model, based on the [`BoardOp`] that produced it.
pub fn normalize(op: BoardOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        BoardOp::List => normalize_list(raw),
        BoardOp::Get | BoardOp::Create => normalize_record(raw),
    }
}

fn record_of(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "name": value.get("name").cloned().unwrap_or(Value::Null),
        "url": value.get("url").cloned().unwrap_or(Value::Null),
        "closed": value.get("closed").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-board response (get/create) to
/// `{id,name,url,closed}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid board response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/members/me/boards` bare-array response to
/// `{total,results:[{id,name,url,closed}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid board-list response: {err}"))?;
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
    fn list_builds_get_members_me_boards() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/members/me/boards");
        assert!(call.body.is_none());
    }

    #[test]
    fn get_requires_board_id() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("board_id"));
    }

    #[test]
    fn get_builds_get_with_board_path() {
        let call = build_call(r#"{"operation":"get","board_id":"B1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/boards/B1");
    }

    #[test]
    fn create_builds_post_with_name() {
        let call = build_call(r#"{"operation":"create","name":"Roadmap"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/boards");
        assert_eq!(call.body.as_ref().unwrap()["name"], "Roadmap");
    }

    #[test]
    fn create_without_name_still_builds_a_call() {
        let call = build_call(r#"{"operation":"create"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert!(call.body.as_ref().unwrap().get("name").is_none());
    }

    #[test]
    fn normalize_get_extracts_record_fields() {
        let raw =
            br#"{"id":"B1","name":"Roadmap","url":"https://trello.com/b/xyz","closed":false}"#;
        let out = normalize(BoardOp::Get, raw).unwrap();
        assert_eq!(out["id"], "B1");
        assert_eq!(out["name"], "Roadmap");
        assert_eq!(out["url"], "https://trello.com/b/xyz");
        assert_eq!(out["closed"], false);
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"B1"}"#;
        let out = normalize(BoardOp::Create, raw).unwrap();
        assert_eq!(out["name"], Value::Null);
        assert_eq!(out["url"], Value::Null);
        assert_eq!(out["closed"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_bare_array() {
        let raw = br#"[{"id":"B1","name":"Roadmap","closed":false},{"id":"B2","name":"Ops","closed":true}]"#;
        let out = normalize(BoardOp::List, raw).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["id"], "B1");
        assert_eq!(out["results"][1]["closed"], true);
    }

    #[test]
    fn normalize_list_handles_empty_array() {
        let out = normalize(BoardOp::List, b"[]").unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_rejects_invalid_json() {
        assert!(normalize(BoardOp::Get, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","board_id":"B1"}"#),
            Ok(BoardOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
