//! `trello_lists` tool domain — pure HTTP-call building and response
//! normalization for Trello list operations (list/create/update/archive).
//! No WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext` `tools::issues` template: `ListOp`
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

/// Trello list operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListOp {
    List,
    Create,
    Update,
    Archive,
}

/// Raw `trello_lists` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct ListsInput {
    operation: ListOp,
    #[serde(default)]
    board_id: Option<String>,
    #[serde(default)]
    list_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    closed: Option<bool>,
}

/// Build the Trello REST v1 [`HttpCall`] for a `trello_lists` invocation.
///
/// Parses `args_json` into a [`ListsInput`], validates the fields required
/// by the selected [`ListOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the
/// field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: ListsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        ListOp::List => build_list(&input),
        ListOp::Create => build_create(&input),
        ListOp::Update => build_update(&input),
        ListOp::Archive => build_archive(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<ListOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: ListOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &ListsInput) -> Result<HttpCall, String> {
    let board_id = super::require_field(input.board_id.as_deref(), "board_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/boards/{board_id}/lists"),
        query: Vec::new(),
        body: None,
    })
}

fn build_create(input: &ListsInput) -> Result<HttpCall, String> {
    let board_id = super::require_field(input.board_id.as_deref(), "board_id")?;
    let mut body = Map::new();
    body.insert("idBoard".to_string(), json!(board_id));
    if let Some(name) = &input.name {
        body.insert("name".to_string(), json!(name));
    }
    Ok(HttpCall {
        method: Method::Post,
        path: "/lists".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_update(input: &ListsInput) -> Result<HttpCall, String> {
    let list_id = super::require_field(input.list_id.as_deref(), "list_id")?;
    let mut body = Map::new();
    if let Some(name) = &input.name {
        body.insert("name".to_string(), json!(name));
    }
    if let Some(closed) = input.closed {
        body.insert("closed".to_string(), json!(closed));
    }
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/lists/{list_id}"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_archive(input: &ListsInput) -> Result<HttpCall, String> {
    let list_id = super::require_field(input.list_id.as_deref(), "list_id")?;
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/lists/{list_id}/closed"),
        query: Vec::new(),
        body: Some(json!({ "value": true })),
    })
}

/// Map a raw Trello REST v1 response body to the compact shape returned to
/// the model, based on the [`ListOp`] that produced it.
pub fn normalize(op: ListOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        ListOp::List => normalize_list(raw),
        ListOp::Create | ListOp::Update => normalize_record(raw),
        ListOp::Archive => Ok(normalize_ack(raw)),
    }
}

fn record_of(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "name": value.get("name").cloned().unwrap_or(Value::Null),
        "idBoard": value.get("idBoard").cloned().unwrap_or(Value::Null),
        "closed": value.get("closed").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-list response (create/update) to
/// `{id,name,idBoard,closed}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid list response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/boards/{board_id}/lists` bare-array response to
/// `{total,results:[{id,name,idBoard,closed}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid list-of-lists response: {err}"))?;
    let results: Vec<Value> = value
        .as_array()
        .into_iter()
        .flatten()
        .map(record_of)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

/// Normalize an archive response. Trello's `/lists/{id}/closed` endpoint
/// replies with the full updated list, but this domain normalizes it to a
/// uniform ack shape; `lib.rs` backfills a null `id` from the request's own
/// `list_id` field.
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
    fn list_requires_board_id() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("board_id"));
    }

    #[test]
    fn list_builds_get_boards_lists_path() {
        let call = build_call(r#"{"operation":"list","board_id":"B1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/boards/B1/lists");
        assert!(call.body.is_none());
    }

    #[test]
    fn create_requires_board_id() {
        let err = build_call(r#"{"operation":"create","name":"Backlog"}"#).unwrap_err();
        assert!(err.contains("board_id"));
    }

    #[test]
    fn create_builds_post_with_id_board_and_name() {
        let call =
            build_call(r#"{"operation":"create","board_id":"B1","name":"Backlog"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/lists");
        let body = call.body.as_ref().unwrap();
        assert_eq!(body["idBoard"], "B1");
        assert_eq!(body["name"], "Backlog");
    }

    #[test]
    fn update_requires_list_id() {
        let err = build_call(r#"{"operation":"update","name":"New"}"#).unwrap_err();
        assert!(err.contains("list_id"));
    }

    #[test]
    fn update_builds_put_with_present_fields_only() {
        let call = build_call(r#"{"operation":"update","list_id":"L1","closed":true}"#).unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/lists/L1");
        let body = call.body.as_ref().unwrap();
        assert_eq!(body["closed"], true);
        assert!(body.get("name").is_none());
    }

    #[test]
    fn archive_requires_list_id() {
        let err = build_call(r#"{"operation":"archive"}"#).unwrap_err();
        assert!(err.contains("list_id"));
    }

    #[test]
    fn archive_builds_closed_path_with_value_true_body() {
        let call = build_call(r#"{"operation":"archive","list_id":"L1"}"#).unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/lists/L1/closed");
        assert_eq!(call.body.as_ref().unwrap()["value"], true);
    }

    #[test]
    fn normalize_create_extracts_record_fields() {
        let raw = br#"{"id":"L1","name":"Backlog","idBoard":"B1","closed":false}"#;
        let out = normalize(ListOp::Create, raw).unwrap();
        assert_eq!(out["id"], "L1");
        assert_eq!(out["name"], "Backlog");
        assert_eq!(out["idBoard"], "B1");
        assert_eq!(out["closed"], false);
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"L1"}"#;
        let out = normalize(ListOp::Update, raw).unwrap();
        assert_eq!(out["name"], Value::Null);
        assert_eq!(out["idBoard"], Value::Null);
        assert_eq!(out["closed"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_bare_array() {
        let raw = br#"[{"id":"L1","name":"Backlog","idBoard":"B1","closed":false},{"id":"L2","name":"Done","idBoard":"B1","closed":false}]"#;
        let out = normalize(ListOp::List, raw).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["id"], "L1");
        assert_eq!(out["results"][1]["id"], "L2");
    }

    #[test]
    fn normalize_list_handles_empty_array() {
        let out = normalize(ListOp::List, b"[]").unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_archive_ack_extracts_id_from_full_list_response() {
        let raw = br#"{"id":"L1","name":"Backlog","closed":true}"#;
        let out = normalize(ListOp::Archive, raw).unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], "L1");
    }

    #[test]
    fn normalize_archive_ack_handles_empty_body() {
        let out = normalize(ListOp::Archive, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn normalize_rejects_invalid_json() {
        assert!(normalize(ListOp::List, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"archive","list_id":"L1"}"#),
            Ok(ListOp::Archive)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
