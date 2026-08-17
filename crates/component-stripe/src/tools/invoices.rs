//! `stripe_invoices` tool domain — pure HTTP-call building and response
//! normalization for Stripe invoice operations (create/list/get/finalize/
//! send/void). No WIT imports — this module is fully host-testable; the
//! actual `extension-host/http` invocation and `describe()` tool metadata
//! live in `lib.rs` / `tool_meta.rs`. See [`crate::tools::customers`] for the
//! template this domain follows; the `customer` + `params` merge on `create`
//! follows [`crate::tools::payment_links`]'s dedicated-field-merged-into-body
//! shape.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Stripe invoice operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceOp {
    Create,
    List,
    Get,
    Finalize,
    Send,
    Void,
}

/// Raw `stripe_invoices` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct InvoicesInput {
    operation: InvoiceOp,
    #[serde(default)]
    id: Option<String>,
    /// Customer id to bill. Required for `create`; also an optional list
    /// filter.
    #[serde(default)]
    customer: Option<String>,
    /// Additional pass-through create body fields, merged with `customer`.
    /// Optional.
    #[serde(default)]
    params: Option<Value>,
    /// List filter, e.g. `"draft"`, `"open"`, `"paid"`.
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

/// Build the Stripe REST [`HttpCall`] for a `stripe_invoices` invocation.
///
/// Parses `args_json` into an [`InvoicesInput`], validates the fields
/// required by the selected [`InvoiceOp`], and returns the resulting
/// request. On missing input or a missing required field, returns `Err`
/// naming the field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: InvoicesInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        InvoiceOp::Create => build_create(&input),
        InvoiceOp::List => Ok(build_list(&input)),
        InvoiceOp::Get => build_get(&input),
        InvoiceOp::Finalize => build_action(&input, "finalize"),
        InvoiceOp::Send => build_action(&input, "send"),
        InvoiceOp::Void => build_action(&input, "void"),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<InvoiceOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: InvoiceOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

/// Build the create body: `params` (if any, and an object) as the base, with
/// `customer` inserted/overwritten on top — `customer` is always the value
/// the caller supplied, even if `params` also carried a stale key of that
/// name.
fn build_create(input: &InvoicesInput) -> Result<HttpCall, String> {
    let customer = super::require_field(input.customer.as_deref(), "customer")?.to_string();
    let mut body = match &input.params {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    body.insert("customer".to_string(), Value::String(customer));
    Ok(HttpCall {
        method: Method::Post,
        path: "/invoices".to_string(),
        query: Vec::new(),
        body: Some(Value::Object(body)),
    })
}

fn build_list(input: &InvoicesInput) -> HttpCall {
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
        path: "/invoices".to_string(),
        query,
        body: None,
    }
}

fn build_get(input: &InvoicesInput) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/invoices/{id}"),
        query: Vec::new(),
        body: None,
    })
}

/// Build a `POST /invoices/{id}/{action}` request (finalize/send/void).
/// Stripe accepts no required body params for any of these three actions.
fn build_action(input: &InvoicesInput, action: &str) -> Result<HttpCall, String> {
    let id = super::require_field(input.id.as_deref(), "id")?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/invoices/{id}/{action}"),
        query: Vec::new(),
        body: None,
    })
}

/// Map a raw Stripe REST response body to the compact shape returned to the
/// model, based on the [`InvoiceOp`] that produced it.
pub fn normalize(op: InvoiceOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        InvoiceOp::List => normalize_list(raw),
        InvoiceOp::Create
        | InvoiceOp::Get
        | InvoiceOp::Finalize
        | InvoiceOp::Send
        | InvoiceOp::Void => normalize_record(raw),
    }
}

/// Pull `{id,customer,status,total,hosted_invoice_url}` out of a single
/// Stripe invoice object, defensively — every field falls back to `null`
/// rather than panicking.
fn invoice_fields(value: &Value) -> Value {
    json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "customer": value.get("customer").cloned().unwrap_or(Value::Null),
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "total": value.get("total").cloned().unwrap_or(Value::Null),
        "hosted_invoice_url": value.get("hosted_invoice_url").cloned().unwrap_or(Value::Null),
    })
}

/// Normalize a single-invoice response (create/get/finalize/send/void) to
/// `{id,customer,status,total,hosted_invoice_url}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid invoice response: {err}"))?;
    Ok(invoice_fields(&value))
}

