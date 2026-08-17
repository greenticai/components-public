//! `clickup_folders` tool domain — pure HTTP-call building and response
//! normalization for ClickUp folder operations (list/get/create). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation lives in `lib.rs`.
//!
//! Follows the `tools::spaces` template: `FolderOp` (input enum) ->
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

/// ClickUp folder operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderOp {
    List,
    Get,
    Create,
}

/// Raw `clickup_folders` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct FoldersInput {
    operation: FolderOp,
    #[serde(default)]
    space_id: Option<String>,
    #[serde(default)]
    folder_id: Option<String>,
    #[serde(default)]
    fields: Option<Value>,
}

/// Build the ClickUp API v2 [`HttpCall`] for a `clickup_folders` invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: FoldersInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        FolderOp::List => build_list(&input),
        FolderOp::Get => build_get(&input),
        FolderOp::Create => build_create(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<FolderOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: FolderOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &FoldersInput) -> Result<HttpCall, String> {
    let space_id = super::require_field(input.space_id.as_deref(), "space_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/space/{space_id}/folder"),
        query: Vec::new(),
        body: None,
    })
}

fn build_get(input: &FoldersInput) -> Result<HttpCall, String> {
    let folder_id = super::require_field(input.folder_id.as_deref(), "folder_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/folder/{folder_id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_create(input: &FoldersInput) -> Result<HttpCall, String> {
    let space_id = super::require_field(input.space_id.as_deref(), "space_id")?;
    let fields = input
        .fields
        .clone()
        .ok_or_else(|| "missing required field: fields".to_string())?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/space/{space_id}/folder"),
        query: Vec::new(),
        body: Some(fields),
    })
}

/// Map a raw ClickUp API v2 response body to the compact shape returned to
/// the model, based on the [`FolderOp`] that produced it.
pub fn normalize(op: FolderOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        FolderOp::List => normalize_list(raw),
        FolderOp::Get | FolderOp::Create => normalize_record(raw),
    }
}

/// Build the compact `{id,name,space_id?}` shape from a single parsed
/// folder JSON value. Shared by [`normalize_record`] (single-folder
/// responses) and [`normalize_list`] (each entry of a folder page).
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
    if let Some(space_id) = value
        .get("space")
        .and_then(|space| space.get("id"))
        .cloned()
    {
        out.insert("space_id".to_string(), space_id);
    }
    Value::Object(out)
}

/// Normalize a single-folder response (get/create) to
/// `{id,name,space_id?}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid folder response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/space/{space_id}/folder` response to
/// `{total,results:[{id,name,space_id?}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid folder list response: {err}"))?;
    let results: Vec<Value> = value
        .get("folders")
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
    fn list_requires_space_id() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("space_id"));
    }

    #[test]
    fn list_builds_get_with_space_path() {
        let call = build_call(r#"{"operation":"list","space_id":"90"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/space/90/folder");
    }

    #[test]
    fn get_requires_folder_id() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("folder_id"));
    }

    #[test]
    fn get_builds_get_with_folder_path() {
        let call = build_call(r#"{"operation":"get","folder_id":"457"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/folder/457");
    }

    #[test]
    fn create_requires_space_id_and_fields() {
        assert!(build_call(r#"{"operation":"create","fields":{"name":"Sprint"}}"#).is_err());
        let err = build_call(r#"{"operation":"create","space_id":"90"}"#).unwrap_err();
        assert!(err.contains("fields"));
    }

    #[test]
    fn create_builds_post_with_fields_body() {
        let call =
            build_call(r#"{"operation":"create","space_id":"90","fields":{"name":"Sprint"}}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/space/90/folder");
        assert_eq!(call.body.as_ref().unwrap()["name"], "Sprint");
    }

    #[test]
    fn normalize_get_extracts_id_name_space_id() {
        let raw = br#"{"id":"457","name":"Sprint","space":{"id":"90"}}"#;
        let out = normalize(FolderOp::Get, raw).unwrap();
        assert_eq!(out["id"], "457");
        assert_eq!(out["name"], "Sprint");
        assert_eq!(out["space_id"], "90");
    }

    #[test]
    fn normalize_record_omits_space_id_when_absent() {
        let raw = br#"{"id":"457","name":"Sprint"}"#;
        let out = normalize(FolderOp::Create, raw).unwrap();
        assert!(out.get("space_id").is_none());
    }

    #[test]
    fn normalize_list_maps_folders_array() {
        let raw = br#"{"folders":[{"id":"457","name":"Sprint","space":{"id":"90"}}]}"#;
        let out = normalize(FolderOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "457");
        assert_eq!(out["results"][0]["space_id"], "90");
    }

    #[test]
    fn normalize_list_handles_empty_folders() {
        let raw = br#"{"folders":[]}"#;
        let out = normalize(FolderOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","folder_id":"457"}"#),
            Ok(FolderOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
