//! `stripe_subscriptions` tool domain — pure HTTP-call building and response
//! normalization for Stripe subscription operations (create/get/list/update/
//! cancel). No WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`. See [`crate::tools::customers`] for the
//! template this domain follows; the `customer`/`items` + `params` merge on
//! `create` follows [`crate::tools::payment_links`]'s
//! dedicated-field-merged-into-body shape.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Stripe subscription operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionOp {
    Create,
    Get,
    List,
    Update,
    Cancel,
}

/// Raw `stripe_subscriptions` tool input, deserialized from the
/// model-supplied `args_json`.
#[derive(Debug, Deserialize)]
struct SubscriptionsInput {
    operation: SubscriptionOp,
    #[serde(default)]
    id: Option<String>,
    /// Customer id to subscribe. Required for `create`; also an optional
    /// list filter.
    #[serde(default)]
    customer: Option<String>,
    /// Subscription items, an array of `{price}` objects. Required for
    /// `create`.
    #[serde(default)]
    items: Option<Value>,
    /// Additional pass-through body fields: merged with `customer`/`items`
    /// on `create`; the full update body on `update`.
    #[serde(default)]
    params: Option<Value>,
    /// List filter, e.g. `"active"`, `"canceled"`, `"past_due"`.
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_subscriptions`
/// invocation.
///
/// Parses `args_json` into a [`SubscriptionsInput`], validates the fields
/// required by the selected [`SubscriptionOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: SubscriptionsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        SubscriptionOp::Create => build_create(&input),
        SubscriptionOp::Get => build_get(&input),
        SubscriptionOp::List => Ok(build_list(&input)),
        SubscriptionOp::Update => build_update(&input),
        SubscriptionOp::Cancel => build_cancel(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<SubscriptionOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: SubscriptionOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

/// Build the create body: `params` (if any, and an object) as the base, with
/// `customer` and `items` inserted/overwritten on top — always the values
/// the caller supplied, even if `params` also carried stale keys of those
/// names.
fn build_create(input: &SubscriptionsInput) -> Result<HttpCall, String> {
    let customer = super::require_field(input.customer.as_deref(), "customer")?.to_string();
    let items = match &input.items {
        Some(value) if !value.is_null() => value.clone(),
        _ => return Err("missing required field: items".to_string()),
    };
    let mut body = match &input.params {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    body.insert("customer".to_string(), Value::String(customer));
    body.insert("items".to_string(), items);
    Ok(HttpCall {
        method: Method::Post,
        path: "/subscriptions".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_get(input: &SubscriptionsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/subscriptions/{id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_list(input: &SubscriptionsInput) -> HttpCall {
    let mut query = Vec::new();
    if let Some(customer) = &input.customer {
        query.push(("customer".to_string(), customer.clone()));
    }
    if let Some(status) = &input.status {
        query.push(("status".to_string(), status.clone()));
    }
    if let Some(limit) = input.limit {
        query.push(("limit".to_string(), limit.to_string()));
    }
    HttpCall {
        method: Method::Get,
        path: "/subscriptions".to_string(),
        query,
        body: None,
    }
}

fn build_update(input: &SubscriptionsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let params = super::require_params(input.params.as_ref(), "params")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/subscriptions/{id}"),
        query: Vec::new(),
        body: Some(params),
    })
}

fn build_cancel(input: &SubscriptionsInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Delete,
        path: format!("/subscriptions/{id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model, based on the [`SubscriptionOp`] that produced it. `cancel` returns
/// the (canceled) subscription object, so it is normalized as a record like
/// the others.
pub fn normalize(op: SubscriptionOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        SubscriptionOp::List => normalize_list(raw),
        SubscriptionOp::Create
        | SubscriptionOp::Get
        | SubscriptionOp::Update
        | SubscriptionOp::Cancel => normalize_record(raw),
    }
}

/// Pull `{id,customer,status,current_period_end}` out of a single Stripe
/// subscription object, defensively — every field falls back to `null`
/// rather than panicking.
fn subscription_fields(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "customer": value.get("customer").cloned().unwrap_or(Value::Null),
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "current_period_end": value.get("current_period_end").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-subscription response (create/get/update/cancel) to
/// `{id,customer,status,current_period_end}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|err| format!("invalid subscription response: {err}"))?;
    Ok(subscription_fields(&value))
}

/// Normalize a list response (Stripe's `data[]` list shape) to
/// `{total, results:[{id,customer,status,current_period_end}]}`. `total` is
/// the count of mapped `results`, not a value read from the response body.
fn normalize_list(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid list response: {err}"))?;
    let results: Vec<Value> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(subscription_fields)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_builds_post_with_customer_and_items_body() {
        let call = build_call(
            r#"{"operation":"create","customer":"cus_1","items":[{"price":"price_1"}]}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/subscriptions");
        assert_eq!(call.body.as_ref().unwrap()["customer"], "cus_1");
        assert_eq!(call.body.as_ref().unwrap()["items"][0]["price"], "price_1");
    }

    #[test]
    fn create_merges_customer_and_items_into_params() {
        let call = build_call(
            r#"{"operation":"create","customer":"cus_1","items":[{"price":"price_1"}],"params":{"trial_period_days":14}}"#,
        )
        .unwrap();
        let body = call.body.unwrap();
        assert_eq!(body["customer"], "cus_1");
        assert_eq!(body["items"][0]["price"], "price_1");
        assert_eq!(body["trial_period_days"], 14);
    }

    #[test]
    fn create_missing_customer_names_field() {
        let err =
            build_call(r#"{"operation":"create","items":[{"price":"price_1"}]}"#).unwrap_err();
        assert!(err.contains("customer"));
    }

    #[test]
    fn create_missing_items_names_field() {
        let err = build_call(r#"{"operation":"create","customer":"cus_1"}"#).unwrap_err();
        assert!(err.contains("items"));
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"sub_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/subscriptions/sub_1");
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn list_builds_get_with_customer_status_limit_query() {
        let call =
            build_call(r#"{"operation":"list","customer":"cus_1","status":"active","limit":5}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/subscriptions");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "customer" && v == "cus_1")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "status" && v == "active")
        );
        assert!(call.query.iter().any(|(k, v)| k == "limit" && v == "5"));
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(call.query.is_empty());
    }

    #[test]
    fn update_builds_post_with_id_and_params() {
        let call = build_call(
            r#"{"operation":"update","id":"sub_1","params":{"cancel_at_period_end":true}}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/subscriptions/sub_1");
        assert_eq!(call.body.as_ref().unwrap()["cancel_at_period_end"], true);
    }

    #[test]
    fn update_missing_params_names_field() {
        let err = build_call(r#"{"operation":"update","id":"sub_1"}"#).unwrap_err();
        assert!(err.contains("params"));
    }

    #[test]
    fn update_missing_id_names_field() {
        let err = build_call(r#"{"operation":"update","params":{"a":1}}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn cancel_builds_delete_with_id_path() {
        let call = build_call(r#"{"operation":"cancel","id":"sub_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Delete));
        assert_eq!(call.path, "/subscriptions/sub_1");
        assert!(call.body.is_none());
    }

    #[test]
    fn cancel_missing_id_names_field() {
        let err = build_call(r#"{"operation":"cancel"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw =
            br#"{"id":"sub_1","customer":"cus_1","status":"active","current_period_end":123}"#;
        let out = normalize(SubscriptionOp::Get, raw).unwrap();
        assert_eq!(out["id"], "sub_1");
        assert_eq!(out["customer"], "cus_1");
        assert_eq!(out["status"], "active");
        assert_eq!(out["current_period_end"], 123);
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"sub_1"}"#;
        let out = normalize(SubscriptionOp::Create, raw).unwrap();
        assert_eq!(out["id"], "sub_1");
        assert_eq!(out["customer"], Value::Null);
        assert_eq!(out["status"], Value::Null);
        assert_eq!(out["current_period_end"], Value::Null);
    }

    #[test]
    fn normalize_cancel_returns_canceled_record() {
        let raw = br#"{"id":"sub_1","status":"canceled"}"#;
        let out = normalize(SubscriptionOp::Cancel, raw).unwrap();
        assert_eq!(out["id"], "sub_1");
        assert_eq!(out["status"], "canceled");
    }

    #[test]
    fn normalize_list_maps_data_array() {
        let raw =
            br#"{"data":[{"id":"sub_1","customer":"cus_1","status":"active"}],"has_more":false}"#;
        let out = normalize(SubscriptionOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "sub_1");
        assert_eq!(out["results"][0]["customer"], "cus_1");
    }

    #[test]
    fn normalize_list_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(SubscriptionOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"cancel","id":"sub_1"}"#),
            Ok(SubscriptionOp::Cancel)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
