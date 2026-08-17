//! `stripe_promotion_codes` tool domain — pure HTTP-call building and response
//! normalization for Stripe promotion code operations (create/get/list/update).
//! No WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`. See [`crate::tools::coupons`] for the template
//! this domain follows (promotion codes are the customer-facing codes that
//! redeem a coupon).

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::{HttpCall, Method};

/// Stripe promotion code operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionCodeOp {
    Create,
    Get,
    List,
    Update,
}

/// Raw `stripe_promotion_codes` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct PromotionCodesInput {
    operation: PromotionCodeOp,
    /// Promotion code id (e.g. `"promo_123"`). Required for get and update.
    #[serde(default)]
    id: Option<String>,
    /// Coupon id this promotion code redeems. Required for create.
    #[serde(default)]
    coupon: Option<String>,
    /// Additional fields for create (merged with `coupon`) or the full update
    /// body. Optional for create; required for update.
    #[serde(default)]
    params: Option<Value>,
    /// Max results to return for list.
    #[serde(default)]
    limit: Option<u32>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_promotion_codes`
/// invocation.
///
/// Parses `args_json` into a [`PromotionCodesInput`], validates the fields
/// required by the selected [`PromotionCodeOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: PromotionCodesInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        PromotionCodeOp::Create => build_create(&input),
        PromotionCodeOp::Get => build_get(&input),
        PromotionCodeOp::List => Ok(build_list(&input)),
        PromotionCodeOp::Update => build_update(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<PromotionCodeOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: PromotionCodeOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &PromotionCodesInput) -> Result<HttpCall, String> {
    let coupon = super::require_field(input.coupon.as_deref(), "coupon")?.to_string();
    let mut body = match &input.params {
        Some(Value::Object(m)) => m.clone(),
        _ => serde_json::Map::new(),
    };
    body.insert("coupon".into(), Value::String(coupon));
    Ok(HttpCall {
        method: Method::Post,
        path: "/promotion_codes".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_get(input: &PromotionCodesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/promotion_codes/{id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_list(input: &PromotionCodesInput) -> HttpCall {
    let mut query = Vec::new();
    if let Some(ref coupon) = input.coupon
        && !coupon.is_empty()
    {
        query.push(("coupon".to_string(), coupon.clone()));
    }
    if let Some(limit) = input.limit {
        query.push(("limit".to_string(), limit.to_string()));
    }
    HttpCall {
        method: Method::Get,
        path: "/promotion_codes".to_string(),
        query,
        body: None,
    }
}

fn build_update(input: &PromotionCodesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let params = super::require_params(input.params.as_ref(), "params")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/promotion_codes/{id}"),
        query: Vec::new(),
        body: Some(params),
    })
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model, based on the [`PromotionCodeOp`] that produced it.
pub fn normalize(op: PromotionCodeOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        PromotionCodeOp::List => normalize_list(raw),
        PromotionCodeOp::Create | PromotionCodeOp::Get | PromotionCodeOp::Update => {
            normalize_record(raw)
        }
    }
}

/// Pull `{id,code,coupon,active}` out of a single Stripe promotion code
/// object, defensively — every field falls back to `null` rather than
/// panicking. The `coupon` field may be a nested coupon object or a plain id
/// string, and is passed through as-is.
fn promotion_code_fields(v: &Value) -> Value {
    json!({
        "id": v.get("id").cloned().unwrap_or(Value::Null),
        "code": v.get("code").cloned().unwrap_or(Value::Null),
        "coupon": v.get("coupon").cloned().unwrap_or(Value::Null),
        "active": v.get("active").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-promotion-code response (create/get/update) to
/// `{id,code,coupon,active}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid promotion code response: {err}"))?;
    Ok(promotion_code_fields(&value))
}

/// Normalize a list response (Stripe's `data[]` list shape) to
/// `{total, results:[{id,code,coupon,active}]}`.
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
        .map(promotion_code_fields)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_builds_post_with_coupon_in_body() {
        let call = build_call(r#"{"operation":"create","coupon":"coup_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/promotion_codes");
        assert_eq!(call.body.as_ref().unwrap()["coupon"], "coup_1");
    }

    #[test]
    fn create_merges_params_with_coupon() {
        let call = build_call(
            r#"{"operation":"create","coupon":"coup_1","params":{"code":"SUMMER25","max_redemptions":100}}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/promotion_codes");
        assert_eq!(call.body.as_ref().unwrap()["coupon"], "coup_1");
        assert_eq!(call.body.as_ref().unwrap()["code"], "SUMMER25");
        assert_eq!(call.body.as_ref().unwrap()["max_redemptions"], 100);
    }

    #[test]
    fn create_missing_coupon_names_field() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("coupon"));
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"promo_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/promotion_codes/promo_1");
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
        assert_eq!(call.path, "/promotion_codes");
        assert!(call.query.iter().any(|(k, v)| k == "limit" && v == "10"));
    }

    #[test]
    fn list_builds_get_with_coupon_query() {
        let call = build_call(r#"{"operation":"list","coupon":"coup_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/promotion_codes");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "coupon" && v == "coup_1")
        );
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/promotion_codes");
        assert!(call.query.is_empty());
    }

    #[test]
    fn update_builds_post_with_id_path_and_params_body() {
        let call = build_call(r#"{"operation":"update","id":"promo_1","params":{"active":false}}"#)
            .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/promotion_codes/promo_1");
        assert_eq!(call.body.as_ref().unwrap()["active"], false);
    }

    #[test]
    fn update_missing_id_names_field() {
        let err = build_call(r#"{"operation":"update","params":{"active":false}}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn update_missing_params_names_field() {
        let err = build_call(r#"{"operation":"update","id":"promo_1"}"#).unwrap_err();
        assert!(err.contains("params"));
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw = br#"{"id":"promo_1","code":"SUMMER25","coupon":{"id":"coup_1"},"active":true}"#;
        let out = normalize(PromotionCodeOp::Get, raw).unwrap();
        assert_eq!(out["id"], "promo_1");
        assert_eq!(out["code"], "SUMMER25");
        assert_eq!(out["coupon"]["id"], "coup_1");
        assert_eq!(out["active"], true);
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"promo_1"}"#;
        let out = normalize(PromotionCodeOp::Get, raw).unwrap();
        assert_eq!(out["id"], "promo_1");
        assert_eq!(out["code"], Value::Null);
        assert_eq!(out["coupon"], Value::Null);
        assert_eq!(out["active"], Value::Null);
    }

    #[test]
    fn normalize_list_maps_data_array() {
        let raw = br#"{"data":[{"id":"promo_1","code":"SUMMER25","coupon":"coup_1","active":true}],"has_more":false}"#;
        let out = normalize(PromotionCodeOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "promo_1");
        assert_eq!(out["results"][0]["code"], "SUMMER25");
    }

    #[test]
    fn normalize_list_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(PromotionCodeOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"update","id":"promo_1"}"#),
            Ok(PromotionCodeOp::Update)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
