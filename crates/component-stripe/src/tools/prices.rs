//! `stripe_prices` tool domain — pure HTTP-call building and response
//! normalization for Stripe price operations (create/list/get). No WIT
//! imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`. See [`crate::tools::customers`] for the
//! template this domain follows.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::{HttpCall, Method};

/// Stripe price operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceOp {
    Create,
    List,
    Get,
}

/// Raw `stripe_prices` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct PricesInput {
    operation: PriceOp,
    #[serde(default)]
    id: Option<String>,
    /// Pass-through create body (`unit_amount`, `currency`, `product`,
    /// `recurring?`). Required for `create`; Stripe itself requires
    /// `unit_amount`, `currency`, and `product` within it.
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    product: Option<String>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_prices` invocation.
///
/// Parses `args_json` into a [`PricesInput`], validates the fields required
/// by the selected [`PriceOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the
/// field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: PricesInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        PriceOp::Create => build_create(&input),
        PriceOp::List => Ok(build_list(&input)),
        PriceOp::Get => build_get(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<PriceOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: PriceOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &PricesInput) -> Result<HttpCall, String> {
    let params = super::require_params(input.params.as_ref(), "params")?;
    Ok(HttpCall {
        method: Method::Post,
        path: "/prices".to_string(),
        query: Vec::new(),
        body: Some(params),
    })
}

fn build_list(input: &PricesInput) -> HttpCall {
    let mut query = Vec::new();
    if let Some(limit) = input.limit {
        query.push(("limit".to_string(), limit.to_string()));
    }
    if let Some(product) = &input.product {
        query.push(("product".to_string(), product.clone()));
    }
    HttpCall {
        method: Method::Get,
        path: "/prices".to_string(),
        query,
        body: None,
    }
}

fn build_get(input: &PricesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/prices/{id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model, based on the [`PriceOp`] that produced it.
pub fn normalize(op: PriceOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        PriceOp::List => normalize_list(raw),
        PriceOp::Create | PriceOp::Get => normalize_record(raw),
    }
}

/// Pull `{id,unit_amount,currency,product}` out of a single Stripe price
/// object, defensively — every field falls back to `null` rather than
/// panicking.
fn price_fields(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "unit_amount": value.get("unit_amount").cloned().unwrap_or(Value::Null),
        "currency": value.get("currency").cloned().unwrap_or(Value::Null),
        "product": value.get("product").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-price response (create/get) to
/// `{id,unit_amount,currency,product}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid price response: {err}"))?;
    Ok(price_fields(&value))
}

/// Normalize a list response (Stripe's `data[]` list shape) to
/// `{total, results:[{id,unit_amount,currency,product}]}`. `total` is the
/// count of mapped `results`, not a value read from the response body.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid list response: {err}"))?;
    let results: Vec<Value> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(price_fields)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_builds_post_with_params_body() {
        let call = build_call(
            r#"{"operation":"create","params":{"unit_amount":1000,"currency":"usd","product":"prod_1"}}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/prices");
        assert_eq!(call.body.as_ref().unwrap()["unit_amount"], 1000);
        assert_eq!(call.body.as_ref().unwrap()["currency"], "usd");
        assert_eq!(call.body.as_ref().unwrap()["product"], "prod_1");
    }

    #[test]
    fn create_missing_params_names_field() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("params"));
    }

    #[test]
    fn list_builds_get_with_limit_and_product_query() {
        let call = build_call(r#"{"operation":"list","limit":5,"product":"prod_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/prices");
        assert!(call.query.iter().any(|(k, v)| k == "limit" && v == "5"));
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "product" && v == "prod_1")
        );
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(call.query.is_empty());
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"price_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/prices/price_1");
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw = br#"{"id":"price_1","unit_amount":1000,"currency":"usd","product":"prod_1"}"#;
        let out = normalize(PriceOp::Get, raw).unwrap();
        assert_eq!(out["id"], "price_1");
        assert_eq!(out["unit_amount"], 1000);
        assert_eq!(out["currency"], "usd");
        assert_eq!(out["product"], "prod_1");
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"price_1"}"#;
        let out = normalize(PriceOp::Create, raw).unwrap();
        assert_eq!(out["id"], "price_1");
        assert_eq!(out["unit_amount"], Value::Null);
        assert_eq!(out["currency"], Value::Null);
        assert_eq!(out["product"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_data_array() {
        let raw =
            br#"{"data":[{"id":"price_1","unit_amount":1000,"currency":"usd","product":"prod_1"}],"has_more":false}"#;
        let out = normalize(PriceOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "price_1");
        assert_eq!(out["results"][0]["currency"], "usd");
    }

    #[test]
    fn normalize_list_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(PriceOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"get","id":"price_1"}"#),
            Ok(PriceOp::Get)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
