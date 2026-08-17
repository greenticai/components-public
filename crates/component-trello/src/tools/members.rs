//! `trello_members` tool domain — pure HTTP-call building and response
//! normalization for Trello member operations (search/assign). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext` `tools::issues` template: `MemberOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::{HttpCall, Method};

/// Trello member operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberOp {
    Search,
    Assign,
}

/// Raw `trello_members` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct MembersInput {
    operation: MemberOp,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    card_id: Option<String>,
    #[serde(default)]
    member_id: Option<String>,
}

/// Build the Trello REST v1 [`HttpCall`] for a `trello_members`
/// invocation.
///
/// Parses `args_json` into a [`MembersInput`], validates the fields
/// required by the selected [`MemberOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: MembersInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        MemberOp::Search => build_search(&input),
        MemberOp::Assign => build_assign(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<MemberOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: MemberOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_search(input: &MembersInput) -> Result<HttpCall, String> {
    let query = super::require_field(input.query.as_deref(), "query")?;
    Ok(HttpCall {
        method: Method::Get,
        path: "/search/members".to_string(),
        query: vec![("query".to_string(), query.to_string())],
        body: None,
    })
}

fn build_assign(input: &MembersInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    let member_id = super::require_field(input.member_id.as_deref(), "member_id")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/cards/{card_id}/idMembers"),
        query: Vec::new(),
        body: Some(json!({ "value": member_id })),
    })
}

/// Map a raw Trello REST v1 response body to the compact shape returned to
/// the model, based on the [`MemberOp`] that produced it.
pub fn normalize(op: MemberOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        MemberOp::Search => normalize_search(raw),
        MemberOp::Assign => Ok(normalize_ack()),
    }
}

fn record_of(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "username": value.get("username").cloned().unwrap_or(Value::Null),
        "fullName": value.get("fullName").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a `/search/members` bare-array response to
/// `{total,results:[{id,username,fullName}]}`.
fn normalize_search(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid member-search response: {err}"))?;
    let results: Vec<Value> = value
        .as_array()
        .into_iter()
        .flatten()
        .map(record_of)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

/// Build an `assign` ack. Trello's `POST /cards/{id}/idMembers` endpoint
/// replies with the card's full `idMembers` array, not a single object
/// with a stable `id` field, so this domain always acks with a null `id`;
/// `lib.rs` backfills it from the request's own `card_id` field — the id
/// the spec calls for on this op.
fn normalize_ack() -> Value {
    json!({ "ok": true, "id": Value::Null })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Method;

    #[test]
    fn search_requires_query() {
        let err = build_call(r#"{"operation":"search"}"#).unwrap_err();
        assert!(err.contains("query"));
    }

    #[test]
    fn search_builds_get_with_query_param() {
        let call = build_call(r#"{"operation":"search","query":"ada"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/search/members");
        assert_eq!(call.query, vec![("query".to_string(), "ada".to_string())]);
        assert!(call.body.is_none());
    }

    #[test]
    fn assign_requires_card_id_and_member_id() {
        assert!(build_call(r#"{"operation":"assign","member_id":"M1"}"#).is_err());
        assert!(build_call(r#"{"operation":"assign","card_id":"C1"}"#).is_err());
    }

    #[test]
    fn assign_builds_post_with_value_body() {
        let call = build_call(r#"{"operation":"assign","card_id":"C1","member_id":"M1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/cards/C1/idMembers");
        assert_eq!(call.body.as_ref().unwrap()["value"], "M1");
    }

    #[test]
    fn normalize_search_maps_bare_array() {
        let raw =
            br#"[{"id":"M1","username":"ada","fullName":"Ada Lovelace"},{"id":"M2","username":"al"}]"#;
        let out = normalize(MemberOp::Search, raw).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["id"], "M1");
        assert_eq!(out["results"][0]["username"], "ada");
        assert_eq!(out["results"][0]["fullName"], "Ada Lovelace");
        assert_eq!(out["results"][1]["fullName"], Value::Null);
    }

    #[test]
    fn normalize_search_handles_empty_array() {
        let out = normalize(MemberOp::Search, b"[]").unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_search_rejects_invalid_json() {
        assert!(normalize(MemberOp::Search, b"not json").is_err());
    }

    #[test]
    fn normalize_assign_ack_with_null_id() {
        let out = normalize(MemberOp::Assign, br#"["M1","M2"]"#).unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn normalize_assign_ack_handles_empty_body() {
        let out = normalize(MemberOp::Assign, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"assign","card_id":"C1","member_id":"M1"}"#),
            Ok(MemberOp::Assign)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
