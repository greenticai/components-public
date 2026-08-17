//! `stripe_disputes` tool domain — pure HTTP-call building and response
//! normalization for Stripe dispute (chargeback) operations (get/list/update/
//! close). No WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`. See [`crate::tools::subscriptions`] for the
//! template this domain follows.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::{HttpCall, Method};

/// Stripe dispute operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisputeOp {
    Get,
    List,
    Update,
    Close,
}

/// Raw `stripe_disputes` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct DisputesInput {
    operation: DisputeOp,
    #[serde(default)]
    id: Option<String>,
    /// Charge id to filter disputes on `list`.
    #[serde(default)]
    charge: Option<String>,
    /// Payment intent id to filter disputes on `list`.
    #[serde(default)]
    payment_intent: Option<String>,
    /// Evidence / metadata body for `update`. Required for update.
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    limit: Option<u32>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_disputes` invocation.
///
/// Parses `args_json` into a [`DisputesInput`], validates the fields required
/// by the selected [`DisputeOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: DisputesInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        DisputeOp::Get => build_get(&input),
        DisputeOp::List => Ok(build_list(&input)),
        DisputeOp::Update => build_update(&input),
        DisputeOp::Close => build_close(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<DisputeOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: DisputeOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_get(input: &DisputesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/disputes/{id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_list(input: &DisputesInput) -> HttpCall {
    let mut query = Vec::new();
    if let Some(charge) = &input.charge {
        query.push(("charge".to_string(), charge.clone()));
    }
    if let Some(payment_intent) = &input.payment_intent {
        query.push(("payment_intent".to_string(), payment_intent.clone()));
    }
    if let Some(limit) = input.limit {
        query.push(("limit".to_string(), limit.to_string()));
    }
    HttpCall {
        method: Method::Get,
        path: "/disputes".to_string(),
        query,
        body: None,
    }
}

fn build_update(input: &DisputesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let params = super::require_params(input.params.as_ref(), "params")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/disputes/{id}"),
        query: Vec::new(),
        body: Some(params),
    })
}

fn build_close(input: &DisputesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/disputes/{id}/close"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model, based on the [`DisputeOp`] that produced it.
pub fn normalize(op: DisputeOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        DisputeOp::List => normalize_list(raw),
        DisputeOp::Get | DisputeOp::Update | DisputeOp::Close => normalize_record(raw),
    }
}

/// Pull `{id,amount,currency,status,reason,charge}` out of a single Stripe
/// dispute object, defensively — every field falls back to `null` rather than
/// panicking.
fn dispute_fields(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "amount": value.get("amount").cloned().unwrap_or(Value::Null),
        "currency": value.get("currency").cloned().unwrap_or(Value::Null),
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "reason": value.get("reason").cloned().unwrap_or(Value::Null),
        "charge": value.get("charge").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-dispute response (get/update/close) to
/// `{id,amount,currency,status,reason,charge}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid dispute response: {err}"))?;
    Ok(dispute_fields(&value))
}

/// Normalize a list response (Stripe's `data[]` list shape) to
/// `{total, results:[{id,amount,currency,status,reason,charge}]}`. `total` is
/// the count of mapped `results`, not a value read from the response body.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid list response: {err}"))?;
    let results: Vec<Value> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(dispute_fields)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"dp_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/disputes/dp_1");
        assert!(call.query.is_empty());
        assert!(call.body.is_none());
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn list_builds_get_with_charge_payment_intent_limit_query() {
        let call =
            build_call(r#"{"operation":"list","charge":"ch_1","payment_intent":"pi_1","limit":5}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/disputes");
        assert!(call.query.iter().any(|(k, v)| k == "charge" && v == "ch_1"));
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "payment_intent" && v == "pi_1")
        );
        assert!(call.query.iter().any(|(k, v)| k == "limit" && v == "5"));
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/disputes");
        assert!(call.query.is_empty());
    }

    #[test]
    fn update_builds_post_with_id_and_params() {
        let call = build_call(
            r#"{"operation":"update","id":"dp_1","params":{"evidence":{"customer_email_address":"jane@example.com"}}}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/disputes/dp_1");
        assert!(call.body.is_some());
    }

    #[test]
    fn update_missing_id_names_field() {
        let err = build_call(r#"{"operation":"update","params":{"evidence":{}}}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn update_missing_params_names_field() {
        let err = build_call(r#"{"operation":"update","id":"dp_1"}"#).unwrap_err();
        assert!(err.contains("params"));
    }

    #[test]
    fn close_builds_post_to_close_path() {
        let call = build_call(r#"{"operation":"close","id":"dp_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/disputes/dp_1/close");
        assert!(call.body.is_none());
    }

    #[test]
    fn close_missing_id_names_field() {
        let err = build_call(r#"{"operation":"close"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw = br#"{"id":"dp_1","amount":5000,"currency":"usd","status":"needs_response","reason":"fraudulent","charge":"ch_1"}"#;
        let out = normalize(DisputeOp::Get, raw).unwrap();
        assert_eq!(out["id"], "dp_1");
        assert_eq!(out["amount"], 5000);
        assert_eq!(out["currency"], "usd");
        assert_eq!(out["status"], "needs_response");
        assert_eq!(out["reason"], "fraudulent");
        assert_eq!(out["charge"], "ch_1");
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"dp_1"}"#;
        let out = normalize(DisputeOp::Get, raw).unwrap();
        assert_eq!(out["id"], "dp_1");
        assert_eq!(out["amount"], Value::Null);
        assert_eq!(out["currency"], Value::Null);
        assert_eq!(out["status"], Value::Null);
        assert_eq!(out["reason"], Value::Null);
        assert_eq!(out["charge"], Value::Null);
    }

    #[test]
    fn normalize_update_returns_updated_record() {
        let raw = br#"{"id":"dp_1","status":"under_review","amount":5000,"currency":"usd","reason":"fraudulent","charge":"ch_1"}"#;
        let out = normalize(DisputeOp::Update, raw).unwrap();
        assert_eq!(out["id"], "dp_1");
        assert_eq!(out["status"], "under_review");
    }

    #[test]
    fn normalize_close_returns_closed_record() {
        let raw = br#"{"id":"dp_1","status":"lost","amount":5000,"currency":"usd","reason":"fraudulent","charge":"ch_1"}"#;
        let out = normalize(DisputeOp::Close, raw).unwrap();
        assert_eq!(out["id"], "dp_1");
        assert_eq!(out["status"], "lost");
    }

    #[test]
    fn normalize_list_maps_data_array() {
        let raw = br#"{"data":[{"id":"dp_1","amount":5000,"currency":"usd","status":"needs_response","reason":"fraudulent","charge":"ch_1"}],"has_more":false}"#;
        let out = normalize(DisputeOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "dp_1");
        assert_eq!(out["results"][0]["status"], "needs_response");
    }

    #[test]
    fn normalize_list_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(DisputeOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"close","id":"dp_1"}"#),
            Ok(DisputeOp::Close)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
