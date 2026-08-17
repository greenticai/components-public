//! `trello_cards` tool domain — pure HTTP-call building and response
//! normalization for Trello card operations (create/get/update/move/
//! archive/delete). No WIT imports — this module is fully host-testable;
//! the actual `extension-host/http` invocation and `describe()` tool
//! metadata live in `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext` `tools::issues` template: `CardOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Trello card operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardOp {
    Create,
    Get,
    Update,
    Move,
    Archive,
    Delete,
}

/// Raw `trello_cards` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct CardsInput {
    operation: CardOp,
    #[serde(default)]
    card_id: Option<String>,
    #[serde(default)]
    list_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    pos: Option<Value>,
}

/// Build the Trello REST v1 [`HttpCall`] for a `trello_cards` invocation.
///
/// Parses `args_json` into a [`CardsInput`], validates the fields required
/// by the selected [`CardOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the
/// field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: CardsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        CardOp::Create => build_create(&input),
        CardOp::Get => build_get(&input),
        CardOp::Update => build_update(&input),
        CardOp::Move => build_move(&input),
        CardOp::Archive => build_archive(&input),
        CardOp::Delete => build_delete(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<CardOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: CardOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &CardsInput) -> Result<HttpCall, String> {
    let list_id = super::require_field(input.list_id.as_deref(), "list_id")?;
    let mut body = Map::new();
    body.insert("idList".to_string(), json!(list_id));
    if let Some(name) = &input.name {
        body.insert("name".to_string(), json!(name));
    }
    if let Some(desc) = &input.desc {
        body.insert("desc".to_string(), json!(desc));
    }
    if let Some(pos) = &input.pos {
        body.insert("pos".to_string(), pos.clone());
    }
    Ok(HttpCall {
        method: Method::Post,
        path: "/cards".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_get(input: &CardsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/cards/{card_id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_update(input: &CardsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    let mut body = Map::new();
    if let Some(name) = &input.name {
        body.insert("name".to_string(), json!(name));
    }
    if let Some(desc) = &input.desc {
        body.insert("desc".to_string(), json!(desc));
    }
    if let Some(list_id) = &input.list_id {
        body.insert("idList".to_string(), json!(list_id));
    }
    if let Some(pos) = &input.pos {
        body.insert("pos".to_string(), pos.clone());
    }
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/cards/{card_id}"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_move(input: &CardsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    let list_id = super::require_field(input.list_id.as_deref(), "list_id")?;
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/cards/{card_id}"),
        query: Vec::new(),
        body: Some(json!({ "idList": list_id })),
    })
}

fn build_archive(input: &CardsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/cards/{card_id}"),
        query: Vec::new(),
        body: Some(json!({ "closed": true })),
    })
}

fn build_delete(input: &CardsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    Ok(HttpCall {
        method: Method::Delete,
        path: format!("/cards/{card_id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Trello REST v1 response body to the compact shape returned to
/// the model, based on the [`CardOp`] that produced it.
pub fn normalize(op: CardOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        CardOp::Create | CardOp::Get | CardOp::Update => normalize_record(raw),
        CardOp::Move | CardOp::Archive | CardOp::Delete => Ok(normalize_ack(raw)),
    }
}

/// Normalize a single-card response (create/get/update) to
/// `{id,name,idList,closed,url}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid card response: {err}"))?;
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "name".to_string(),
        value.get("name").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "idList".to_string(),
        value.get("idList").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "closed".to_string(),
        value.get("closed").cloned().unwrap_or(Value::Null),
    );
    if let Some(url) = value.get("url").and_then(Value::as_str) {
        out.insert("url".to_string(), Value::String(url.to_string()));
    }
    Ok(Value::Object(out))
}

/// Normalize a move/archive/delete response — delete replies `204 No
/// Content` (empty `raw`); move/archive reply the full updated card, but
/// this domain normalizes all three to a uniform ack shape. `lib.rs`
/// backfills a null `id` from the request's own `card_id` field.
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
        let err = build_call(r#"{"operation":"create","name":"Card"}"#).unwrap_err();
        assert!(err.contains("list_id"));
    }

    #[test]
    fn create_builds_post_with_id_list_and_optional_fields() {
        let call = build_call(
            r#"{"operation":"create","list_id":"L1","name":"Ship it","desc":"details","pos":"top"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/cards");
        let body = call.body.as_ref().unwrap();
        assert_eq!(body["idList"], "L1");
        assert_eq!(body["name"], "Ship it");
        assert_eq!(body["desc"], "details");
        assert_eq!(body["pos"], "top");
    }

    #[test]
    fn get_requires_card_id() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("card_id"));
    }

    #[test]
    fn get_builds_get_with_card_path() {
        let call = build_call(r#"{"operation":"get","card_id":"C1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/cards/C1");
        assert!(call.body.is_none());
    }

    #[test]
    fn update_requires_card_id() {
        let err = build_call(r#"{"operation":"update","name":"New"}"#).unwrap_err();
        assert!(err.contains("card_id"));
    }

    #[test]
    fn update_builds_put_with_present_fields_only() {
        let call =
            build_call(r#"{"operation":"update","card_id":"C1","name":"New name"}"#).unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/cards/C1");
        let body = call.body.as_ref().unwrap();
        assert_eq!(body["name"], "New name");
        assert!(body.get("desc").is_none());
        assert!(body.get("idList").is_none());
    }

    #[test]
    fn move_requires_card_id_and_list_id() {
        assert!(build_call(r#"{"operation":"move","card_id":"C1"}"#).is_err());
        assert!(build_call(r#"{"operation":"move","list_id":"L2"}"#).is_err());
        let call = build_call(r#"{"operation":"move","card_id":"C1","list_id":"L2"}"#).unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/cards/C1");
        assert_eq!(call.body.as_ref().unwrap()["idList"], "L2");
    }

    #[test]
    fn archive_builds_closed_true_body() {
        let call = build_call(r#"{"operation":"archive","card_id":"C1"}"#).unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/cards/C1");
        assert_eq!(call.body.as_ref().unwrap()["closed"], true);
    }

    #[test]
    fn archive_requires_card_id() {
        let err = build_call(r#"{"operation":"archive"}"#).unwrap_err();
        assert!(err.contains("card_id"));
    }

    #[test]
    fn delete_builds_delete() {
        let call = build_call(r#"{"operation":"delete","card_id":"C9"}"#).unwrap();
        assert!(matches!(call.method, Method::Delete));
        assert_eq!(call.path, "/cards/C9");
    }

    #[test]
    fn delete_requires_card_id() {
        let err = build_call(r#"{"operation":"delete"}"#).unwrap_err();
        assert!(err.contains("card_id"));
    }

    #[test]
    fn normalize_get_extracts_record_fields() {
        let raw = br#"{"id":"C1","name":"Ship it","idList":"L1","closed":false,"url":"https://trello.com/c/abc"}"#;
        let out = normalize(CardOp::Get, raw).unwrap();
        assert_eq!(out["id"], "C1");
        assert_eq!(out["name"], "Ship it");
        assert_eq!(out["idList"], "L1");
        assert_eq!(out["closed"], false);
        assert_eq!(out["url"], "https://trello.com/c/abc");
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"C1"}"#;
        let out = normalize(CardOp::Create, raw).unwrap();
        assert_eq!(out["name"], Value::Null);
        assert_eq!(out["idList"], Value::Null);
        assert_eq!(out["closed"], Value::Null);
        assert!(out.get("url").is_none());
    }

    #[test]
    fn normalize_delete_ack_handles_empty_body() {
        let out = normalize(CardOp::Delete, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn normalize_move_ack_extracts_id_from_full_card_response() {
        let raw = br#"{"id":"C1","name":"Ship it","idList":"L2"}"#;
        let out = normalize(CardOp::Move, raw).unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], "C1");
    }

    #[test]
    fn normalize_rejects_invalid_json() {
        assert!(normalize(CardOp::Get, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"delete","card_id":"C9"}"#),
            Ok(CardOp::Delete)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
