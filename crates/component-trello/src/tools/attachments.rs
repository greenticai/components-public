//! `trello_attachments` tool domain — pure HTTP-call building and response
//! normalization for Trello card-attachment operations (add/list). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext` `tools::issues` template: `AttachmentOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary.
//!
//! Only URL-based attachments are supported (Trello's `POST
//! /cards/{id}/attachments` accepts a `url` field without a multipart file
//! upload). File-upload attachments are NOT supported by this domain.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Trello attachment operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentOp {
    Add,
    List,
}

/// Raw `trello_attachments` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct AttachmentsInput {
    operation: AttachmentOp,
    #[serde(default)]
    card_id: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// Build the Trello REST v1 [`HttpCall`] for a `trello_attachments`
/// invocation.
///
/// Parses `args_json` into an [`AttachmentsInput`], validates the fields
/// required by the selected [`AttachmentOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field. `add` requires `url` — only URL-based attachments are
/// supported; file upload is not.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: AttachmentsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        AttachmentOp::Add => build_add(&input),
        AttachmentOp::List => build_list(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<AttachmentOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: AttachmentOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_add(input: &AttachmentsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    let url = super::require_field(input.url.as_deref(), "url")?;
    let mut body = Map::new();
    body.insert("url".to_string(), json!(url));
    if let Some(name) = &input.name {
        body.insert("name".to_string(), json!(name));
    }
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/cards/{card_id}/attachments"),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_list(input: &AttachmentsInput) -> Result<HttpCall, String> {
    let card_id = super::require_field(input.card_id.as_deref(), "card_id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/cards/{card_id}/attachments"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Trello REST v1 response body to the compact shape returned to
/// the model, based on the [`AttachmentOp`] that produced it.
pub fn normalize(op: AttachmentOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        AttachmentOp::Add => normalize_record(raw),
        AttachmentOp::List => normalize_list(raw),
    }
}

/// Normalize a single-attachment response to `{id,name,url,bytes?}`.
/// `bytes` is only present when Trello's response carries it (it's absent
/// or `null` for pure URL attachments without a known size).
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
    out.insert(
        "url".to_string(),
        value.get("url").cloned().unwrap_or(Value::Null),
    );
    if let Some(bytes) = value.get("bytes") {
        out.insert("bytes".to_string(), bytes.clone());
    }
    Value::Object(out)
}

fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid attachment response: {err}"))?;
    Ok(record_of(&value))
}

/// Normalize a `/cards/{card_id}/attachments` bare-array response to
/// `{total,results:[{id,name,url,bytes?}]}`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid attachment-list response: {err}"))?;
    let results: Vec<Value> = value
        .as_array()
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
    fn add_requires_card_id_and_url() {
        assert!(build_call(r#"{"operation":"add","url":"https://x"}"#).is_err());
        assert!(build_call(r#"{"operation":"add","card_id":"C1"}"#).is_err());
    }

    #[test]
    fn add_builds_post_with_url_and_optional_name() {
        let call = build_call(
            r#"{"operation":"add","card_id":"C1","url":"https://example.com/f.png","name":"screenshot"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/cards/C1/attachments");
        let body = call.body.as_ref().unwrap();
        assert_eq!(body["url"], "https://example.com/f.png");
        assert_eq!(body["name"], "screenshot");
    }

    #[test]
    fn add_body_omits_name_when_absent() {
        let call =
            build_call(r#"{"operation":"add","card_id":"C1","url":"https://example.com/f.png"}"#)
                .unwrap();
        assert!(call.body.as_ref().unwrap().get("name").is_none());
    }

    #[test]
    fn list_requires_card_id() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("card_id"));
    }

    #[test]
    fn list_builds_get_with_attachments_path() {
        let call = build_call(r#"{"operation":"list","card_id":"C1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/cards/C1/attachments");
        assert!(call.body.is_none());
    }

    #[test]
    fn normalize_add_extracts_record_fields_with_bytes() {
        let raw =
            br#"{"id":"AT1","name":"screenshot","url":"https://example.com/f.png","bytes":1024}"#;
        let out = normalize(AttachmentOp::Add, raw).unwrap();
        assert_eq!(out["id"], "AT1");
        assert_eq!(out["name"], "screenshot");
        assert_eq!(out["url"], "https://example.com/f.png");
        assert_eq!(out["bytes"], 1024);
    }

    #[test]
    fn normalize_record_omits_bytes_when_absent_and_handles_missing_fields() {
        let raw = br#"{"id":"AT1"}"#;
        let out = normalize(AttachmentOp::Add, raw).unwrap();
        assert_eq!(out["name"], Value::Null);
        assert_eq!(out["url"], Value::Null);
        assert!(out.get("bytes").is_none());
    }

    #[test]
    fn normalize_list_maps_bare_array() {
        let raw = br#"[{"id":"AT1","name":"a"},{"id":"AT2","name":"b"}]"#;
        let out = normalize(AttachmentOp::List, raw).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["id"], "AT1");
        assert_eq!(out["results"][1]["id"], "AT2");
    }

    #[test]
    fn normalize_list_handles_empty_array() {
        let out = normalize(AttachmentOp::List, b"[]").unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_rejects_invalid_json() {
        assert!(normalize(AttachmentOp::List, b"not json").is_err());
        assert!(normalize(AttachmentOp::Add, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"list","card_id":"C1"}"#),
            Ok(AttachmentOp::List)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
