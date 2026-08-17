//! `clickup_tasks` tool domain — pure HTTP-call building and response
//! normalization for ClickUp task operations (create/get/update/delete/
//! search). No WIT imports — this module is fully host-testable; the
//! actual `extension-host/http` invocation and `describe()` tool metadata
//! live in `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext` `tools::issues` template: `TaskOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// ClickUp task operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOp {
    Create,
    Get,
    Update,
    Delete,
    Search,
}

/// Raw `clickup_tasks` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct TasksInput {
    operation: TaskOp,
    #[serde(default)]
    list_id: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    fields: Option<Value>,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    statuses: Option<Vec<String>>,
    #[serde(default)]
    include_closed: Option<bool>,
}

/// Build the ClickUp API v2 [`HttpCall`] for a `clickup_tasks` invocation.
///
/// Parses `args_json` into a [`TasksInput`], validates the fields required
/// by the selected [`TaskOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the
/// field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: TasksInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        TaskOp::Create => build_create(&input),
        TaskOp::Get => build_get(&input),
        TaskOp::Update => build_update(&input),
        TaskOp::Delete => build_delete(&input),
        TaskOp::Search => build_search(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<TaskOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: TaskOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &TasksInput) -> Result<HttpCall, String> {
    let list_id = super::require_field(input.list_id.as_deref(), "list_id")?;
    let fields = input
        .fields
        .clone()
        .ok_or_else(|| "missing required field: fields".to_string())?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/list/{list_id}/task"),
        query: Vec::new(),
        body: Some(fields),
    })
}

fn build_get(input: &TasksInput) -> Result<HttpCall, String> {
    let task_id = super::require_field(input.task_id.as_deref(), "task_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/task/{task_id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_update(input: &TasksInput) -> Result<HttpCall, String> {
    let task_id = super::require_field(input.task_id.as_deref(), "task_id")?;
    let fields = input
        .fields
        .clone()
        .ok_or_else(|| "missing required field: fields".to_string())?;
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/task/{task_id}"),
        query: Vec::new(),
        body: Some(fields),
    })
}

fn build_delete(input: &TasksInput) -> Result<HttpCall, String> {
    let task_id = super::require_field(input.task_id.as_deref(), "task_id")?;
    Ok(HttpCall {
        method: Method::Delete,
        path: format!("/task/{task_id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_search(input: &TasksInput) -> Result<HttpCall, String> {
    let list_id = super::require_field(input.list_id.as_deref(), "list_id")?;
    let mut query = Vec::new();
    if let Some(page) = input.page {
        query.push(("page".to_string(), page.to_string()));
    }
    if let Some(statuses) = input.statuses.as_ref().filter(|s| !s.is_empty()) {
        for status in statuses {
            query.push(("statuses[]".to_string(), status.clone()));
        }
    }
    if let Some(include_closed) = input.include_closed {
        query.push(("include_closed".to_string(), include_closed.to_string()));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/list/{list_id}/task"),
        query,
        body: None,
    })
}

/// Map a raw ClickUp API v2 response body to the compact shape returned to
/// the model, based on the [`TaskOp`] that produced it.
pub fn normalize(op: TaskOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        TaskOp::Search => normalize_search(raw),
        TaskOp::Create | TaskOp::Get | TaskOp::Update => normalize_record(raw),
        TaskOp::Delete => Ok(normalize_ack(raw)),
    }
}

fn extract_status(value: &Value) -> Value {
    value
        .get("status")
        .and_then(|status| status.get("status"))
        .cloned()
        .unwrap_or(Value::Null)
}

fn extract_list_id(value: &Value) -> Option<Value> {
    value.get("list").and_then(|list| list.get("id")).cloned()
}

/// Build the compact `{id,name,status}` shape from a single parsed task
/// JSON value, shared by [`normalize_record`] (single-task responses) and
/// [`normalize_search`] (each entry of a task page).
fn short_record_of(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "name": value.get("name").cloned().unwrap_or(Value::Null),
        "status": extract_status(value),
    })
}

/// Normalize a single-task response (create/get/update) to
/// `{id,name,status,url,list_id?}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid task response: {err}"))?;

    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "name".to_string(),
        value.get("name").cloned().unwrap_or(Value::Null),
    );
    out.insert("status".to_string(), extract_status(&value));
    out.insert(
        "url".to_string(),
        value.get("url").cloned().unwrap_or(Value::Null),
    );
    if let Some(list_id) = extract_list_id(&value) {
        out.insert("list_id".to_string(), list_id);
    }
    Ok(Value::Object(out))
}

