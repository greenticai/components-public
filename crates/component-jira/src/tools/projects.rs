//! `jira_projects` tool domain — pure HTTP-call building and response
//! normalization for Jira project operations (list/get). No WIT imports —
//! this module is fully host-testable; the actual `extension-host/http`
//! invocation lives in `lib.rs`.
//!
//! Follows the `tools::issues` template: `ProjectOp` (input enum) ->
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

/// Jira project operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectOp {
    List,
    Get,
}

/// Raw `jira_projects` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct ProjectsInput {
    operation: ProjectOp,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    max_results: Option<u32>,
}

/// Build the Jira REST v3 [`HttpCall`] for a `jira_projects` invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: ProjectsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        ProjectOp::List => Ok(build_list(&input)),
        ProjectOp::Get => build_get(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<ProjectOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: ProjectOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &ProjectsInput) -> HttpCall {
    let mut query = Vec::new();
    if let Some(q) = input.query.as_deref().filter(|q| !q.is_empty()) {
        query.push(("query".to_string(), q.to_string()));
    }
    if let Some(max_results) = input.max_results {
        query.push(("maxResults".to_string(), max_results.to_string()));
    }
    HttpCall {
        method: Method::Get,
        path: "/rest/api/3/project/search".to_string(),
        query,
        body: None,
    }
}

fn build_get(input: &ProjectsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/rest/api/3/project/{id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Jira REST v3 response body to the compact shape returned to
/// the model, based on the [`ProjectOp`] that produced it.
pub fn normalize(op: ProjectOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        ProjectOp::List => normalize_list(raw),
        ProjectOp::Get => normalize_record(raw),
    }
}

fn extract_lead(value: &Value) -> Option<Value> {
    value
        .get("lead")
        .and_then(|lead| lead.get("displayName"))
        .cloned()
}

/// Build the compact `{id,key,name,lead?}` shape from a single parsed
/// project JSON value. Shared by `normalize_record` (single-project
/// responses) and `normalize_list` (each entry of a project page).
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "key".to_string(),
        value.get("key").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "name".to_string(),
        value.get("name").cloned().unwrap_or(Value::Null),
    );
    if let Some(lead) = extract_lead(value) {
        out.insert("lead".to_string(), lead);
    }
    Value::Object(out)
}

/// Normalize a single-project response (get) to `{id,key,name,lead?}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid project response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/project/search` response to
/// `{total,results:[{id,key,name,lead?}]}`. The search endpoint nests
/// results under `values`, not `issues`/`comments` like other domains.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid project search response: {err}"))?;
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
    fn list_builds_get_with_query_and_max_results() {
        let call = build_call(r#"{"operation":"list","query":"acme","max_results":10}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/api/3/project/search");
        assert!(call.query.iter().any(|(k, v)| k == "query" && v == "acme"));
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "maxResults" && v == "10")
        );
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(call.query.is_empty());
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"AB"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/api/3/project/AB");
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_id_key_name_lead() {
        let raw = br#"{"id":"10000","key":"AB","name":"Acme Board","lead":{"displayName":"Jane"}}"#;
        let out = normalize(ProjectOp::Get, raw).unwrap();
        assert_eq!(out["id"], "10000");
        assert_eq!(out["key"], "AB");
        assert_eq!(out["name"], "Acme Board");
        assert_eq!(out["lead"], "Jane");
    }

    #[test]
    fn normalize_record_omits_lead_when_absent() {
        let raw = br#"{"id":"10000","key":"AB","name":"Acme Board"}"#;
        let out = normalize(ProjectOp::Get, raw).unwrap();
        assert!(out.get("lead").is_none());
    }

    #[test]
    fn normalize_list_maps_values_array() {
        let raw = br#"{"total":1,"values":[{"id":"10000","key":"AB","name":"Acme Board"}]}"#;
        let out = normalize(ProjectOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["key"], "AB");
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","id":"AB"}"#),
            Ok(ProjectOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
