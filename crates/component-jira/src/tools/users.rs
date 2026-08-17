//! `jira_users` tool domain — pure HTTP-call building and response
//! normalization for Jira user search. No WIT imports — this module is
//! fully host-testable; the actual `extension-host/http` invocation lives
//! in `lib.rs`.
//!
//! Follows the `tools::issues` template: `UserOp` (input enum) ->
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

/// Jira user operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserOp {
    Search,
}

/// Raw `jira_users` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct UsersInput {
    operation: UserOp,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    max_results: Option<u32>,
}

/// Build the Jira REST v3 [`HttpCall`] for a `jira_users` invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: UsersInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        UserOp::Search => build_search(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<UserOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: UserOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_search(input: &UsersInput) -> Result<HttpCall, String> {
    let query = super::require_field(input.query.as_deref(), "query")?;
    let mut params = vec![("query".to_string(), query.to_string())];
    if let Some(max_results) = input.max_results {
        params.push(("maxResults".to_string(), max_results.to_string()));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: "/rest/api/3/user/search".to_string(),
        query: params,
        body: None,
    })
}

/// Map a raw Jira REST v3 response body to the compact shape returned to
/// the model, based on the [`UserOp`] that produced it.
pub fn normalize(op: UserOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        UserOp::Search => normalize_search(raw),
    }
}

/// Build the compact `{accountId,displayName,emailAddress?,active}` shape
/// from a single parsed user JSON value. `emailAddress` may be absent or
/// null on some Jira accounts, so it falls back to `Value::Null` rather
/// than being omitted or panicking.
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "accountId".to_string(),
        value.get("accountId").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "displayName".to_string(),
        value.get("displayName").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "emailAddress".to_string(),
        value.get("emailAddress").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "active".to_string(),
        value.get("active").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a `/user/search` response — a bare JSON array, not wrapped in
/// an object — to `{total,results:[{accountId,displayName,emailAddress?,active}]}`.
fn normalize_search(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid user search response: {err}"))?;
    let users: Vec<&Value> = value.as_array().into_iter().flatten().collect();
    let results: Vec<Value> = users.iter().map(|item| record_of(item)).collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_builds_get_with_query() {
        let call = build_call(r#"{"operation":"search","query":"jane"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/api/3/user/search");
        assert!(call.query.iter().any(|(k, v)| k == "query" && v == "jane"));
        assert!(!call.query.iter().any(|(k, _)| k == "maxResults"));
    }

    #[test]
    fn search_includes_optional_max_results() {
        let call = build_call(r#"{"operation":"search","query":"jane","max_results":10}"#).unwrap();
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "maxResults" && v == "10")
        );
    }

    #[test]
    fn search_missing_query_names_field() {
        let err = build_call(r#"{"operation":"search"}"#).unwrap_err();
        assert!(err.contains("query"));
    }

    #[test]
    fn normalize_search_maps_bare_array() {
        let raw = br#"[{"accountId":"acc-1","displayName":"Jane Doe","emailAddress":"jane@example.com","active":true}]"#;
        let out = normalize(UserOp::Search, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["accountId"], "acc-1");
        assert_eq!(out["results"][0]["displayName"], "Jane Doe");
        assert_eq!(out["results"][0]["emailAddress"], "jane@example.com");
        assert_eq!(out["results"][0]["active"], true);
    }

    #[test]
    fn normalize_search_handles_missing_email_address() {
        let raw = br#"[{"accountId":"acc-1","displayName":"Jane Doe","active":true}]"#;
        let out = normalize(UserOp::Search, raw).unwrap();
        assert_eq!(out["results"][0]["emailAddress"], Value::Null);
    }

    #[test]
    fn normalize_search_handles_empty_array() {
        let out = normalize(UserOp::Search, b"[]").unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"search","query":"jane"}"#),
            Ok(UserOp::Search)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
