//! `jira_sprints` tool domain — pure HTTP-call building and response
//! normalization for Jira Software agile sprint operations (list/get/
//! create/move_issues). No WIT imports — this module is fully
//! host-testable; the actual `extension-host/http` invocation lives in
//! `lib.rs`.
//!
//! Follows the `tools::issues` template: `SprintOp` (input enum) ->
//! `build_call` (pure request builder) -> `normalize` (pure response
//! mapper). Like `tools::boards`, these endpoints live under the Jira
//! Software Agile REST API (`/rest/agile/1.0`).

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Jira sprint operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SprintOp {
    List,
    Get,
    Create,
    MoveIssues,
}

/// Raw `jira_sprints` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct SprintsInput {
    operation: SprintOp,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    board_id: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    origin_board_id: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
    #[serde(default)]
    issues: Vec<String>,
}

/// Build the Jira Software Agile [`HttpCall`] for a `jira_sprints`
/// invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: SprintsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        SprintOp::List => build_list(&input),
        SprintOp::Get => build_get(&input),
        SprintOp::Create => build_create(&input),
        SprintOp::MoveIssues => build_move_issues(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<SprintOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: SprintOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &SprintsInput) -> Result<HttpCall, String> {
    let board_id = super::require_field(input.board_id.as_deref(), "board_id")?;
    let mut query = Vec::new();
    if let Some(state) = input.state.as_deref().filter(|s| !s.is_empty()) {
        query.push(("state".to_string(), state.to_string()));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/rest/agile/1.0/board/{board_id}/sprint"),
        query,
        body: None,
    })
}

fn build_get(input: &SprintsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/rest/agile/1.0/sprint/{id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_create(input: &SprintsInput) -> Result<HttpCall, String> {
    let name = super::require_field(input.name.as_deref(), "name")?;
    let origin_board_id =
        super::require_field(input.origin_board_id.as_deref(), "origin_board_id")?;
    let mut body = Map::new();
    body.insert("name".to_string(), json!(name));
    body.insert("originBoardId".to_string(), json!(origin_board_id));
    if let Some(start_date) = input.start_date.as_deref().filter(|s| !s.is_empty()) {
        body.insert("startDate".to_string(), json!(start_date));
    }
    if let Some(end_date) = input.end_date.as_deref().filter(|s| !s.is_empty()) {
        body.insert("endDate".to_string(), json!(end_date));
    }
    Ok(HttpCall {
        method: Method::Post,
        path: "/rest/agile/1.0/sprint".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_move_issues(input: &SprintsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    if input.issues.is_empty() {
        return Err("missing required field: issues".to_string());
    }
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/rest/agile/1.0/sprint/{id}/issue"),
        query: Vec::new(),
        body: Some(json!({ "issues": input.issues })),
    })
}

/// Map a raw Jira Software Agile response body to the compact shape
/// returned to the model, based on the [`SprintOp`] that produced it.
pub fn normalize(op: SprintOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        SprintOp::List => normalize_list(raw),
        SprintOp::Get | SprintOp::Create => normalize_record(raw),
        SprintOp::MoveIssues => Ok(normalize_move_ack(raw)),
    }
}

/// Build the compact `{id,name,state,startDate?,endDate?}` shape from a
/// single parsed sprint JSON value. Shared by `normalize_record`
/// (single-sprint responses) and `normalize_list` (each entry of a sprint
/// page).
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
        "state".to_string(),
        value.get("state").cloned().unwrap_or(Value::Null),
    );
    if let Some(start_date) = value.get("startDate") {
        out.insert("startDate".to_string(), start_date.clone());
    }
    if let Some(end_date) = value.get("endDate") {
        out.insert("endDate".to_string(), end_date.clone());
    }
    Value::Object(out)
}

/// Normalize a single-sprint response (get/create) to
/// `{id,name,state,startDate?,endDate?}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid sprint response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/board/{id}/sprint` list response to
/// `{total,results:[{id,name,state,startDate?,endDate?}]}`. The agile
/// endpoint nests results under `values`, like `jira_boards`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid sprint list response: {err}"))?;
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

