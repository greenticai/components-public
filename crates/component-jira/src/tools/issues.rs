//! `jira_issues` tool domain — pure HTTP-call building and response
//! normalization for Jira issue operations (create/search/get/update/
//! transition/assign/delete). No WIT imports — this module is fully
//! host-testable; the actual `extension-host/http` invocation and
//! `describe()` tool metadata live in `lib.rs` / `tool_meta.rs`.
//!
//! This is the template the other seven Jira tool domains copy: keep the
//! shape `IssueOp` (input enum) -> `build_call` (pure request builder) ->
//! `normalize` (pure response mapper), with no WIT/host types crossing the
//! boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Jira issue operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueOp {
    Create,
    Search,
    Get,
    Update,
    Transition,
    Assign,
    Delete,
}

/// Raw `jira_issues` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct IssuesInput {
    operation: IssueOp,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    jql: Option<String>,
    #[serde(default)]
    fields: Option<Value>,
    #[serde(default)]
    transition_id: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    max_results: Option<u32>,
    #[serde(default)]
    return_fields: Vec<String>,
}

/// Build the Jira REST v3 [`HttpCall`] for a `jira_issues` invocation.
///
/// Parses `args_json` into an [`IssuesInput`], validates the fields required
/// by the selected [`IssueOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the
/// field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: IssuesInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        IssueOp::Create => build_create(&input),
        IssueOp::Search => build_search(&input),
        IssueOp::Get => build_get(&input),
        IssueOp::Update => build_update(&input),
        IssueOp::Transition => build_transition(&input),
        IssueOp::Assign => build_assign(&input),
        IssueOp::Delete => build_delete(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<IssueOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: IssueOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &IssuesInput) -> Result<HttpCall, String> {
    let fields = input
        .fields
        .clone()
        .ok_or_else(|| "missing required field: fields".to_string())?;
    Ok(HttpCall {
        method: Method::Post,
        path: "/rest/api/3/issue".to_string(),
        query: Vec::new(),
        body: Some(json!({ "fields": fields })),
    })
}

fn build_search(input: &IssuesInput) -> Result<HttpCall, String> {
    let jql = super::require_field(input.jql.as_deref(), "jql")?.to_string();
    let mut query = vec![("jql".to_string(), jql)];
    if let Some(max_results) = input.max_results {
        query.push(("maxResults".to_string(), max_results.to_string()));
    }
    if !input.return_fields.is_empty() {
        query.push(("fields".to_string(), input.return_fields.join(",")));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: "/rest/api/3/search/jql".to_string(),
        query,
        body: None,
    })
}

fn build_get(input: &IssuesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let mut query = Vec::new();
    if !input.return_fields.is_empty() {
        query.push(("fields".to_string(), input.return_fields.join(",")));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/rest/api/3/issue/{id}"),
        query,
        body: None,
    })
}

fn build_update(input: &IssuesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let fields = input
        .fields
        .clone()
        .ok_or_else(|| "missing required field: fields".to_string())?;
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/rest/api/3/issue/{id}"),
        query: Vec::new(),
        body: Some(json!({ "fields": fields })),
    })
}

fn build_transition(input: &IssuesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let transition_id = super::require_field(input.transition_id.as_deref(), "transition_id")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/rest/api/3/issue/{id}/transitions"),
        query: Vec::new(),
        body: Some(json!({ "transition": { "id": transition_id } })),
    })
}

fn build_assign(input: &IssuesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let account_id = super::require_field(input.account_id.as_deref(), "account_id")?;
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/rest/api/3/issue/{id}/assignee"),
        query: Vec::new(),
        body: Some(json!({ "accountId": account_id })),
    })
}

fn build_delete(input: &IssuesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Delete,
        path: format!("/rest/api/3/issue/{id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Jira REST v3 response body to the compact shape returned to
/// the model, based on the [`IssueOp`] that produced it.
pub fn normalize(op: IssueOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        IssueOp::Search => normalize_search(raw),
        IssueOp::Create | IssueOp::Get | IssueOp::Update => normalize_record(raw),
        IssueOp::Delete | IssueOp::Assign | IssueOp::Transition => Ok(normalize_ack(raw)),
    }
}

fn fields_of(value: &Value) -> Option<&Value> {
    value.get("fields")
}

fn extract_summary(fields: Option<&Value>) -> Value {
    fields
        .and_then(|f| f.get("summary"))
        .cloned()
        .unwrap_or(Value::Null)
}

fn extract_status(fields: Option<&Value>) -> Value {
    fields
        .and_then(|f| f.get("status"))
        .and_then(|status| status.get("name"))
        .cloned()
        .unwrap_or(Value::Null)
}

fn extract_assignee(fields: Option<&Value>) -> Value {
    fields
        .and_then(|f| f.get("assignee"))
        .and_then(|assignee| assignee.get("displayName"))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Normalize a single-issue response (create/get/update) to
/// `{id,key,summary,status,assignee,url?}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid issue response: {err}"))?;
    let fields = fields_of(&value);

    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "key".to_string(),
        value.get("key").cloned().unwrap_or(Value::Null),
    );
    out.insert("summary".to_string(), extract_summary(fields));
    out.insert("status".to_string(), extract_status(fields));
    out.insert("assignee".to_string(), extract_assignee(fields));
    if let Some(url) = value.get("self").and_then(Value::as_str) {
        out.insert("url".to_string(), Value::String(url.to_string()));
    }
    Ok(Value::Object(out))
}

