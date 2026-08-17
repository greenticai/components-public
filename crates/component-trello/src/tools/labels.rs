//! `trello_labels` tool domain — pure HTTP-call building and response
//! normalization for Trello label operations (list/add/remove). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext` `tools::issues` template: `LabelOp`
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

/// Trello label operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelOp {
    List,
    Add,
    Remove,
}

/// Raw `trello_labels` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct LabelsInput {
    operation: LabelOp,
    #[serde(default)]
    board_id: Option<String>,
    #[serde(default)]
    card_id: Option<String>,
    #[serde(default)]
    label_id: Option<String>,
}

/// Build the Trello REST v1 [`HttpCall`] for a `trello_labels` invocation.
///
/// Parses `args_json` into a [`LabelsInput`], validates the fields required
/// by the selected [`LabelOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the
/// field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: LabelsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        LabelOp::List => build_list(&input),
        LabelOp::Add => build_add(&input),
        LabelOp::Remove => build_remove(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<LabelOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: LabelOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &LabelsInput) -> Result<HttpCall, String> {
    let board_id = super::require_field(input.board_id.as_deref(), "board_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/boards/{board_id}/labels"),
        query: Vec::new(),
        body: None,
    })
}

fn build_add(input: &LabelsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    let label_id = super::require_field(input.label_id.as_deref(), "label_id")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/cards/{card_id}/idLabels"),
        query: Vec::new(),
        body: Some(json!({ "value": label_id })),
    })
}

fn build_remove(input: &LabelsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    let label_id = super::require_field(input.label_id.as_deref(), "label_id")?;
    Ok(HttpCall {
        method: Method::Delete,
        path: format!("/cards/{card_id}/idLabels/{label_id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Trello REST v1 response body to the compact shape returned to
/// the model, based on the [`LabelOp`] that produced it.
pub fn normalize(op: LabelOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        LabelOp::List => normalize_list(raw),
        LabelOp::Add | LabelOp::Remove => Ok(normalize_ack()),
    }
}

fn record_of(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "name": value.get("name").cloned().unwrap_or(Value::Null),
        "color": value.get("color").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a `/boards/{board_id}/labels` bare-array response to
/// `{total,results:[{id,name,color}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid label-list response: {err}"))?;
    let results: Vec<Value> = value
        .as_array()
        .into_iter()
        .flatten()
        .map(record_of)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

/// Build an add/remove ack. Trello's `idLabels` add/remove endpoints don't
/// reply with a stable single-object shape the model would find useful (add
/// replies the card's full `idLabels` array; remove replies an empty body),
/// so this domain always acks with a null `id`; `lib.rs` backfills it from
/// the request's own `card_id` field — the id the spec calls for on these
/// two ops.
fn normalize_ack() -> Value {
    json!({ "ok": true, "id": Value::Null })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Method;

    #[test]
    fn list_requires_board_id() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("board_id"));
    }

    #[test]
    fn list_builds_get_boards_labels_path() {
        let call = build_call(r#"{"operation":"list","board_id":"B1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/boards/B1/labels");
        assert!(call.body.is_none());
    }

    #[test]
    fn add_requires_card_id_and_label_id() {
        assert!(build_call(r#"{"operation":"add","label_id":"L1"}"#).is_err());
        assert!(build_call(r#"{"operation":"add","card_id":"C1"}"#).is_err());
    }

    #[test]
    fn add_builds_post_with_value_body() {
        let call = build_call(r#"{"operation":"add","card_id":"C1","label_id":"L1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/cards/C1/idLabels");
        assert_eq!(call.body.as_ref().unwrap()["value"], "L1");
    }

    #[test]
    fn remove_requires_card_id_and_label_id() {
        assert!(build_call(r#"{"operation":"remove","label_id":"L1"}"#).is_err());
        assert!(build_call(r#"{"operation":"remove","card_id":"C1"}"#).is_err());
    }

    #[test]
    fn remove_builds_delete_with_card_and_label_path() {
        let call = build_call(r#"{"operation":"remove","card_id":"C1","label_id":"L1"}"#).unwrap();
        assert!(matches!(call.method, Method::Delete));
        assert_eq!(call.path, "/cards/C1/idLabels/L1");
        assert!(call.body.is_none());
    }

    #[test]
    fn normalize_list_maps_bare_array() {
        let raw = br#"[{"id":"LB1","name":"Urgent","color":"red"},{"id":"LB2","name":"","color":"green"}]"#;
        let out = normalize(LabelOp::List, raw).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["id"], "LB1");
        assert_eq!(out["results"][0]["name"], "Urgent");
        assert_eq!(out["results"][0]["color"], "red");
        assert_eq!(out["results"][1]["id"], "LB2");
    }

    #[test]
    fn normalize_list_handles_empty_array() {
        let out = normalize(LabelOp::List, b"[]").unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_list_rejects_invalid_json() {
        assert!(normalize(LabelOp::List, b"not json").is_err());
    }

    #[test]
    fn normalize_list_record_handles_missing_fields_without_panicking() {
        let raw = br#"[{"id":"LB1"}]"#;
        let out = normalize(LabelOp::List, raw).unwrap();
        assert_eq!(out["results"][0]["name"], Value::Null);
        assert_eq!(out["results"][0]["color"], Value::Null);
    }

    #[test]
    fn normalize_add_and_remove_ack_with_null_id() {
        let add = normalize(LabelOp::Add, br#"["LB1","LB2"]"#).unwrap();
        assert_eq!(add["ok"], true);
        assert_eq!(add["id"], Value::Null);

        let remove = normalize(LabelOp::Remove, b"").unwrap();
        assert_eq!(remove["ok"], true);
        assert_eq!(remove["id"], Value::Null);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"remove","card_id":"C1","label_id":"L1"}"#),
            Ok(LabelOp::Remove)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
