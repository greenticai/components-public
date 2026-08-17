//! `trello_checklists` tool domain — pure HTTP-call building and response
//! normalization for Trello checklist operations (create/add_item/
//! update_item). No WIT imports — this module is fully host-testable; the
//! actual `extension-host/http` invocation and `describe()` tool metadata
//! live in `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext` `tools::issues` template:
//! `ChecklistOp` (input enum) -> `build_call` (pure request builder) ->
//! `normalize` (pure response mapper), with no WIT/host types crossing the
//! boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Trello checklist operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChecklistOp {
    Create,
    AddItem,
    UpdateItem,
}

/// Raw `trello_checklists` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct ChecklistsInput {
    operation: ChecklistOp,
    #[serde(default)]
    card_id: Option<String>,
    #[serde(default)]
    checklist_id: Option<String>,
    #[serde(default)]
    checkitem_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

/// Build the Trello REST v1 [`HttpCall`] for a `trello_checklists`
/// invocation.
///
/// Parses `args_json` into a [`ChecklistsInput`], validates the fields
/// required by the selected [`ChecklistOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: ChecklistsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        ChecklistOp::Create => build_create(&input),
        ChecklistOp::AddItem => build_add_item(&input),
        ChecklistOp::UpdateItem => build_update_item(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<ChecklistOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: ChecklistOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &ChecklistsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    let mut body = Map::new();
    body.insert("idCard".to_string(), json!(card_id));
    if let Some(name) = &input.name {
        body.insert("name".to_string(), json!(name));
    }
    Ok(HttpCall {
        method: Method::Post,
        path: "/checklists".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_add_item(input: &ChecklistsInput) -> Result<HttpCall, String> {
    let checklist_id = super::require_field(input.checklist_id.as_deref(), "checklist_id")?;
    let mut body = Map::new();
    if let Some(name) = &input.name {
        body.insert("name".to_string(), json!(name));
    }
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/checklists/{checklist_id}/checkItems"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_update_item(input: &ChecklistsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    let checkitem_id = super::require_field(input.checkitem_id.as_deref(), "checkitem_id")?;
    let mut body = Map::new();
    if let Some(state) = &input.state {
        body.insert("state".to_string(), json!(state));
    }
    if let Some(name) = &input.name {
        body.insert("name".to_string(), json!(name));
    }
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/cards/{card_id}/checkItem/{checkitem_id}"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

/// Map a raw Trello REST v1 response body to the compact shape returned to
/// the model, based on the [`ChecklistOp`] that produced it.
pub fn normalize(op: ChecklistOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        ChecklistOp::Create | ChecklistOp::AddItem => normalize_record(raw),
        ChecklistOp::UpdateItem => Ok(normalize_ack(raw)),
    }
}

/// Normalize a single checklist/checkItem response (create/add_item) to
/// `{id,name,state?}`. `state` is only present on checkItem responses
/// (`add_item`); a checklist response (`create`) has no `state` field.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid checklist response: {err}"))?;
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "name".to_string(),
        value.get("name").cloned().unwrap_or(Value::Null),
    );
    if let Some(state) = value.get("state") {
        out.insert("state".to_string(), state.clone());
    }
    Ok(Value::Object(out))
}

/// Normalize an `update_item` response — Trello's `checkItem` endpoint
/// replies with the full updated item, but this domain normalizes it to a
/// uniform ack shape; `lib.rs` backfills a null `id` from the request's own
/// `checkitem_id` field.
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
    fn create_requires_card_id() {
        let err = build_call(r#"{"operation":"create","name":"Steps"}"#).unwrap_err();
        assert!(err.contains("card_id"));
    }

    #[test]
    fn create_builds_post_with_id_card_and_name() {
        let call = build_call(r#"{"operation":"create","card_id":"C1","name":"Steps"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/checklists");
        let body = call.body.as_ref().unwrap();
        assert_eq!(body["idCard"], "C1");
        assert_eq!(body["name"], "Steps");
    }

    #[test]
    fn add_item_requires_checklist_id() {
        let err = build_call(r#"{"operation":"add_item","name":"Step 1"}"#).unwrap_err();
        assert!(err.contains("checklist_id"));
    }

    #[test]
    fn add_item_builds_post_with_check_items_path() {
        let call =
            build_call(r#"{"operation":"add_item","checklist_id":"CL1","name":"Step 1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/checklists/CL1/checkItems");
        assert_eq!(call.body.as_ref().unwrap()["name"], "Step 1");
    }

    #[test]
    fn update_item_requires_card_id_and_checkitem_id() {
        assert!(build_call(r#"{"operation":"update_item","card_id":"C1"}"#).is_err());
        assert!(build_call(r#"{"operation":"update_item","checkitem_id":"CI1"}"#).is_err());
        let call = build_call(
            r#"{"operation":"update_item","card_id":"C1","checkitem_id":"CI1","state":"complete"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/cards/C1/checkItem/CI1");
        assert_eq!(call.body.as_ref().unwrap()["state"], "complete");
    }

    #[test]
    fn update_item_body_includes_only_present_fields() {
        let call = build_call(
            r#"{"operation":"update_item","card_id":"C1","checkitem_id":"CI1","name":"Renamed"}"#,
        )
        .unwrap();
        let body = call.body.as_ref().unwrap();
        assert_eq!(body["name"], "Renamed");
        assert!(body.get("state").is_none());
    }

    #[test]
    fn normalize_create_extracts_id_and_name_without_state() {
        let raw = br#"{"id":"CL1","name":"Steps","idCard":"C1"}"#;
        let out = normalize(ChecklistOp::Create, raw).unwrap();
        assert_eq!(out["id"], "CL1");
        assert_eq!(out["name"], "Steps");
        assert!(out.get("state").is_none());
    }

    #[test]
    fn normalize_add_item_extracts_id_name_state() {
        let raw = br#"{"id":"CI1","name":"Step 1","state":"incomplete"}"#;
        let out = normalize(ChecklistOp::AddItem, raw).unwrap();
        assert_eq!(out["id"], "CI1");
        assert_eq!(out["name"], "Step 1");
        assert_eq!(out["state"], "incomplete");
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br"{}";
        let out = normalize(ChecklistOp::Create, raw).unwrap();
        assert_eq!(out["id"], Value::Null);
        assert_eq!(out["name"], Value::Null);
    }

    #[test]
    fn normalize_update_item_ack_extracts_id_from_full_response() {
        let raw = br#"{"id":"CI1","name":"Step 1","state":"complete"}"#;
        let out = normalize(ChecklistOp::UpdateItem, raw).unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], "CI1");
    }

    #[test]
    fn normalize_update_item_ack_handles_empty_body() {
        let out = normalize(ChecklistOp::UpdateItem, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn normalize_rejects_invalid_json() {
        assert!(normalize(ChecklistOp::Create, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"add_item","checklist_id":"CL1"}"#),
            Ok(ChecklistOp::AddItem)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
