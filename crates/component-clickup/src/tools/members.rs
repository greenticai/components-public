//! `clickup_members` tool domain — pure HTTP-call building and response
//! normalization for ClickUp list-member lookup. No WIT imports — this
//! module is fully host-testable; the actual `extension-host/http`
//! invocation lives in `lib.rs`.
//!
//! Follows the `tools::spaces` template: `MemberOp` (input enum) ->
//! `build_call` (pure request builder) -> `normalize` (pure response
//! mapper). Currently a single read-only operation (`list`), kept as an enum
//! rather than a bare function for symmetry with every other `tools::*`
//! domain and to leave room for future member operations.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// ClickUp member operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberOp {
    List,
}

/// Raw `clickup_members` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct MembersInput {
    operation: MemberOp,
    #[serde(default)]
    list_id: Option<String>,
}

/// Build the ClickUp API v2 [`HttpCall`] for a `clickup_members`
/// invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: MembersInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        MemberOp::List => build_list(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<MemberOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: MemberOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &MembersInput) -> Result<HttpCall, String> {
    let list_id = super::require_field(input.list_id.as_deref(), "list_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/list/{list_id}/member"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw ClickUp API v2 response body to the compact shape returned to
/// the model, based on the [`MemberOp`] that produced it.
pub fn normalize(op: MemberOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        MemberOp::List => normalize_list(raw),
    }
}

/// Build the compact `{id,username,email?}` shape from a single parsed
/// member JSON value.
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "username".to_string(),
        value.get("username").cloned().unwrap_or(Value::Null),
    );
    if let Some(email) = value.get("email").cloned() {
        out.insert("email".to_string(), email);
    }
    Value::Object(out)
}

/// Normalize a `/list/{list_id}/member` response to
/// `{total,results:[{id,username,email?}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid member list response: {err}"))?;
    let results: Vec<Value> = value
        .get("members")
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
    fn list_requires_list_id() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("list_id"));
    }

    #[test]
    fn list_builds_get_with_list_path() {
        let call = build_call(r#"{"operation":"list","list_id":"124"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/list/124/member");
    }

    #[test]
    fn normalize_list_maps_members_array_with_email() {
        let raw = br#"{"members":[{"id":1,"username":"bob","email":"bob@example.com"}]}"#;
        let out = normalize(MemberOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], 1);
        assert_eq!(out["results"][0]["username"], "bob");
        assert_eq!(out["results"][0]["email"], "bob@example.com");
    }

    #[test]
    fn normalize_list_omits_email_when_absent() {
        let raw = br#"{"members":[{"id":1,"username":"bob"}]}"#;
        let out = normalize(MemberOp::List, raw).unwrap();
        assert!(out["results"][0].get("email").is_none());
    }

    #[test]
    fn normalize_list_handles_empty_members() {
        let raw = br#"{"members":[]}"#;
        let out = normalize(MemberOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"list","list_id":"124"}"#),
            Ok(MemberOp::List)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
