//! `clickup_lists` tool domain — pure HTTP-call building and response
//! normalization for ClickUp list operations (list/get/create). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation lives in `lib.rs`.
//!
//! Follows the `tools::folders` template: `ListOp` (input enum) ->
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

/// ClickUp list operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListOp {
    List,
    Get,
    Create,
}

/// Raw `clickup_lists` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct ListsInput {
    operation: ListOp,
    #[serde(default)]
    folder_id: Option<String>,
    #[serde(default)]
    list_id: Option<String>,
    #[serde(default)]
    fields: Option<Value>,
}

/// Build the ClickUp API v2 [`HttpCall`] for a `clickup_lists` invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: ListsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        ListOp::List => build_list(&input),
        ListOp::Get => build_get(&input),
        ListOp::Create => build_create(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
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
    let folder_id = super::require_field(input.folder_id.as_deref(), "folder_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/folder/{folder_id}/list"),
        query: Vec::new(),
        body: None,
    })
}

fn build_get(input: &ListsInput) -> Result<HttpCall, String> {
    let list_id = super::require_field(input.list_id.as_deref(), "list_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/list/{list_id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_create(input: &ListsInput) -> Result<HttpCall, String> {
    let folder_id = super::require_field(input.folder_id.as_deref(), "folder_id")?;
    let fields = input
        .fields
        .clone()
        .ok_or_else(|| "missing required field: fields".to_string())?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/folder/{folder_id}/list"),
        query: Vec::new(),
        body: Some(fields),
    })
}

/// Map a raw ClickUp API v2 response body to the compact shape returned to
/// the model, based on the [`ListOp`] that produced it.
pub fn normalize(op: ListOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        ListOp::List => normalize_list(raw),
        ListOp::Get | ListOp::Create => normalize_record(raw),
    }
}

/// Build the compact `{id,name,folder_id?}` shape from a single parsed list
/// JSON value. Shared by [`normalize_record`] (single-list responses) and
/// [`normalize_list`] (each entry of a list page).
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "name".to_string(),
        value.get("name").cloned().unwrap_or(Value::Null),
    );
    if let Some(folder_id) = value
        .get("folder")
        .and_then(|folder| folder.get("id"))
        .cloned()
    {
        out.insert("folder_id".to_string(), folder_id);
    }
    Value::Object(out)
}

/// Normalize a single-list response (get/create) to `{id,name,folder_id?}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid list response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/folder/{folder_id}/list` response to
/// `{total,results:[{id,name,folder_id?}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid list-of-lists response: {err}"))?;
    let results: Vec<Value> = value
        .get("lists")
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
    fn list_requires_folder_id() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("folder_id"));
    }

    #[test]
    fn list_builds_get_with_folder_path() {
        let call = build_call(r#"{"operation":"list","folder_id":"457"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/folder/457/list");
    }

    #[test]
    fn get_requires_list_id() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("list_id"));
    }

    #[test]
    fn get_builds_get_with_list_path() {
        let call = build_call(r#"{"operation":"get","list_id":"124"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/list/124");
    }

    #[test]
    fn create_requires_folder_id_and_fields() {
        assert!(build_call(r#"{"operation":"create","fields":{"name":"Backlog"}}"#).is_err());
        let err = build_call(r#"{"operation":"create","folder_id":"457"}"#).unwrap_err();
        assert!(err.contains("fields"));
    }

    #[test]
    fn create_builds_post_with_fields_body() {
        let call =
            build_call(r#"{"operation":"create","folder_id":"457","fields":{"name":"Backlog"}}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/folder/457/list");
        assert_eq!(call.body.as_ref().unwrap()["name"], "Backlog");
    }

    #[test]
    fn normalize_get_extracts_id_name_folder_id() {
        let raw = br#"{"id":"124","name":"Backlog","folder":{"id":"457"}}"#;
        let out = normalize(ListOp::Get, raw).unwrap();
        assert_eq!(out["id"], "124");
        assert_eq!(out["name"], "Backlog");
        assert_eq!(out["folder_id"], "457");
    }

    #[test]
    fn normalize_record_omits_folder_id_when_absent() {
        let raw = br#"{"id":"124","name":"Backlog"}"#;
        let out = normalize(ListOp::Create, raw).unwrap();
        assert!(out.get("folder_id").is_none());
    }

    #[test]
    fn normalize_list_maps_lists_array() {
        let raw = br#"{"lists":[{"id":"124","name":"Backlog","folder":{"id":"457"}}]}"#;
        let out = normalize(ListOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "124");
        assert_eq!(out["results"][0]["folder_id"], "457");
    }

    #[test]
    fn normalize_list_handles_empty_lists() {
        let raw = br#"{"lists":[]}"#;
        let out = normalize(ListOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","list_id":"124"}"#),
            Ok(ListOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