/// Normalize a list response (Stripe's `data[]` list shape) to
/// `{total, results:[{id,customer,status,total,hosted_invoice_url}]}`.
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
        .map(invoice_fields)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_builds_post_with_customer_body() {
        let call = build_call(r#"{"operation":"create","customer":"cus_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/invoices");
        assert_eq!(call.body.as_ref().unwrap()["customer"], "cus_1");
    }

    #[test]
    fn create_merges_customer_into_params() {
        let call = build_call(
            r#"{"operation":"create","customer":"cus_1","params":{"auto_advance":false}}"#,
        )
        .unwrap();
        let body = call.body.unwrap();
        assert_eq!(body["customer"], "cus_1");
        assert_eq!(body["auto_advance"], false);
    }

    #[test]
    fn create_missing_customer_names_field() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("customer"));
    }

    #[test]
    fn list_builds_get_with_customer_status_limit_query() {
        let call =
            build_call(r#"{"operation":"list","customer":"cus_1","status":"open","limit":5}"#)
                .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/invoices");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "customer" && v == "cus_1")
        );
        assert!(call.query.iter().any(|(k, v)| k == "status" && v == "open"));
        assert!(call.query.iter().any(|(k, v)| k == "limit" && v == "5"));
    }

    #[test]
    fn list_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"list"}"#).unwrap();
        assert!(call.query.is_empty());
    }

    #[test]
    fn get_builds_get_with_id_path() {
        let call = build_call(r#"{"operation":"get","id":"in_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/invoices/in_1");
    }

    #[test]
    fn get_missing_id_names_field() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn finalize_builds_post_with_sub_path() {
        let call = build_call(r#"{"operation":"finalize","id":"in_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/invoices/in_1/finalize");
        assert!(call.body.is_none());
    }

    #[test]
    fn finalize_missing_id_names_field() {
        let err = build_call(r#"{"operation":"finalize"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn send_builds_post_with_sub_path() {
        let call = build_call(r#"{"operation":"send","id":"in_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/invoices/in_1/send");
    }

    #[test]
    fn send_missing_id_names_field() {
        let err = build_call(r#"{"operation":"send"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn void_builds_post_with_sub_path() {
        let call = build_call(r#"{"operation":"void","id":"in_1"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/invoices/in_1/void");
    }

    #[test]
    fn void_missing_id_names_field() {
        let err = build_call(r#"{"operation":"void"}"#).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn normalize_record_extracts_fields() {
        let raw = br#"{"id":"in_1","customer":"cus_1","status":"open","total":1000,"hosted_invoice_url":"https://x"}"#;
        let out = normalize(InvoiceOp::Get, raw).unwrap();
        assert_eq!(out["id"], "in_1");
        assert_eq!(out["customer"], "cus_1");
        assert_eq!(out["status"], "open");
        assert_eq!(out["total"], 1000);
        assert_eq!(out["hosted_invoice_url"], "https://x");
    }

    #[test]
    fn normalize_record_handles_missing_fields_without_panicking() {
        let raw = br#"{"id":"in_1"}"#;
        let out = normalize(InvoiceOp::Create, raw).unwrap();
        assert_eq!(out["id"], "in_1");
        assert_eq!(out["customer"], Value::Null);
        assert_eq!(out["status"], Value::Null);
        assert_eq!(out["total"], Value::Null);
        assert_eq!(out["hosted_invoice_url"], Value::Null);
    }

    #[test]
    fn normalize_finalize_send_void_extract_record() {
        let raw = br#"{"id":"in_1","status":"void"}"#;
        assert_eq!(
            normalize(InvoiceOp::Finalize, raw).unwrap()["status"],
            "void"
        );
        assert_eq!(normalize(InvoiceOp::Send, raw).unwrap()["status"], "void");
        assert_eq!(normalize(InvoiceOp::Void, raw).unwrap()["status"], "void");
    }

    #[test]
    fn normalize_list_maps_data_array() {
        let raw =
            br#"{"data":[{"id":"in_1","customer":"cus_1","status":"open"}],"has_more":false}"#;
        let out = normalize(InvoiceOp::List, raw).unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["results"][0]["id"], "in_1");
        assert_eq!(out["results"][0]["customer"], "cus_1");
    }

    #[test]
    fn normalize_list_handles_empty_data() {
        let raw = br#"{"data":[]}"#;
        let out = normalize(InvoiceOp::List, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"void","id":"in_1"}"#),
            Ok(InvoiceOp::Void)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