/// Normalize a search response to `{total, results:[{key,summary,status,assignee}]}`.
///
/// The `/rest/api/3/search/jql` endpoint (the classic `/rest/api/3/search`
/// endpoint is removed for Jira Cloud) does not return a top-level `total`
/// — it paginates via `nextPageToken` instead. `total` in the normalized
/// output is therefore always the count of mapped `results`, not a value
/// read from the response body.
fn normalize_search(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid search response: {err}"))?;
    let results: Vec<Value> = value
        .get("issues")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|issue| {
            let fields = fields_of(issue);
            json!({
                "key": issue.get("key").cloned().unwrap_or(Value::Null),
                "summary": extract_summary(fields),
                "status": extract_status(fields),
                "assignee": extract_assignee(fields),
            })
        })
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

/// Normalize a delete/assign/transition response — these Jira endpoints
/// return `204 No Content` on success, so `raw` is typically empty; `id` is
/// only recoverable if the (unusual) response body happens to echo it.
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
    fn create_builds_post_with_fields() {
        let call = build_call(r#"{"operation":"create","fields":{"project":{"key":"AB"},"issuetype":{"name":"Task"},"summary":"Hi"}}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/rest/api/3/issue");
        assert_eq!(call.body.as_ref().unwrap()["fields"]["summary"], "Hi");
    }

    #[test]
    fn search_builds_get_with_jql_query() {
        let call =
            build_call(r#"{"operation":"search","jql":"project = AB","max_results":5}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/api/3/search/jql");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "jql" && v == "project = AB")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "maxResults" && v == "5")
        );
    }

    #[test]
    fn transition_requires_id() {
        assert!(build_call(r#"{"operation":"transition","id":"AB-1"}"#).is_err());
        let ok =
            build_call(r#"{"operation":"transition","id":"AB-1","transition_id":"31"}"#).unwrap();
        assert_eq!(ok.path, "/rest/api/3/issue/AB-1/transitions");
    }

    #[test]
    fn update_missing_fields_names_field() {
        let err = build_call(r#"{"operation":"update","id":"AB-1"}"#).unwrap_err();
        assert!(err.contains("fields"));
    }

    #[test]
    fn update_builds_put_with_fields() {
        let call =
            build_call(r#"{"operation":"update","id":"AB-1","fields":{"summary":"Updated"}}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/rest/api/3/issue/AB-1");
        assert_eq!(call.body.as_ref().unwrap()["fields"]["summary"], "Updated");
    }

    #[test]
    fn delete_builds_delete() {
        let call = build_call(r#"{"operation":"delete","id":"AB-9"}"#).unwrap();
        assert!(matches!(call.method, Method::Delete));
        assert_eq!(call.path, "/rest/api/3/issue/AB-9");
    }

    #[test]
    fn normalize_get_extracts_key_and_summary() {
        let raw =
            br#"{"id":"1001","key":"AB-1","fields":{"summary":"Hi","status":{"name":"To Do"}}}"#;
        let out = normalize(IssueOp::Get, raw).unwrap();
        assert_eq!(out["key"], "AB-1");
        assert_eq!(out["summary"], "Hi");
        assert_eq!(out["status"], "To Do");
    }

    #[test]
    fn normalize_search_maps_issue_list() {
        // /rest/api/3/search/jql has no top-level `total`; the normalized
        // `total` is derived from the mapped `results` length instead.
        let raw =
            br#"{"issues":[{"key":"AB-1","fields":{"summary":"Hi","status":{"name":"Done"}}}]}"#;
        let out = normalize(IssueOp::Search, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["key"], "AB-1");
    }

    #[test]
    fn normalize_search_total_reflects_results_len_not_response_total() {
        // Even if the response body carries a `total` field (e.g. a stale
        // fixture or a future API change), the normalized `total` must
        // still be the mapped results count, not the response value.
        let raw = br#"{"total":999,"issues":[{"key":"AB-1"},{"key":"AB-2"}]}"#;
        let out = normalize(IssueOp::Search, raw).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn normalize_search_handles_empty_issues() {
        let raw = br#"{"issues":[]}"#;
        let out = normalize(IssueOp::Search, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn create_missing_fields_names_field() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("fields"));
    }

    #[test]
    fn search_missing_jql_names_field() {
        let err = build_call(r#"{"operation":"search"}"#).unwrap_err();
        assert!(err.contains("jql"));
    }

    #[test]
    fn assign_requires_account_id() {
        assert!(build_call(r#"{"operation":"assign","id":"AB-1"}"#).is_err());
        let call =
            build_call(r#"{"operation":"assign","id":"AB-1","account_id":"acc-1"}"#).unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/rest/api/3/issue/AB-1/assignee");
        assert_eq!(call.body.as_ref().unwrap()["accountId"], "acc-1");
    }

    #[test]
    fn get_builds_query_from_return_fields() {
        let call =
            build_call(r#"{"operation":"get","id":"AB-1","return_fields":["summary","status"]}"#)
                .unwrap();
        assert_eq!(call.path, "/rest/api/3/issue/AB-1");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "fields" && v == "summary,status")
        );
    }

    #[test]
    fn normalize_record_handles_missing_nested_fields() {
        let raw = br#"{"id":"1001","key":"AB-1","fields":{}}"#;
        let out = normalize(IssueOp::Create, raw).unwrap();
        assert_eq!(out["summary"], Value::Null);
        assert_eq!(out["status"], Value::Null);
        assert_eq!(out["assignee"], Value::Null);
        assert!(out.get("url").is_none());
    }

    #[test]
    fn normalize_delete_ack_handles_empty_body() {
        let out = normalize(IssueOp::Delete, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"delete","id":"AB-9"}"#),
            Ok(IssueOp::Delete)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