/// Normalize a move-issues response — this Jira endpoint returns `204 No
/// Content` on success and never echoes the number of issues moved, so
/// `moved` is left `null` here; `lib.rs` backfills it by counting the
/// request's own `issues` array.
fn normalize_move_ack(_raw: &[u8]) -> Value {
    json!({ "ok": true, "moved": Value::Null })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Method;

    #[test]
    fn list_builds_get_with_state_query() {
        let call = build_call(r#"{"operation":"list","board_id":"1","state":"active"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/agile/1.0/board/1/sprint");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "state" && v == "active")
        );
    }

    #[test]
    fn list_missing_board_id_names_field() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("board_id"));
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"37"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/agile/1.0/sprint/37");
    }

    #[test]
    fn create_builds_post_with_name_and_origin_board_id() {
        let call = build_call(
            r#"{"operation":"create","name":"Sprint 5","origin_board_id":"1","start_date":"2026-07-01","end_date":"2026-07-14"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/rest/agile/1.0/sprint");
        assert_eq!(call.body.as_ref().unwrap()["name"], "Sprint 5");
        assert_eq!(call.body.as_ref().unwrap()["originBoardId"], "1");
        assert_eq!(call.body.as_ref().unwrap()["startDate"], "2026-07-01");
        assert_eq!(call.body.as_ref().unwrap()["endDate"], "2026-07-14");
    }

    #[test]
    fn create_missing_origin_board_id_names_field() {
        let err = build_call(r#"{"operation":"create","name":"Sprint 5"}"#).unwrap_err();
        assert!(err.contains("origin_board_id"));
    }

    #[test]
    fn create_omits_optional_dates_when_absent() {
        let call = build_call(r#"{"operation":"create","name":"Sprint 5","origin_board_id":"1"}"#)
            .unwrap();
        assert!(call.body.as_ref().unwrap().get("startDate").is_none());
        assert!(call.body.as_ref().unwrap().get("endDate").is_none());
    }

    #[test]
    fn move_issues_builds_post_with_issues_array() {
        let call = build_call(r#"{"operation":"move_issues","id":"37","issues":["AB-1","AB-2"]}"#)
            .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/rest/agile/1.0/sprint/37/issue");
        assert_eq!(call.body.as_ref().unwrap()["issues"][0], "AB-1");
        assert_eq!(call.body.as_ref().unwrap()["issues"][1], "AB-2");
    }

    #[test]
    fn move_issues_missing_issues_names_field() {
        let err = build_call(r#"{"operation":"move_issues","id":"37"}"#).unwrap_err();
        assert!(err.contains("issues"));
    }

    #[test]
    fn normalize_record_extracts_id_name_state_dates() {
        let raw =
            br#"{"id":37,"name":"Sprint 5","state":"active","startDate":"2026-07-01","endDate":"2026-07-14"}"#;
        let out = normalize(SprintOp::Get, raw).unwrap();
        assert_eq!(out["id"], 37);
        assert_eq!(out["name"], "Sprint 5");
        assert_eq!(out["state"], "active");
        assert_eq!(out["startDate"], "2026-07-01");
        assert_eq!(out["endDate"], "2026-07-14");
    }

    #[test]
    fn normalize_record_omits_dates_when_absent() {
        let raw = br#"{"id":37,"name":"Sprint 5","state":"future"}"#;
        let out = normalize(SprintOp::Create, raw).unwrap();
        assert!(out.get("startDate").is_none());
        assert!(out.get("endDate").is_none());
    }

    #[test]
    fn normalize_list_maps_values_array() {
        let raw = br#"{"total":1,"values":[{"id":37,"name":"Sprint 5","state":"active"}]}"#;
        let out = normalize(SprintOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["name"], "Sprint 5");
    }

    #[test]
    fn normalize_move_issues_ack_handles_empty_body() {
        let out = normalize(SprintOp::MoveIssues, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["moved"], Value::Null);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"move_issues","id":"37","issues":["AB-1"]}"#),
            Ok(SprintOp::MoveIssues)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