/// Normalize a `/list/{list_id}/task` search response to
/// `{total,results:[{id,name,status}]}`.
fn normalize_search(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid task search response: {err}"))?;
    let results: Vec<Value> = value
        .get("tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(short_record_of)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

/// Normalize a delete response. ClickUp's delete endpoint returns `200 {}`
/// (or an empty body), so `id` is only recoverable if the (unusual)
/// response body happens to echo it; `lib.rs` backfills `id` from the
/// request's own `task_id` field when this comes back null.
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
    fn create_requires_list_id() {
        let err = build_call(r#"{"operation":"create","fields":{"name":"Task"}}"#).unwrap_err();
        assert!(err.contains("list_id"));
    }

    #[test]
    fn create_requires_fields() {
        let err = build_call(r#"{"operation":"create","list_id":"1"}"#).unwrap_err();
        assert!(err.contains("fields"));
    }

    #[test]
    fn create_builds_post_with_fields_body() {
        let call =
            build_call(r#"{"operation":"create","list_id":"1","fields":{"name":"Ship it"}}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/list/1/task");
        assert_eq!(call.body.as_ref().unwrap()["name"], "Ship it");
    }

    #[test]
    fn get_requires_task_id() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("task_id"));
    }

    #[test]
    fn get_builds_get_with_task_path() {
        let call = build_call(r#"{"operation":"get","task_id":"abc"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/task/abc");
    }

    #[test]
    fn update_requires_task_id_and_fields() {
        assert!(build_call(r#"{"operation":"update","fields":{"name":"X"}}"#).is_err());
        let err = build_call(r#"{"operation":"update","task_id":"abc"}"#).unwrap_err();
        assert!(err.contains("fields"));
    }

    #[test]
    fn update_builds_put_with_fields_body() {
        let call = build_call(
            r#"{"operation":"update","task_id":"abc","fields":{"status":"in progress"}}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/task/abc");
        assert_eq!(call.body.as_ref().unwrap()["status"], "in progress");
    }

    #[test]
    fn delete_builds_delete() {
        let call = build_call(r#"{"operation":"delete","task_id":"abc"}"#).unwrap();
        assert!(matches!(call.method, Method::Delete));
        assert_eq!(call.path, "/task/abc");
    }

    #[test]
    fn delete_requires_task_id() {
        let err = build_call(r#"{"operation":"delete"}"#).unwrap_err();
        assert!(err.contains("task_id"));
    }

    #[test]
    fn search_requires_list_id() {
        let err = build_call(r#"{"operation":"search"}"#).unwrap_err();
        assert!(err.contains("list_id"));
    }

    #[test]
    fn search_builds_get_with_query() {
        let call = build_call(
            r#"{"operation":"search","list_id":"1","page":2,"statuses":["open","closed"],"include_closed":true}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/list/1/task");
        assert!(call.query.iter().any(|(k, v)| k == "page" && v == "2"));
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "statuses[]" && v == "open")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "statuses[]" && v == "closed")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "include_closed" && v == "true")
        );
    }

    #[test]
    fn normalize_get_extracts_id_name_status() {
        let raw = br#"{"id":"9hz","name":"Ship it","status":{"status":"in progress"},"url":"https://app.clickup.com/t/9hz"}"#;
        let out = normalize(TaskOp::Get, raw).unwrap();
        assert_eq!(out["id"], "9hz");
        assert_eq!(out["name"], "Ship it");
        assert_eq!(out["status"], "in progress");
        assert_eq!(out["url"], "https://app.clickup.com/t/9hz");
    }

    #[test]
    fn normalize_record_omits_list_id_when_absent() {
        let raw = br#"{"id":"9hz","name":"Ship it"}"#;
        let out = normalize(TaskOp::Create, raw).unwrap();
        assert!(out.get("list_id").is_none());
        assert_eq!(out["status"], Value::Null);
    }

    #[test]
    fn normalize_record_includes_list_id_when_present() {
        let raw = br#"{"id":"9hz","name":"Ship it","list":{"id":"123"}}"#;
        let out = normalize(TaskOp::Get, raw).unwrap();
        assert_eq!(out["list_id"], "123");
    }

    #[test]
    fn normalize_search_maps_tasks_array() {
        let raw = br#"{"tasks":[{"id":"9hz","name":"Ship it","status":{"status":"open"}}]}"#;
        let out = normalize(TaskOp::Search, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "9hz");
        assert_eq!(out["results"][0]["status"], "open");
    }

    #[test]
    fn normalize_search_handles_empty_tasks() {
        let raw = br#"{"tasks":[]}"#;
        let out = normalize(TaskOp::Search, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_delete_ack_handles_empty_body() {
        let out = normalize(TaskOp::Delete, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"delete","task_id":"abc"}"#),
            Ok(TaskOp::Delete)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
