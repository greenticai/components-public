//! `stripe_refunds` tool domain — pure HTTP-call building and response
//! normalization for Stripe refund operations (create/get/list). Refunds are
//! money-moving, so this domain is kept deliberately narrow (no update/
//! cancel — Stripe refunds cannot be modified or reversed via the API). No
//! WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`. See [`crate::tools::customers`] for the
//! template this domain follows.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Stripe refund operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefundOp {
    Create,
    Get,
    List,
}

/// Raw `stripe_refunds` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct RefundsInput {
    operation: RefundOp,
    #[serde(default)]
    id: Option<String>,
    /// Payment intent to refund. `create`/`list` require at least one of
    /// `payment_intent`/`charge`.
    #[serde(default)]
    payment_intent: Option<String>,
    /// Charge to refund. `create`/`list` require at least one of
    /// `payment_intent`/`charge`.
    #[serde(default)]
    charge: Option<String>,
    /// Amount to refund, in the currency's smallest unit. Optional — Stripe
    /// refunds the full remaining amount when omitted.
    #[serde(default)]
    amount: Option<i64>,
    #[serde(default)]
    limit: Option<u32>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_refunds` invocation.
///
/// Parses `args_json` into a [`RefundsInput`], validates the fields required
/// by the selected [`RefundOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the
/// field(s). `create` requires at least one of `payment_intent`/`charge` —
/// the error names both when neither is supplied.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: RefundsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        RefundOp::Create => build_create(&input),
        RefundOp::Get => build_get(&input),
        RefundOp::List => Ok(build_list(&input)),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<RefundOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: RefundOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &RefundsInput) -> Result<HttpCall, String> {
    if input.payment_intent.is_none() && input.charge.is_none() {
        return Err("missing required field: at least one of payment_intent, charge".to_string());
    }
    let mut body = Map::new();
    if let Some(payment_intent) = &input.payment_intent {
        body.insert(
            "payment_intent".to_string(),
            Value::String(payment_intent.clone()),
        );
    }
    if let Some(charge) = &input.charge {
        body.insert("charge".to_string(), Value::String(charge.clone()));
    }
    if let Some(amount) = input.amount {
        body.insert("amount".to_string(), json!(amount));
    }
    Ok(HttpCall {
        method: Method::Post,
        path: "/refunds".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_get(input: &RefundsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/refunds/{id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_list(input: &RefundsInput) -> HttpCall {
    let mut query = Vec::new();
    if let Some(payment_intent) = &input.payment_intent {
        query.push(("payment_intent".to_string(), payment_intent.clone()));
    }
    if let Some(charge) = &input.charge {
        query.push(("charge".to_string(), charge.clone()));
    }
    if let Some(limit) = input.limit {
        query.push(("limit".to_string(), limit.to_string()));
    }
    HttpCall {
        method: Method::Get,
        path: "/refunds".to_string(),
        query,
        body: None,
    }
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model, based on the [`RefundOp`] that produced it.
pub fn normalize(op: RefundOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        RefundOp::List => normalize_list(raw),
        RefundOp::Create | RefundOp::Get => normalize_record(raw),
    }
}

/// Pull `{id,amount,status,payment_intent,charge}` out of a single Stripe
/// refund object, defensively — every field falls back to `null` rather than
/// panicking.
fn refund_fields(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "amount": value.get("amount").cloned().unwrap_or(Value::Null),
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "payment_intent": value.get("payment_intent").cloned().unwrap_or(Value::Null),
        "charge": value.get("charge").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-refund response (create/get) to
/// `{id,amount,status,payment_intent,charge}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid refund response: {err}"))?;
    Ok(refund_fields(&value))
}

/// Normalize a list response (Stripe's `data[]` list shape) to
/// `{total, results:[{id,amount,status,payment_intent,charge}]}`. `total` is
/// the count of mapped `results`, not a value read from the response body.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid list response: {err}"))?;
    let results: Vec<Value> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(refund_fields)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_builds_post_with_payment_intent_body() {
        let call = build_call(r#"{"operation":"create","payment_intent":"pi_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/refunds");
        assert_eq!(call.body.as_ref().unwrap()["payment_intent"], "pi_1");
        assert!(call.body.as_ref().unwrap().get("charge").is_none());
    }

    #[test]
    fn create_builds_post_with_charge_body() {
        let call = build_call(r#"{"operation":"create","charge":"ch_1"}"#).unwrap();
        assert_eq!(call.body.as_ref().unwrap()["charge"], "ch_1");
    }

    #[test]
    fn create_includes_optional_amount() {
        let call =
            build_call(r#"{"operation":"create","payment_intent":"pi_1","amount":500}"#).unwrap();
        assert_eq!(call.body.as_ref().unwrap()["amount"], 500);
    }

    #[test]
    fn create_missing_both_payment_intent_and_charge_names_both_fields() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("payment_intent"));
        assert!(err.contains("charge"));
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"re_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/refunds/re_1");
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn list_builds_get_with_payment_intent_charge_limit_query() {
        let call =
            build_call(r#"{"operation":"list","payment_intent":"pi_1","charge":"ch_1","limit":5}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/refunds");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "payment_intent" && v == "pi_1")
        );
        assert!(call.query.iter().any(|(k, v)| k == "charge" && v == "ch_1"));
        assert!(call.query.iter().any(|(k, v)| k == "limit" && v == "5"));
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(call.query.is_empty());
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw = br#"{"id":"re_1","amount":500,"status":"succeeded","payment_intent":"pi_1","charge":"ch_1"}"#;
        let out = normalize(RefundOp::Get, raw).unwrap();
        assert_eq!(out["id"], "re_1");
        assert_eq!(out["amount"], 500);
        assert_eq!(out["status"], "succeeded");
        assert_eq!(out["payment_intent"], "pi_1");
        assert_eq!(out["charge"], "ch_1");
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"re_1"}"#;
        let out = normalize(RefundOp::Create, raw).unwrap();
        assert_eq!(out["id"], "re_1");
        assert_eq!(out["amount"], Value::Null);
        assert_eq!(out["status"], Value::Null);
        assert_eq!(out["payment_intent"], Value::Null);
        assert_eq!(out["charge"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_data_array() {
        let raw = br#"{"data":[{"id":"re_1","amount":500,"status":"succeeded"}],"has_more":false}"#;
        let out = normalize(RefundOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "re_1");
        assert_eq!(out["results"][0]["amount"], 500);
    }

    #[test]
    fn normalize_list_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(RefundOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"list","limit":5}"#),
            Ok(RefundOp::List)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
