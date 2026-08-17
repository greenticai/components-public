//! `clickup_custom_fields` tool domain — pure HTTP-call building and
//! response normalization for ClickUp custom field operations (get/set). No
//! WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation lives in `lib.rs`.
//!
//! Follows the `tools::tasks` template: `CustomFieldOp` (input enum) ->
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

/// ClickUp custom field operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomFieldOp {
    Get,
    Set,
}

/// Raw `clickup_custom_fields` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct CustomFieldsInput {
    operation: CustomFieldOp,
    #[serde(default)]
    list_id: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    field_id: Option<String>,
    #[serde(default)]
    value: Option<Value>,
}

/// Build the ClickUp API v2 [`HttpCall`] for a `clickup_custom_fields`
/// invocation.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: CustomFieldsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        CustomFieldOp::Get => build_get(&input),
        CustomFieldOp::Set => build_set(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<CustomFieldOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: CustomFieldOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_get(input: &CustomFieldsInput) -> Result<HttpCall, String> {
    let list_id = super::require_field(input.list_id.as_deref(), "list_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/list/{list_id}/field"),
        query: Vec::new(),
        body: None,
    })
}

fn build_set(input: &CustomFieldsInput) -> Result<HttpCall, String> {
    let task_id = super::require_field(input.task_id.as_deref(), "task_id")?;
    let field_id = super::require_field(input.field_id.as_deref(), "field_id")?;
    let value = input
        .value
        .clone()
        .ok_or_else(|| "missing required field: value".to_string())?;
    let mut body = Map::new();
    body.insert("value".to_string(), value);
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/task/{task_id}/field/{field_id}"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

/// Map a raw ClickUp API v2 response body to the compact shape returned to
/// the model, based on the [`CustomFieldOp`] that produced it.
pub fn normalize(op: CustomFieldOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        CustomFieldOp::Get => normalize_get(raw),
        CustomFieldOp::Set => Ok(normalize_ack(raw)),
    }
}

/// Build the compact `{id,name,type}` shape from a single parsed custom
/// field JSON value.
fn record_of(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "name": value.get("name").cloned().unwrap_or(Value::Null),
        "type": value.get("type").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a `/list/{list_id}/field` response to
/// `{total,results:[{id,name,type}]}`.
fn normalize_get(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid custom field response: {err}"))?;
    let results: Vec<Value> = value
        .get("fields")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(record_of)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

/// Normalize a `set` response. ClickUp's set-field endpoint returns the
/// updated task, not the field id, so `id` here is only recoverable if the
/// (unusual) response body happens to echo it; `lib.rs` backfills `id` from
/// the request's own `field_id` field when this comes back null.
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
    fn get_requires_list_id() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("list_id"));
    }

    #[test]
    fn get_builds_get_with_list_path() {
        let call = build_call(r#"{"operation":"get","list_id":"124"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/list/124/field");
    }

    #[test]
    fn set_requires_task_id_field_id_and_value() {
        assert!(build_call(r#"{"operation":"set","field_id":"f1","value":"x"}"#).is_err());
        let err = build_call(r#"{"operation":"set","task_id":"9hz","value":"x"}"#).unwrap_err();
        assert!(err.contains("field_id"));
        let err = build_call(r#"{"operation":"set","task_id":"9hz","field_id":"f1"}"#).unwrap_err();
        assert!(err.contains("value"));
    }

    #[test]
    fn set_builds_post_with_value_body() {
        let call =
            build_call(r#"{"operation":"set","task_id":"9hz","field_id":"f1","value":"high"}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/task/9hz/field/f1");
        assert_eq!(call.body.as_ref().unwrap()["value"], "high");
    }

    #[test]
    fn set_accepts_non_string_value() {
        let call = build_call(r#"{"operation":"set","task_id":"9hz","field_id":"f1","value":42}"#)
            .unwrap();
        assert_eq!(call.body.as_ref().unwrap()["value"], 42);
    }

    #[test]
    fn normalize_get_maps_fields_array() {
        let raw = br#"{"fields":[{"id":"f1","name":"Priority","type":"drop_down"}]}"#;
        let out = normalize(CustomFieldOp::Get, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "f1");
        assert_eq!(out["results"][0]["name"], "Priority");
        assert_eq!(out["results"][0]["type"], "drop_down");
    }

    #[test]
    fn normalize_get_handles_empty_fields() {
        let raw = br#"{"fields":[]}"#;
        let out = normalize(CustomFieldOp::Get, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_set_ack_handles_empty_body() {
        let out = normalize(CustomFieldOp::Set, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","list_id":"124"}"#),
            Ok(CustomFieldOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
