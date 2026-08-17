//! `jira_boards` tool domain — pure HTTP-call building and response
//! normalization for Jira Software agile board operations (list/get). No
//! WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation lives in `lib.rs`.
//!
//! Follows the `tools::issues` template: `BoardOp` (input enum) ->
//! `build_call` (pure request builder) -> `normalize` (pure response
//! mapper). Unlike `tools::issues`/`tools::projects`, these endpoints live
//! under the Jira Software Agile REST API (`/rest/agile/1.0`), not the
//! platform REST v3 API.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Jira board operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardOp {
    List,
    Get,
}

/// Raw `jira_boards` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct BoardsInput {
    operation: BoardOp,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    project_key_or_id: Option<String>,
    #[serde(default)]
    max_results: Option<u32>,
}

/// Build the Jira Software Agile [`HttpCall`] for a `jira_boards`
/// invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: BoardsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        BoardOp::List => Ok(build_list(&input)),
        BoardOp::Get => build_get(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<BoardOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: BoardOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &BoardsInput) -> HttpCall {
    let mut query = Vec::new();
    if let Some(project) = input.project_key_or_id.as_deref().filter(|p| !p.is_empty()) {
        query.push(("projectKeyOrId".to_string(), project.to_string()));
    }
    if let Some(max_results) = input.max_results {
        query.push(("maxResults".to_string(), max_results.to_string()));
    }
    HttpCall {
        method: Method::Get,
        path: "/rest/agile/1.0/board".to_string(),
        query,
        body: None,
    }
}

fn build_get(input: &BoardsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/rest/agile/1.0/board/{id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Jira Software Agile response body to the compact shape
/// returned to the model, based on the [`BoardOp`] that produced it.
pub fn normalize(op: BoardOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        BoardOp::List => normalize_list(raw),
        BoardOp::Get => normalize_record(raw),
    }
}

fn extract_project_key(value: &Value) -> Option<Value> {
    value
        .get("location")
        .and_then(|location| location.get("projectKey"))
        .cloned()
}

/// Build the compact `{id,name,type,projectKey?}` shape from a single
/// parsed board JSON value. Shared by `normalize_record` (single-board
/// responses) and `normalize_list` (each entry of a board page).
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
    out.insert(
        "type".to_string(),
        value.get("type").cloned().unwrap_or(Value::Null),
    );
    if let Some(project_key) = extract_project_key(value) {
        out.insert("projectKey".to_string(), project_key);
    }
    Value::Object(out)
}

/// Normalize a single-board response (get) to `{id,name,type,projectKey?}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid board response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/board` list response to
/// `{total,results:[{id,name,type,projectKey?}]}`. The agile endpoint
/// nests results under `values`, like `jira_projects` search.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid board list response: {err}"))?;
    let total = value.get("total").cloned().unwrap_or(Value::Null);
    let results: Vec<Value> = value
        .get("values")
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
    fn list_builds_get_with_project_and_max_results() {
        let call = build_call(r#"{"operation":"list","project_key_or_id":"AB","max_results":25}"#)
            .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/agile/1.0/board");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "projectKeyOrId" && v == "AB")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "maxResults" && v == "25")
        );
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(call.query.is_empty());
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/agile/1.0/board/1");
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_id_name_type_project_key() {
        let raw =
            br#"{"id":1,"name":"Sprint Board","type":"scrum","location":{"projectKey":"AB"}}"#;
        let out = normalize(BoardOp::Get, raw).unwrap();
        assert_eq!(out["id"], 1);
        assert_eq!(out["name"], "Sprint Board");
        assert_eq!(out["type"], "scrum");
        assert_eq!(out["projectKey"], "AB");
    }

    #[test]
    fn normalize_record_omits_project_key_when_absent() {
        let raw = br#"{"id":1,"name":"Sprint Board","type":"scrum"}"#;
        let out = normalize(BoardOp::Get, raw).unwrap();
        assert!(out.get("projectKey").is_none());
    }

    #[test]
    fn normalize_list_maps_values_array() {
        let raw = br#"{"total":1,"values":[{"id":1,"name":"Sprint Board","type":"scrum"}]}"#;
        let out = normalize(BoardOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["name"], "Sprint Board");
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","id":"1"}"#),
            Ok(BoardOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
