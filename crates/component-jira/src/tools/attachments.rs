//! `jira_attachments` tool domain — pure HTTP-call building and response
//! normalization for Jira issue attachment operations (add/list). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation lives in `lib.rs`.
//!
//! Follows the `tools::issues` template: `AttachmentOp` (input enum) ->
//! `build_call` (pure request builder) -> `normalize` (pure response
//! mapper).
//!
//! # `add` is not supported
//!
//! Jira's real upload endpoint (`POST /rest/api/3/issue/{id}/attachments`)
//! is `multipart/form-data` with a required `X-Atlassian-Token: no-check`
//! header. [`crate::client::HttpCall`] can only carry a JSON
//! `Option<serde_json::Value>` body (see `client.rs`) — there is no way to
//! express a multipart boundary or stream a file through it. Rather than
//! silently sending a broken (non-multipart) request that Jira would
//! reject, or faking a fixed-content upload, `build_call` for `add` always
//! returns `Err` unconditionally, before validating anything else. Callers
//! (and the A14 README) should tell users to attach files via the Jira UI
//! or the Jira API directly.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Jira attachment operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentOp {
    Add,
    List,
}

/// Raw `jira_attachments` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct AttachmentsInput {
    operation: AttachmentOp,
    #[serde(default)]
    id: Option<String>,
}

/// Build the Jira REST v3 [`HttpCall`] for a `jira_attachments` invocation.
///
/// `add` always fails — see the module-level doc comment for why file
/// upload cannot be expressed as an [`HttpCall`].
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: AttachmentsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        AttachmentOp::Add => Err(
            "jira_attachments add (file upload) is not supported by this extension; attach via the Jira UI or API directly"
                .to_string(),
        ),
        AttachmentOp::List => build_list(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires.
pub fn parse_operation(args_json: &str) -> Result<AttachmentOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: AttachmentOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_list(input: &AttachmentsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/rest/api/3/issue/{id}"),
        query: vec![("fields".to_string(), "attachment".to_string())],
        body: None,
    })
}

/// Map a raw Jira REST v3 response body to the compact shape returned to
/// the model, based on the [`AttachmentOp`] that produced it. `add` never
/// reaches here — `build_call` rejects it before any host call is made.
pub fn normalize(op: AttachmentOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        AttachmentOp::Add => {
            Err("jira_attachments add (file upload) is not supported by this extension".to_string())
        }
        AttachmentOp::List => normalize_list(raw),
    }
}

/// Build the compact `{id,filename,size,mimeType,url}` shape from a single
/// parsed attachment JSON value. Jira's `content` field is the download
/// URL, mapped to `url` here.
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "filename".to_string(),
        value.get("filename").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "size".to_string(),
        value.get("size").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "mimeType".to_string(),
        value.get("mimeType").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "url".to_string(),
        value.get("content").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a `GET /issue/{id}?fields=attachment` response (an issue-get
/// response) to `{total,results:[{id,filename,size,mimeType,url}]}`. Jira's
/// issue-get response has no separate attachment total, so `total` is
/// computed as the length of `fields.attachment`.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid attachment list response: {err}"))?;
    let attachments: Vec<&Value> = value
        .get("fields")
        .and_then(|fields| fields.get("attachment"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let results: Vec<Value> = attachments.iter().map(|item| record_of(item)).collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_always_returns_unsupported_error() {
        let err = build_call(r#"{"operation":"add","id":"AB-1"}"#).unwrap_err();
        assert!(err.contains("not supported"));
    }

    #[test]
    fn add_errors_even_without_id() {
        // add is documented to always fail before validating other fields.
        let err = build_call(r#"{"operation":"add"}"#).unwrap_err();
        assert!(err.contains("not supported"));
    }

    #[test]
    fn list_builds_get_with_fields_attachment_query() {
        let call = build_call(r#"{"operation":"list","id":"AB-1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/rest/api/3/issue/AB-1");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "fields" && v == "attachment")
        );
    }

    #[test]
    fn list_missing_id_names_field() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_list_maps_fields_attachment_array() {
        let raw = br#"{"fields":{"attachment":[{"id":"10001","filename":"screenshot.png","size":2048,"mimeType":"image/png","content":"https://example.atlassian.net/attachment/content/10001"}]}}"#;
        let out = normalize(AttachmentOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "10001");
        assert_eq!(out["results"][0]["filename"], "screenshot.png");
        assert_eq!(out["results"][0]["size"], 2048);
        assert_eq!(out["results"][0]["mimeType"], "image/png");
        assert_eq!(
            out["results"][0]["url"],
            "https://example.atlassian.net/attachment/content/10001"
        );
    }

    #[test]
    fn normalize_list_handles_no_attachments() {
        let raw = br#"{"fields":{}}"#;
        let out = normalize(AttachmentOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_add_returns_unsupported_error() {
        let err = normalize(AttachmentOp::Add, b"").unwrap_err();
        assert!(err.contains("not supported"));
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"list","id":"AB-1"}"#),
            Ok(AttachmentOp::List)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
