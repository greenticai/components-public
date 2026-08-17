//! `stripe_coupons` tool domain — pure HTTP-call building and response
//! normalization for Stripe coupon operations (create/get/list/delete). No
//! WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`. See [`crate::tools::refunds`] for the template
//! this domain follows.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::{HttpCall, Method};

/// Stripe coupon operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CouponOp {
    Create,
    Get,
    List,
    Delete,
}

/// Raw `stripe_coupons` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct CouponsInput {
    operation: CouponOp,
    #[serde(default)]
    id: Option<String>,
    /// Coupon fields for `create` (must include `duration` + one of
    /// `percent_off`/`amount_off`). Required for create; passed through to
    /// Stripe as-is.
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    limit: Option<u32>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_coupons` invocation.
///
/// Parses `args_json` into a [`CouponsInput`], validates the fields required
/// by the selected [`CouponOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: CouponsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        CouponOp::Create => build_create(&input),
        CouponOp::Get => build_get(&input),
        CouponOp::List => Ok(build_list(&input)),
        CouponOp::Delete => build_delete(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<CouponOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: CouponOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &CouponsInput) -> Result<HttpCall, String> {
    let params = super::require_params(input.params.as_ref(), "params")?;
    Ok(HttpCall {
        method: Method::Post,
        path: "/coupons".to_string(),
        query: Vec::new(),
        body: Some(params),
    })
}

fn build_get(input: &CouponsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/coupons/{id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_list(input: &CouponsInput) -> HttpCall {
    let mut query = Vec::new();
    if let Some(limit) = input.limit {
        query.push(("limit".to_string(), limit.to_string()));
    }
    HttpCall {
        method: Method::Get,
        path: "/coupons".to_string(),
        query,
        body: None,
    }
}

fn build_delete(input: &CouponsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Delete,
        path: format!("/coupons/{id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model, based on the [`CouponOp`] that produced it.
pub fn normalize(op: CouponOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        CouponOp::List => normalize_list(raw),
        CouponOp::Create | CouponOp::Get | CouponOp::Delete => normalize_record(raw),
    }
}

/// Pull `{id,name,percent_off,amount_off,duration,valid,deleted}` out of a
/// single Stripe coupon object, defensively — every field falls back to `null`
/// rather than panicking. The `deleted` field handles the delete response
/// shape `{id,deleted:true}` without a separate extractor.
fn coupon_fields(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "name": value.get("name").cloned().unwrap_or(Value::Null),
        "percent_off": value.get("percent_off").cloned().unwrap_or(Value::Null),
        "amount_off": value.get("amount_off").cloned().unwrap_or(Value::Null),
        "duration": value.get("duration").cloned().unwrap_or(Value::Null),
        "valid": value.get("valid").cloned().unwrap_or(Value::Null),
        "deleted": value.get("deleted").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-coupon response (create/get/delete) to
/// `{id,name,percent_off,amount_off,duration,valid,deleted}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid coupon response: {err}"))?;
    Ok(coupon_fields(&value))
}

/// Normalize a list response (Stripe's `data[]` list shape) to
/// `{total, results:[{id,name,percent_off,amount_off,duration,valid,deleted}]}`.
/// `total` is the count of mapped `results`, not a value read from the
/// response body.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid list response: {err}"))?;
    let results: Vec<Value> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(coupon_fields)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_builds_post_with_params_body() {
        let call = build_call(
            r#"{"operation":"create","params":{"percent_off":25,"duration":"forever"}}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/coupons");
        assert_eq!(call.body.as_ref().unwrap()["percent_off"], 25);
        assert_eq!(call.body.as_ref().unwrap()["duration"], "forever");
    }

    #[test]
    fn create_missing_params_names_field() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("params"));
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"COUP_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/coupons/COUP_1");
        assert!(call.query.is_empty());
        assert!(call.body.is_none());
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn list_builds_get_with_limit_query() {
        let call = build_call(r#"{"operation":"list","limit":10}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/coupons");
        assert!(call.query.iter().any(|(k, v)| k == "limit" && v == "10"));
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/coupons");
        assert!(call.query.is_empty());
    }

    #[test]
    fn delete_builds_delete_with_id_path() {
        let call = build_call(r#"{"operation":"delete","id":"COUP_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Delete));
        assert_eq!(call.path, "/coupons/COUP_1");
        assert!(call.body.is_none());
    }

    #[test]
    fn delete_missing_id_names_field() {
        let err = build_call(r#"{"operation":"delete"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw = br#"{"id":"COUP_1","name":"25% off","percent_off":25.0,"amount_off":null,"duration":"forever","valid":true,"deleted":null}"#;
        let out = normalize(CouponOp::Get, raw).unwrap();
        assert_eq!(out["id"], "COUP_1");
        assert_eq!(out["name"], "25% off");
        assert_eq!(out["percent_off"], 25.0);
        assert_eq!(out["duration"], "forever");
        assert_eq!(out["valid"], true);
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"COUP_1"}"#;
        let out = normalize(CouponOp::Get, raw).unwrap();
        assert_eq!(out["id"], "COUP_1");
        assert_eq!(out["name"], Value::Null);
        assert_eq!(out["percent_off"], Value::Null);
        assert_eq!(out["amount_off"], Value::Null);
        assert_eq!(out["duration"], Value::Null);
        assert_eq!(out["valid"], Value::Null);
        assert_eq!(out["deleted"], Value::Null);
    }

    #[test]
    fn normalize_delete_returns_deleted_record() {
        let raw = br#"{"id":"COUP_1","deleted":true}"#;
        let out = normalize(CouponOp::Delete, raw).unwrap();
        assert_eq!(out["id"], "COUP_1");
        assert_eq!(out["deleted"], true);
    }

    #[test]
    fn normalize_list_maps_data_array() {
        let raw = br#"{"data":[{"id":"COUP_1","name":"25% off","percent_off":25.0,"duration":"forever","valid":true}],"has_more":false}"#;
        let out = normalize(CouponOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "COUP_1");
        assert_eq!(out["results"][0]["name"], "25% off");
    }

    #[test]
    fn normalize_list_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(CouponOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"delete","id":"COUP_1"}"#),
            Ok(CouponOp::Delete)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
