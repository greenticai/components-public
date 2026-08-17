//! `stripe_payment_intents` tool domain — pure HTTP-call building and response
//! normalization for Stripe PaymentIntent operations (create/get/list/confirm/
//! cancel). No WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`. See [`crate::tools::subscriptions`] for the
//! template this domain follows; the `amount`/`currency` + `params` merge on
//! `create` follows a similar dedicated-field-merged-into-body shape.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Stripe PaymentIntent operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentIntentOp {
    Create,
    Get,
    List,
    Confirm,
    Cancel,
}

/// Raw `stripe_payment_intents` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct PaymentIntentsInput {
    operation: PaymentIntentOp,
    #[serde(default)]
    id: Option<String>,
    /// Amount in the currency's smallest unit (e.g. cents). Required for
    /// `create`.
    #[serde(default)]
    amount: Option<i64>,
    /// Three-letter ISO 4217 currency code. Required for `create`.
    #[serde(default)]
    currency: Option<String>,
    /// Customer id to associate with the PaymentIntent. Optional for
    /// `create`; also an optional list filter.
    #[serde(default)]
    customer: Option<String>,
    /// Additional pass-through body fields: merged with `amount`/`currency`/
    /// `customer` on `create`; passed verbatim on `confirm`/`cancel`.
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    limit: Option<u32>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_payment_intents`
/// invocation.
///
/// Parses `args_json` into a [`PaymentIntentsInput`], validates the fields
/// required by the selected [`PaymentIntentOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: PaymentIntentsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        PaymentIntentOp::Create => build_create(&input),
        PaymentIntentOp::Get => build_get(&input),
        PaymentIntentOp::List => Ok(build_list(&input)),
        PaymentIntentOp::Confirm => build_confirm(&input),
        PaymentIntentOp::Cancel => build_cancel(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<PaymentIntentOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: PaymentIntentOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

/// Build the create body: `params` (if any, and an object) as the base, with
/// `amount`, `currency`, and (optionally) `customer` inserted/overwritten on
/// top — always the values the caller supplied, even if `params` also carried
/// stale keys of those names.
fn build_create(input: &PaymentIntentsInput) -> Result<HttpCall, String> {
    let amount = input
        .amount
        .ok_or_else(|| "missing required field: amount".to_string())?;
    let currency = super::require_field(input.currency.as_deref(), "currency")?.to_string();
    let mut body = match &input.params {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    body.insert("amount".to_string(), json!(amount));
    body.insert("currency".to_string(), Value::String(currency));
    if let Some(customer) = &input.customer {
        body.insert("customer".to_string(), Value::String(customer.clone()));
    }
    Ok(HttpCall {
        method: Method::Post,
        path: "/payment_intents".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_get(input: &PaymentIntentsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/payment_intents/{id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_list(input: &PaymentIntentsInput) -> HttpCall {
    let mut query = Vec::new();
    if let Some(customer) = &input.customer {
        query.push(("customer".to_string(), customer.clone()));
    }
    if let Some(limit) = input.limit {
        query.push(("limit".to_string(), limit.to_string()));
    }
    HttpCall {
        method: Method::Get,
        path: "/payment_intents".to_string(),
        query,
        body: None,
    }
}

fn build_confirm(input: &PaymentIntentsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let body = match &input.params {
        Some(v @ Value::Object(_)) => Some(v.clone()),
        _ => None,
    };
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/payment_intents/{id}/confirm"),
        query: Vec::new(),
        body,
    })
}

fn build_cancel(input: &PaymentIntentsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let body = match &input.params {
        Some(v @ Value::Object(_)) => Some(v.clone()),
        _ => None,
    };
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/payment_intents/{id}/cancel"),
        query: Vec::new(),
        body,
    })
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model, based on the [`PaymentIntentOp`] that produced it. `cancel` and
/// `confirm` return the updated PaymentIntent object, so they are normalized
/// as records like `create` and `get`.
pub fn normalize(op: PaymentIntentOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        PaymentIntentOp::List => normalize_list(raw),
        PaymentIntentOp::Create
        | PaymentIntentOp::Get
        | PaymentIntentOp::Confirm
        | PaymentIntentOp::Cancel => normalize_record(raw),
    }
}

/// Pull `{id,amount,currency,status,client_secret}` out of a single Stripe
/// PaymentIntent object, defensively — every field falls back to `null`
/// rather than panicking.
fn payment_intent_fields(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "amount": value.get("amount").cloned().unwrap_or(Value::Null),
        "currency": value.get("currency").cloned().unwrap_or(Value::Null),
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "client_secret": value.get("client_secret").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single PaymentIntent response (create/get/confirm/cancel) to
/// `{id,amount,currency,status,client_secret}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid payment intent response: {err}"))?;
    Ok(payment_intent_fields(&value))
}

/// Normalize a list response (Stripe's `data[]` list shape) to
/// `{total, results:[{id,amount,currency,status,client_secret}]}`. `total` is
/// the count of mapped `results`, not a value read from the response body.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid list response: {err}"))?;
    let results: Vec<Value> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(payment_intent_fields)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_builds_post_with_amount_and_currency_body() {
        let call = build_call(r#"{"operation":"create","amount":2000,"currency":"usd"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/payment_intents");
        assert_eq!(call.body.as_ref().unwrap()["amount"], 2000);
        assert_eq!(call.body.as_ref().unwrap()["currency"], "usd");
    }

    #[test]
    fn create_merges_params_and_customer_into_body() {
        let call = build_call(
            r#"{"operation":"create","amount":5000,"currency":"eur","customer":"cus_1","params":{"description":"test"}}"#,
        )
        .unwrap();
        let body = call.body.unwrap();
        assert_eq!(body["amount"], 5000);
        assert_eq!(body["currency"], "eur");
        assert_eq!(body["customer"], "cus_1");
        assert_eq!(body["description"], "test");
    }

    #[test]
    fn create_missing_amount_names_field() {
        let err = build_call(r#"{"operation":"create","currency":"usd"}"#).unwrap_err();
        assert!(err.contains("amount"));
    }

    #[test]
    fn create_missing_currency_names_field() {
        let err = build_call(r#"{"operation":"create","amount":2000}"#).unwrap_err();
        assert!(err.contains("currency"));
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"pi_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/payment_intents/pi_1");
        assert!(call.body.is_none());
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn list_builds_get_with_customer_and_limit_query() {
        let call = build_call(r#"{"operation":"list","customer":"cus_1","limit":10}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/payment_intents");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "customer" && v == "cus_1")
        );
        assert!(call.query.iter().any(|(k, v)| k == "limit" && v == "10"));
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/payment_intents");
        assert!(call.query.is_empty());
    }

    #[test]
    fn confirm_builds_post_with_id_path() {
        let call = build_call(r#"{"operation":"confirm","id":"pi_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/payment_intents/pi_1/confirm");
        assert!(call.body.is_none());
    }

    #[test]
    fn confirm_missing_id_names_field() {
        let err = build_call(r#"{"operation":"confirm"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn cancel_builds_post_with_id_path() {
        let call = build_call(r#"{"operation":"cancel","id":"pi_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/payment_intents/pi_1/cancel");
        assert!(call.body.is_none());
    }

    #[test]
    fn cancel_missing_id_names_field() {
        let err = build_call(r#"{"operation":"cancel"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw = br#"{"id":"pi_1","amount":2000,"currency":"usd","status":"requires_payment_method","client_secret":"pi_1_secret_abc"}"#;
        let out = normalize(PaymentIntentOp::Get, raw).unwrap();
        assert_eq!(out["id"], "pi_1");
        assert_eq!(out["amount"], 2000);
        assert_eq!(out["currency"], "usd");
        assert_eq!(out["status"], "requires_payment_method");
        assert_eq!(out["client_secret"], "pi_1_secret_abc");
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"pi_1"}"#;
        let out = normalize(PaymentIntentOp::Create, raw).unwrap();
        assert_eq!(out["id"], "pi_1");
        assert_eq!(out["amount"], Value::Null);
        assert_eq!(out["currency"], Value::Null);
        assert_eq!(out["status"], Value::Null);
        assert_eq!(out["client_secret"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_data_array() {
        let raw = br#"{"data":[{"id":"pi_1","amount":2000,"currency":"usd","status":"succeeded"}],"has_more":false}"#;
        let out = normalize(PaymentIntentOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "pi_1");
        assert_eq!(out["results"][0]["amount"], 2000);
        assert_eq!(out["results"][0]["currency"], "usd");
    }

    #[test]
    fn normalize_list_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(PaymentIntentOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"cancel","id":"pi_1"}"#),
            Ok(PaymentIntentOp::Cancel)
        );
        assert_eq!(
            parse_operation(r#"{"operation":"create","amount":1000,"currency":"usd"}"#),
            Ok(PaymentIntentOp::Create)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
