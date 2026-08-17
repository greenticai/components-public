//! `stripe_customers` tool domain — pure HTTP-call building and response
//! normalization for Stripe customer operations (create/search/get/update/
//! delete). No WIT imports — this module is fully host-testable; the actual
//! `extension-host/http` invocation and `describe()` tool metadata live in
//! `lib.rs` / `tool_meta.rs`.
//!
//! This is the template the other three Batch-1 Stripe tool domains
//! (products, prices, payment_links) copy: keep the shape `CustomerOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary. POST
//! bodies are returned as a plain JSON `Value` — `lib.rs`'s `stripe_send`
//! converts it to Stripe's `application/x-www-form-urlencoded` body shape.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::{HttpCall, Method};

/// Stripe customer operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomerOp {
    Create,
    Search,
    Get,
    Update,
    Delete,
}

/// Raw `stripe_customers` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct CustomersInput {
    operation: CustomerOp,
    #[serde(default)]
    id: Option<String>,
    /// Pass-through create/update body (e.g. `email`, `name`, `description`,
    /// `metadata`). Required for `create` and `update`.
    #[serde(default)]
    params: Option<Value>,
    /// Stripe search-query string. Required for `search`.
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_customers` invocation.
///
/// Parses `args_json` into a [`CustomersInput`], validates the fields
/// required by the selected [`CustomerOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: CustomersInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        CustomerOp::Create => build_create(&input),
        CustomerOp::Search => build_search(&input),
        CustomerOp::Get => build_get(&input),
        CustomerOp::Update => build_update(&input),
        CustomerOp::Delete => build_delete(&input),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<CustomerOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: CustomerOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &CustomersInput) -> Result<HttpCall, String> {
    let params = super::require_params(input.params.as_ref(), "params")?;
    Ok(HttpCall {
        method: Method::Post,
        path: "/customers".to_string(),
        query: Vec::new(),
        body: Some(params),
    })
}

fn build_search(input: &CustomersInput) -> Result<HttpCall, String> {
    let query = super::require_field(input.query.as_deref(), "query")?.to_string();
    let mut params = vec![("query".to_string(), query)];
    if let Some(limit) = input.limit {
        params.push(("limit".to_string(), limit.to_string()));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: "/customers/search".to_string(),
        query: params,
        body: None,
    })
}

fn build_get(input: &CustomersInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/customers/{id}"),
        query: Vec::new(),
        body: None,
    })
}

fn build_update(input: &CustomersInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    let params = super::require_params(input.params.as_ref(), "params")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/customers/{id}"),
        query: Vec::new(),
        body: Some(params),
    })
}

fn build_delete(input: &CustomersInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Delete,
        path: format!("/customers/{id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model, based on the [`CustomerOp`] that produced it.
pub fn normalize(op: CustomerOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        CustomerOp::Search => normalize_search(raw),
        CustomerOp::Create | CustomerOp::Get | CustomerOp::Update => normalize_record(raw),
        CustomerOp::Delete => Ok(normalize_delete(raw)),
    }
}

/// Pull `{id,email,name,created}` out of a single Stripe customer object,
/// defensively — every field falls back to `null` rather than panicking.
fn customer_fields(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "email": value.get("email").cloned().unwrap_or(Value::Null),
        "name": value.get("name").cloned().unwrap_or(Value::Null),
        "created": value.get("created").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-customer response (create/get/update) to
/// `{id,email,name,created}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid customer response: {err}"))?;
    Ok(customer_fields(&value))
}

/// Normalize a search response (Stripe's `data[]` list shape) to
/// `{total, results:[{id,email,name,created}]}`. `total` is the count of
/// mapped `results`, not a value read from the response body (Stripe list
/// responses paginate via `has_more`, not a top-level `total`).
fn normalize_search(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid search response: {err}"))?;
    let results: Vec<Value> = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(customer_fields)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

/// Normalize a delete response. Stripe's `DELETE /customers/{id}` returns
/// `{"id":..., "object":"customer", "deleted":true}`; `id` is read from the
/// body when present, falling back to `null` if the body is empty/malformed.
fn normalize_delete(raw: &[u8]) -> Value {
    let id = serde_json::from_slice::<Value>(raw)
        .ok()
        .and_then(|value| value.get("id").cloned())
        .unwrap_or(Value::Null);
    json!({ "ok": true, "id": id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_builds_post_with_params_body() {
        let call =
            build_call(r#"{"operation":"create","params":{"email":"a@b.com","name":"Ada"}}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/customers");
        assert_eq!(call.body.as_ref().unwrap()["email"], "a@b.com");
        assert_eq!(call.body.as_ref().unwrap()["name"], "Ada");
    }

    #[test]
    fn create_missing_params_names_field() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("params"));
    }

    #[test]
    fn search_builds_get_with_query_and_limit() {
        let call =
            build_call(r#"{"operation":"search","query":"email:'a@b.com'","limit":5}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/customers/search");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "query" && v == "email:'a@b.com'")
        );
        assert!(call.query.iter().any(|(k, v)| k == "limit" && v == "5"));
    }

    #[test]
    fn search_missing_query_names_field() {
        let err = build_call(r#"{"operation":"search"}"#).unwrap_err();
        assert!(err.contains("query"));
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"cus_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/customers/cus_1");
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn update_builds_post_with_id_and_params() {
        let call = build_call(r#"{"operation":"update","id":"cus_1","params":{"name":"Updated"}}"#)
            .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/customers/cus_1");
        assert_eq!(call.body.as_ref().unwrap()["name"], "Updated");
    }

    #[test]
    fn update_missing_params_names_field() {
        let err = build_call(r#"{"operation":"update","id":"cus_1"}"#).unwrap_err();
        assert!(err.contains("params"));
    }

    #[test]
    fn delete_builds_delete_with_id_path() {
        let call = build_call(r#"{"operation":"delete","id":"cus_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Delete));
        assert_eq!(call.path, "/customers/cus_1");
        assert!(call.body.is_none());
    }

    #[test]
    fn delete_missing_id_names_field() {
        let err = build_call(r#"{"operation":"delete"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw = br#"{"id":"cus_1","email":"a@b.com","name":"Ada","created":123}"#;
        let out = normalize(CustomerOp::Get, raw).unwrap();
        assert_eq!(out["id"], "cus_1");
        assert_eq!(out["email"], "a@b.com");
        assert_eq!(out["name"], "Ada");
        assert_eq!(out["created"], 123);
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"cus_1"}"#;
        let out = normalize(CustomerOp::Create, raw).unwrap();
        assert_eq!(out["id"], "cus_1");
        assert_eq!(out["email"], Value::Null);
        assert_eq!(out["name"], Value::Null);
        assert_eq!(out["created"], Value::Null);
    }

    #[test]
    fn normalize_search_maps_data_array() {
        let raw = br#"{"object":"search_result","data":[{"id":"cus_1","email":"a@b.com"}],"has_more":false}"#;
        let out = normalize(CustomerOp::Search, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "cus_1");
        assert_eq!(out["results"][0]["email"], "a@b.com");
    }

    #[test]
    fn normalize_search_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(CustomerOp::Search, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_delete_extracts_id() {
        let raw = br#"{"id":"cus_1","object":"customer","deleted":true}"#;
        let out = normalize(CustomerOp::Delete, raw).unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], "cus_1");
    }

    #[test]
    fn normalize_delete_handles_empty_body() {
        let out = normalize(CustomerOp::Delete, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"delete","id":"cus_1"}"#),
            Ok(CustomerOp::Delete)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
