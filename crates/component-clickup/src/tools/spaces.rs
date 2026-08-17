//! `clickup_spaces` tool domain — pure HTTP-call building and response
//! normalization for ClickUp space operations (list/get). No WIT imports —
//! this module is fully host-testable; the actual `extension-host/http`
//! invocation lives in `lib.rs`.
//!
//! Follows the `tools::tasks` / `component-jira-ext` `tools::projects`
//! template: `SpaceOp` (input enum) -> `build_call` (pure request builder)
//! -> `normalize` (pure response mapper).

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// ClickUp space operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceOp {
    List,
    Get,
}

/// Raw `clickup_spaces` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct SpacesInput {
    operation: SpaceOp,
    #[serde(default)]
    team_id: Option<String>,
    #[serde(default)]
    space_id: Option<String>,
}

/// Build the ClickUp API v2 [`HttpCall`] for a `clickup_spaces` invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: SpacesInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        SpaceOp::List => build_list(&input),
        SpaceOp::Get => build_get(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<SpaceOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: SpaceOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &SpacesInput) -> Result<HttpCall, String> {
    let team_id = super::require_field(input.team_id.as_deref(), "team_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/team/{team_id}/space"),
        query: Vec::new(),
        body: None,
    })
}

fn build_get(input: &SpacesInput) -> Result<HttpCall, String> {
    let space_id = super::require_field(input.space_id.as_deref(), "space_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/space/{space_id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw ClickUp API v2 response body to the compact shape returned to
/// the model, based on the [`SpaceOp`] that produced it.
pub fn normalize(op: SpaceOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        SpaceOp::List => normalize_list(raw),
        SpaceOp::Get => normalize_record(raw),
    }
}

/// Build the compact `{id,name,private?}` shape from a single parsed space
/// JSON value. Shared by [`normalize_record`] (single-space responses) and
/// [`normalize_list`] (each entry of a space page).
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
    if let Some(private) = value.get("private").cloned() {
        out.insert("private".to_string(), private);
    }
    Value::Object(out)
}

/// Normalize a single-space response (get) to `{id,name,private?}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid space response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/team/{team_id}/space` response to
/// `{total,results:[{id,name,private?}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid space list response: {err}"))?;
    let results: Vec<Value> = value
        .get("spaces")
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
    fn list_requires_team_id() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("team_id"));
    }

    #[test]
    fn list_builds_get_with_team_path() {
        let call = build_call(r#"{"operation":"list","team_id":"1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/team/1/space");
    }

    #[test]
    fn get_requires_space_id() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("space_id"));
    }

    #[test]
    fn get_builds_get_with_space_path() {
        let call = build_call(r#"{"operation":"get","space_id":"90"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/space/90");
    }

    #[test]
    fn normalize_get_extracts_id_name_private() {
        let raw = br#"{"id":"90","name":"Engineering","private":true}"#;
        let out = normalize(SpaceOp::Get, raw).unwrap();
        assert_eq!(out["id"], "90");
        assert_eq!(out["name"], "Engineering");
        assert_eq!(out["private"], true);
    }

    #[test]
    fn normalize_record_omits_private_when_absent() {
        let raw = br#"{"id":"90","name":"Engineering"}"#;
        let out = normalize(SpaceOp::Get, raw).unwrap();
        assert!(out.get("private").is_none());
    }

    #[test]
    fn normalize_list_maps_spaces_array() {
        let raw = br#"{"spaces":[{"id":"90","name":"Engineering","private":false}]}"#;
        let out = normalize(SpaceOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "90");
        assert_eq!(out["results"][0]["private"], false);
    }

    #[test]
    fn normalize_list_handles_empty_spaces() {
        let raw = br#"{"spaces":[]}"#;
        let out = normalize(SpaceOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","space_id":"90"}"#),
            Ok(SpaceOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
