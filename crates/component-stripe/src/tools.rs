//! Pure, host-testable Stripe tool domains (customers, products, prices,
//! payment links, …). Each submodule maps a group of `stripe_*` tool
//! operations to `client::HttpCall` requests and normalizes the corresponding
//! Stripe REST responses. No WIT imports live here — the WIT `invoke_tool`
//! dispatch in `lib.rs` calls into these modules.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
pub mod checkout_sessions;
pub mod coupons;
pub mod customers;
pub mod disputes;
pub mod files;
pub mod invoices;
pub mod payment_intents;
pub mod payment_links;
pub mod prices;
pub mod products;
pub mod promotion_codes;
pub mod refunds;
pub mod subscriptions;

/// Fetch a required string field, rejecting `None` and the empty string.
/// Shared by every `tools::*` domain's `build_call`.
pub(crate) fn require_field<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(format!("missing required field: {name}")),
    }
}

/// Fetch a required JSON object/value field (Stripe's `params` pass-through),
/// rejecting `None` and `Value::Null`. Shared by every `tools::*` domain's
/// `build_call` for create/update operations whose body is a model-supplied
/// params object.
pub(crate) fn require_params(
    value: Option<&serde_json::Value>,
    name: &str,
) -> Result<serde_json::Value, String> {
    match value {
        Some(v) if !v.is_null() => Ok(v.clone()),
        _ => Err(format!("missing required field: {name}")),
    }
}
