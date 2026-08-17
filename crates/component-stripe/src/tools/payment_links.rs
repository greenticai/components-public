//! `stripe_payment_links` tool domain — pure HTTP-call building and response
//! normalization for Stripe payment link operations (create/get/list/
//! deactivate). No WIT imports — this module is fully host-testable; the
//! actual `extension-host/http` invocation and `describe()` tool metadata
//! live in `lib.rs` / `tool_meta.rs`. See [`crate::tools::customers`] for the
//! template this domain follows.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Stripe payment-link operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentLinkOp {
    Create,
    Get,
    List,
    Deactivate,
}

/// Raw `stripe_payment_links` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct PaymentLinksInput {
    operation: PaymentLinkOp,
    #[serde(default)]
    id: Option<String>,
    /// Line items for `create`, an array of `{price,quantity}` objects.
    /// Required for `create`.
    #[serde(default)]
    line_items: Option<Value>,
    /// Additional pass-through create body fields (e.g.
    /// `after_completion`), merged with `line_items`. Optional.
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    limit: Option<u32>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_payment_links`
/// invocation.
///
/// Parses `args_json` into a [`PaymentLinksInput`], validates the fields
/// required by the selected [`PaymentLinkOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: PaymentLinksInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        PaymentLinkOp::Create => build_create(&input),
        PaymentLinkOp::Get => build_get(&input),
        PaymentLinkOp::List => Ok(build_list(&input)),
        PaymentLinkOp::Deactivate => build_deactivate(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<PaymentLinkOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: PaymentLinkOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

/// Build the create body: `params` (if any, and an object) as the base, with
/// `line_items` inserted/overwritten on top — `line_items` is always the
/// value the caller supplied, even if `params` also carried a stale key of
/// that name.
fn build_create(input: &PaymentLinksInput) -> Result<HttpCall, String> {
    let line_items = match &input.line_items {
        Some(value) if !value.is_null() => value.clone(),
        _ => return Err("missing required field: line_items".to_string()),
    };
    let mut body = match &input.params {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    body.insert("line_items".to_string(), line_items);
    Ok(HttpCall {
        method: Method::Post,
        path: "/payment_links".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_get(input: &PaymentLinksInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/payment_links/{id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_list(input: &PaymentLinksInput) -> HttpCall {
    let mut query = Vec::new();
    if let Some(limit) = input.limit {
        query.push(("limit".to_string(), limit.to_string()));
    }
    HttpCall {
        method: Method::Get,
        path: "/payment_links".to_string(),
        query,
        body: None,
    }
}

fn build_deactivate(input: &PaymentLinksInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/payment_links/{id}"),
        query: Vec::new(),
        body: Some(json!({ "active": false })),
    })
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model, based on the [`PaymentLinkOp`] that produced it.
pub fn normalize(op: PaymentLinkOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        PaymentLinkOp::List => normalize_list(raw),
        PaymentLinkOp::Create | PaymentLinkOp::Get | PaymentLinkOp::Deactivate => {
            normalize_record(raw)
        }
    }
}

/// Pull `{id,url,active}` out of a single Stripe payment link object,
/// defensively — every field falls back to `null` rather than panicking.
fn payment_link_fields(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "url": value.get("url").cloned().unwrap_or(Value::Null),
        "active": value.get("active").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-payment-link response (create/get/deactivate) to
/// `{id,url,active}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid payment link response: {err}"))?;
    Ok(payment_link_fields(&value))
}

/// Normalize a list response (Stripe's `data[]` list shape) to
/// `{total, results:[{id,url,active}]}`. `total` is the count of mapped
/// `results`, not a value read from the response body.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid list response: {err}"))?;
    let results: Vec<Value> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(payment_link_fields)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_builds_post_with_line_items_body() {
        let call =
            build_call(r#"{"operation":"create","line_items":[{"price":"price_1","quantity":1}]}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/payment_links");
        assert_eq!(
            call.body.as_ref().unwrap()["line_items"][0]["price"],
            "price_1"
        );
    }

    #[test]
    fn create_merges_line_items_into_params() {
        let call = build_call(
            r#"{"operation":"create","params":{"after_completion":{"type":"redirect"}},"line_items":[{"price":"price_1","quantity":2}]}"#,
        )
        .unwrap();
        let body = call.body.unwrap();
        assert_eq!(body["after_completion"]["type"], "redirect");
        assert_eq!(body["line_items"][0]["quantity"], 2);
    }

    #[test]
    fn create_missing_line_items_names_field() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("line_items"));
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"plink_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/payment_links/plink_1");
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn list_builds_get_with_limit_query() {
        let call = build_call(r#"{"operation":"list","limit":5}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/payment_links");
        assert!(call.query.iter().any(|(k, v)| k == "limit" && v == "5"));
    }

    #[test]
    fn list_with_no_limit_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(call.query.is_empty());
    }

    #[test]
    fn deactivate_builds_post_with_active_false_body() {
        let call = build_call(r#"{"operation":"deactivate","id":"plink_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/payment_links/plink_1");
        assert_eq!(call.body.as_ref().unwrap()["active"], false);
    }

    #[test]
    fn deactivate_missing_id_names_field() {
        let err = build_call(r#"{"operation":"deactivate"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw = br#"{"id":"plink_1","url":"https://buy.stripe.com/x","active":true}"#;
        let out = normalize(PaymentLinkOp::Get, raw).unwrap();
        assert_eq!(out["id"], "plink_1");
        assert_eq!(out["url"], "https://buy.stripe.com/x");
        assert_eq!(out["active"], true);
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"plink_1"}"#;
        let out = normalize(PaymentLinkOp::Create, raw).unwrap();
        assert_eq!(out["id"], "plink_1");
        assert_eq!(out["url"], Value::Null);
        assert_eq!(out["active"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_data_array() {
        let raw =
            br#"{"data":[{"id":"plink_1","url":"https://buy.stripe.com/x","active":true}],"has_more":false}"#;
        let out = normalize(PaymentLinkOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "plink_1");
    }

    #[test]
    fn normalize_list_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(PaymentLinkOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_deactivate_extracts_active_false() {
        let raw = br#"{"id":"plink_1","url":"https://buy.stripe.com/x","active":false}"#;
        let out = normalize(PaymentLinkOp::Deactivate, raw).unwrap();
        assert_eq!(out["active"], false);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"deactivate","id":"plink_1"}"#),
            Ok(PaymentLinkOp::Deactivate)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
