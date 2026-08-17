//! `stripe_checkout_sessions` tool domain — pure HTTP-call building and
//! response normalization for Stripe Checkout Session operations
//! (create/get). No WIT imports — this module is fully host-testable; the
//! actual `extension-host/http` invocation and `describe()` tool metadata
//! live in `lib.rs` / `tool_meta.rs`. See [`crate::tools::customers`] for the
//! template this domain follows.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::{HttpCall, Method};

/// Stripe Checkout Session operation selected by the `operation` input
/// field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckoutSessionOp {
    Create,
    Get,
}

/// Raw `stripe_checkout_sessions` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct CheckoutSessionsInput {
    operation: CheckoutSessionOp,
    #[serde(default)]
    id: Option<String>,
    /// Pass-through create body (`mode`, `line_items`, `success_url`, …).
    /// Required for `create`.
    #[serde(default)]
    params: Option<Value>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_checkout_sessions`
/// invocation.
///
/// Parses `args_json` into a [`CheckoutSessionsInput`], validates the fields
/// required by the selected [`CheckoutSessionOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: CheckoutSessionsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        CheckoutSessionOp::Create => build_create(&input),
        CheckoutSessionOp::Get => build_get(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<CheckoutSessionOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: CheckoutSessionOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &CheckoutSessionsInput) -> Result<HttpCall, String> {
    let params = super::require_params(input.params.as_ref(), "params")?;
    Ok(HttpCall {
        method: Method::Post,
        path: "/checkout/sessions".to_string(),
        query: Vec::new(),
        body: Some(params),
    })
}

fn build_get(input: &CheckoutSessionsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/checkout/sessions/{id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model. Both `create` and `get` return a single Checkout Session object.
pub fn normalize(_op: CheckoutSessionOp, raw: &[u8]) -> Result<Value, String> {
    normalize_record(raw)
}

/// Pull `{id,url,payment_status,mode}` out of a single Stripe Checkout
/// Session object, defensively — every field falls back to `null` rather
/// than panicking.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid checkout session response: {err}"))?;
    Ok(json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "url": value.get("url").cloned().unwrap_or(Value::Null),
        "payment_status": value.get("payment_status").cloned().unwrap_or(Value::Null),
        "mode": value.get("mode").cloned().unwrap_or(Value::Null),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_builds_post_with_params_body() {
        let call = build_call(
            r#"{"operation":"create","params":{"mode":"payment","line_items":[{"price":"price_1","quantity":1}],"success_url":"https://x/success"}}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/checkout/sessions");
        assert_eq!(call.body.as_ref().unwrap()["mode"], "payment");
        assert_eq!(
            call.body.as_ref().unwrap()["line_items"][0]["price"],
            "price_1"
        );
        assert_eq!(
            call.body.as_ref().unwrap()["success_url"],
            "https://x/success"
        );
    }

    #[test]
    fn create_missing_params_names_field() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("params"));
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"cs_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/checkout/sessions/cs_1");
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw = br#"{"id":"cs_1","url":"https://checkout.stripe.com/x","payment_status":"unpaid","mode":"payment"}"#;
        let out = normalize(CheckoutSessionOp::Get, raw).unwrap();
        assert_eq!(out["id"], "cs_1");
        assert_eq!(out["url"], "https://checkout.stripe.com/x");
        assert_eq!(out["payment_status"], "unpaid");
        assert_eq!(out["mode"], "payment");
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"cs_1"}"#;
        let out = normalize(CheckoutSessionOp::Create, raw).unwrap();
        assert_eq!(out["id"], "cs_1");
        assert_eq!(out["url"], Value::Null);
        assert_eq!(out["payment_status"], Value::Null);
        assert_eq!(out["mode"], Value::Null);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","id":"cs_1"}"#),
            Ok(CheckoutSessionOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
