//! `calendly_webhooks` tool domain — pure HTTP-call building and response
//! normalization for Calendly webhook subscription operations
//! (create/list/delete). No WIT imports — this module is fully
//! host-testable; the actual `extension-host/http` invocation and
//! `describe()` tool metadata live in `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext::tools::issues` template: `WebhookOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Calendly webhook subscription operation selected by the `operation`
/// input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookOp {
    Create,
    List,
    Delete,
}

/// Raw `calendly_webhooks` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct WebhooksInput {
    operation: WebhookOp,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    events: Option<Vec<String>>,
    #[serde(default)]
    organization: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    signing_key: Option<String>,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    uuid: Option<String>,
}

/// Build the Calendly REST v2 [`HttpCall`] for a `calendly_webhooks`
/// invocation.
///
/// Parses `args_json` into a [`WebhooksInput`], validates the fields
/// required by the selected [`WebhookOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: WebhooksInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        WebhookOp::Create => build_create(&input),
        WebhookOp::List => build_list(&input),
        WebhookOp::Delete => build_delete(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<WebhookOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: WebhookOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

/// Fetch a required, non-empty `events` array, rejecting `None` and an
/// empty list.
fn require_events(events: Option<&Vec<String>>) -> Result<&Vec<String>, String> {
    match events {
        Some(list) if !list.is_empty() => Ok(list),
        _ => Err("missing required field: events".to_string()),
    }
}

fn build_create(input: &WebhooksInput) -> Result<HttpCall, String> {
    let url = super::require_field(input.url.as_deref(), "url")?;
    let events = require_events(input.events.as_ref())?;
    let organization = super::require_field(input.organization.as_deref(), "organization")?;
    let scope = super::require_field(input.scope.as_deref(), "scope")?;

    let mut body = Map::new();
    body.insert("url".to_string(), Value::String(url.to_string()));
    body.insert(
        "events".to_string(),
        Value::Array(events.iter().cloned().map(Value::String).collect()),
    );
    body.insert(
        "organization".to_string(),
        Value::String(organization.to_string()),
    );
    body.insert("scope".to_string(), Value::String(scope.to_string()));
    if let Some(user) = input.user.as_deref().filter(|v| !v.is_empty()) {
        body.insert("user".to_string(), Value::String(user.to_string()));
    }
    if let Some(signing_key) = input.signing_key.as_deref().filter(|v| !v.is_empty()) {
        body.insert(
            "signing_key".to_string(),
            Value::String(signing_key.to_string()),
        );
    }

    Ok(HttpCall {
        method: Method::Post,
        path: "/webhook_subscriptions".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_list(input: &WebhooksInput) -> Result<HttpCall, String> {
    let organization = super::require_field(input.organization.as_deref(), "organization")?;
    let scope = super::require_field(input.scope.as_deref(), "scope")?;

    let mut query = vec![
        ("organization".to_string(), organization.to_string()),
        ("scope".to_string(), scope.to_string()),
    ];
    if let Some(user) = input.user.as_deref().filter(|v| !v.is_empty()) {
        query.push(("user".to_string(), user.to_string()));
    }
    if let Some(count) = input.count {
        query.push(("count".to_string(), count.to_string()));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: "/webhook_subscriptions".to_string(),
        query,
        body: None,
    })
}

fn build_delete(input: &WebhooksInput) -> Result<HttpCall, String> {
    let uuid = super::require_field(input.uuid.as_deref(), "uuid")?;
    Ok(HttpCall {
        method: Method::Delete,
        path: format!("/webhook_subscriptions/{uuid}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Calendly Webhook Subscriptions response body to the compact
/// shape returned to the model, based on the [`WebhookOp`] that produced
/// it.
pub fn normalize(op: WebhookOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        WebhookOp::Create => normalize_create(raw),
        WebhookOp::List => normalize_list(raw),
        WebhookOp::Delete => normalize_delete(raw),
    }
}

/// Map a single webhook subscription resource to
/// `{uri,callback_url,state,events,scope}`, without panicking on
/// missing/absent fields.
fn record_of(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "uri".to_string(),
        value.get("uri").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "callback_url".to_string(),
        value.get("callback_url").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "state".to_string(),
        value.get("state").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "events".to_string(),
        value.get("events").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "scope".to_string(),
        value.get("scope").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a `POST /webhook_subscriptions` response, unwrapping
/// Calendly's `{"resource": {...}}` envelope.
fn normalize_create(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid webhook subscription response: {err}"))?;
    let resource = value.get("resource").unwrap_or(&value);
    Ok(record_of(resource))
}

/// Normalize a `GET /webhook_subscriptions` list response to
/// `{total,results:[{uri,callback_url,state,events,scope}]}`, mapping the
/// `collection[]` array. `total` is `pagination.count` when present,
/// falling back to the mapped `results` length.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid webhook subscriptions list response: {err}"))?;
    let results: Vec<Value> = value
        .get("collection")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(record_of)
        .collect();
    let total = value
        .get("pagination")
        .and_then(|pagination| pagination.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(results.len() as u64);
    Ok(json!({ "total": total, "results": results }))
}

/// Normalize a `DELETE /webhook_subscriptions/{uuid}` response to
/// `{ok:true, id}`. Calendly's delete response is an empty 204 body, so
/// `id` is left `null` here; the dispatch layer
/// (`lib.rs::invoke_webhooks`) backfills it from the request's own `uuid`
/// field.
fn normalize_delete(raw: &[u8]) -> Result<Value, String> {
    // The delete response body isn't used for the normalized shape, but a
    // malformed non-empty body is still worth rejecting explicitly rather
    // than silently ignoring it.
    if !raw.is_empty() && serde_json::from_slice::<Value>(raw).is_err() {
        return Err("invalid delete response".to_string());
    }
    Ok(json!({ "ok": true, "id": Value::Null }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_requires_url_events_organization_and_scope() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("url"));

        let err =
            build_call(r#"{"operation":"create","url":"https://example.com/hook"}"#).unwrap_err();
        assert!(err.contains("events"));

        let err = build_call(
            r#"{"operation":"create","url":"https://example.com/hook","events":["invitee.created"]}"#,
        )
        .unwrap_err();
        assert!(err.contains("organization"));

        let err = build_call(
            r#"{"operation":"create","url":"https://example.com/hook","events":["invitee.created"],"organization":"https://api.calendly.com/organizations/o1"}"#,
        )
        .unwrap_err();
        assert!(err.contains("scope"));
    }

    #[test]
    fn create_rejects_empty_events_array() {
        let err = build_call(
            r#"{"operation":"create","url":"https://example.com/hook","events":[],"organization":"https://api.calendly.com/organizations/o1","scope":"organization"}"#,
        )
        .unwrap_err();
        assert!(err.contains("events"));
    }

    #[test]
    fn create_builds_post_with_required_body() {
        let call = build_call(
            r#"{"operation":"create","url":"https://example.com/hook","events":["invitee.created","invitee.canceled"],"organization":"https://api.calendly.com/organizations/o1","scope":"organization"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/webhook_subscriptions");
        let body = call.body.unwrap();
        assert_eq!(body["url"], "https://example.com/hook");
        assert_eq!(
            body["events"],
            json!(["invitee.created", "invitee.canceled"])
        );
        assert_eq!(
            body["organization"],
            "https://api.calendly.com/organizations/o1"
        );
        assert_eq!(body["scope"], "organization");
        assert!(body.get("user").is_none());
        assert!(body.get("signing_key").is_none());
    }

    #[test]
    fn create_includes_optional_user_and_signing_key_when_present() {
        let call = build_call(
            r#"{"operation":"create","url":"https://example.com/hook","events":["invitee.created"],"organization":"https://api.calendly.com/organizations/o1","scope":"user","user":"https://api.calendly.com/users/u1","signing_key":"shh"}"#,
        )
        .unwrap();
        let body = call.body.unwrap();
        assert_eq!(body["user"], "https://api.calendly.com/users/u1");
        assert_eq!(body["signing_key"], "shh");
    }

    #[test]
    fn list_requires_organization_and_scope() {
        let err = build_call(r#"{"operation":"list"}"#).unwrap_err();
        assert!(err.contains("organization"));

        let err = build_call(
            r#"{"operation":"list","organization":"https://api.calendly.com/organizations/o1"}"#,
        )
        .unwrap_err();
        assert!(err.contains("scope"));
    }

    #[test]
    fn list_builds_get_with_query_params() {
        let call = build_call(
            r#"{"operation":"list","organization":"https://api.calendly.com/organizations/o1","scope":"organization","user":"https://api.calendly.com/users/u1","count":20}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/webhook_subscriptions");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "organization"
                    && v == "https://api.calendly.com/organizations/o1")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "scope" && v == "organization")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "user" && v == "https://api.calendly.com/users/u1")
        );
        assert!(call.query.iter().any(|(k, v)| k == "count" && v == "20"));
    }

    #[test]
    fn list_without_optional_filters_has_only_required_query() {
        let call = build_call(
            r#"{"operation":"list","organization":"https://api.calendly.com/organizations/o1","scope":"organization"}"#,
        )
        .unwrap();
        assert_eq!(call.query.len(), 2);
    }

    #[test]
    fn delete_requires_uuid() {
        let err = build_call(r#"{"operation":"delete"}"#).unwrap_err();
        assert!(err.contains("uuid"));

        let call = build_call(r#"{"operation":"delete","uuid":"AAAA"}"#).unwrap();
        assert!(matches!(call.method, Method::Delete));
        assert_eq!(call.path, "/webhook_subscriptions/AAAA");
        assert!(call.body.is_none());
    }

    #[test]
    fn normalize_create_unwraps_resource_envelope() {
        let raw = json!({
            "resource": {
                "uri": "https://api.calendly.com/webhook_subscriptions/AAAA",
                "callback_url": "https://example.com/hook",
                "state": "active",
                "events": ["invitee.created"],
                "scope": "organization"
            }
        })
        .to_string();
        let out = normalize(WebhookOp::Create, raw.as_bytes()).unwrap();
        assert_eq!(
            out["uri"],
            "https://api.calendly.com/webhook_subscriptions/AAAA"
        );
        assert_eq!(out["callback_url"], "https://example.com/hook");
        assert_eq!(out["state"], "active");
        assert_eq!(out["events"], json!(["invitee.created"]));
        assert_eq!(out["scope"], "organization");
    }

    #[test]
    fn normalize_create_handles_missing_fields_without_panicking() {
        let raw = json!({ "resource": {} }).to_string();
        let out = normalize(WebhookOp::Create, raw.as_bytes()).unwrap();
        assert_eq!(out["uri"], Value::Null);
        assert_eq!(out["state"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_collection_and_pagination_count() {
        let raw = json!({
            "collection": [
                { "uri": "https://api.calendly.com/webhook_subscriptions/AAAA", "state": "active" },
                { "uri": "https://api.calendly.com/webhook_subscriptions/BBBB", "state": "disabled" }
            ],
            "pagination": { "count": 2 }
        })
        .to_string();
        let out = normalize(WebhookOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["state"], "active");
        assert_eq!(out["results"][1]["state"], "disabled");
    }

    #[test]
    fn normalize_list_falls_back_to_results_len_without_pagination() {
        let raw = json!({ "collection": [{ "uri": "x" }] }).to_string();
        let out = normalize(WebhookOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 1);
    }

    #[test]
    fn normalize_list_handles_empty_collection() {
        let raw = json!({ "collection": [] }).to_string();
        let out = normalize(WebhookOp::List, raw.as_bytes()).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_delete_returns_ack_with_null_id_for_backfill() {
        let out = normalize(WebhookOp::Delete, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn normalize_delete_rejects_malformed_non_empty_body() {
        assert!(normalize(WebhookOp::Delete, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"delete","uuid":"AAAA"}"#),
            Ok(WebhookOp::Delete)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
