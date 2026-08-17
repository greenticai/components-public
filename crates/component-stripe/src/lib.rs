#![allow(unsafe_op_in_unsafe_fn)]

//! Stripe billing and payments for Greentic flows.
//!
//! `greentic.stripe` ships thirteen tools as a DESIGN extension, reachable to an
//! agentic worker and to nothing else — the flow runner has no path to
//! `greentic:extension-design/tools`. This is the other half.
//!
//! `auth`, `client`, `tool_meta`, `tools` and every `tools::*` module are the
//! extension's, copied verbatim; they were already WIT-free. Only `transport`
//! and `ops` are new.

use serde::Serialize;
use serde_json::Value;

use greentic_types::cbor::canonical;

pub use describe::{SchemaIr, input_schema, output_schema};

mod auth;
mod client;
mod describe;
mod ops;
mod tool_meta;
mod tools;
mod transport;

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_ID: &str = "stripe";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every operation this component exposes. The strings are a CROSS-REPO
/// contract: each appears here, in the `describe()` op list, and as the
/// `operation` of a node type in the extension's describe.json. The runner
/// dispatches on it and does not default.
pub const OPS: &[&str] = &[
    "stripe_customers",
    "stripe_products",
    "stripe_prices",
    "stripe_payment_links",
    "stripe_invoices",
    "stripe_subscriptions",
    "stripe_checkout_sessions",
    "stripe_refunds",
    "stripe_payment_intents",
    "stripe_disputes",
    "stripe_coupons",
    "stripe_promotion_codes",
    "stripe_files",
];

/// Route an operation name to its handler. Extracted from the WIT layer so it
/// is testable off-wasm; the `node::Guest` impl is a thin wrapper over it.
pub fn dispatch(op: &str, input: &Value) -> Value {
    match op {
        "stripe_customers" => ops::stripe_customers(input),
        "stripe_products" => ops::stripe_products(input),
        "stripe_prices" => ops::stripe_prices(input),
        "stripe_payment_links" => ops::stripe_payment_links(input),
        "stripe_invoices" => ops::stripe_invoices(input),
        "stripe_subscriptions" => ops::stripe_subscriptions(input),
        "stripe_checkout_sessions" => ops::stripe_checkout_sessions(input),
        "stripe_refunds" => ops::stripe_refunds(input),
        "stripe_payment_intents" => ops::stripe_payment_intents(input),
        "stripe_disputes" => ops::stripe_disputes(input),
        "stripe_coupons" => ops::stripe_coupons(input),
        "stripe_promotion_codes" => ops::stripe_promotion_codes(input),
        "stripe_files" => ops::stripe_files(input),
        other => ops::err(format!("unsupported op: {other}")),
    }
}

/// One line per operation, shown wherever the descriptor is rendered.
pub fn op_summary(op: &str) -> &'static str {
    match op {
        "stripe_customers" => "Create, read, update or list customers",
        "stripe_products" => "Manage the product catalogue",
        "stripe_prices" => "Manage prices attached to products",
        "stripe_payment_links" => "Create and manage shareable payment links",
        "stripe_invoices" => "Draft, send and read invoices",
        "stripe_subscriptions" => "Create, update and cancel subscriptions",
        "stripe_checkout_sessions" => "Create and read Checkout sessions",
        "stripe_refunds" => "Issue and read refunds",
        "stripe_payment_intents" => "Create, confirm and read payment intents",
        "stripe_disputes" => "Read and respond to disputes",
        "stripe_coupons" => "Manage coupons",
        "stripe_promotion_codes" => "Manage promotion codes",
        "stripe_files" => "Upload and read files",
        _ => "Unknown operation",
    }
}

/// The QA spec for a component that asks nothing at wizard time. The credential
/// is declared on the node type's `config_schema`, authored per node; asking
/// again here would create a second place to put it with no rule for which wins.
pub fn empty_qa_spec() -> serde_json::Value {
    serde_json::json!({ "version": 1, "questions": [] })
}

/// Every i18n key referenced by the schemas, in one place.
pub fn i18n_keys() -> Vec<String> {
    [
        "stripe.schema.input.title",
        "stripe.schema.input.description",
        "stripe.schema.input.token.title",
        "stripe.schema.input.token.description",
        "stripe.schema.output.title",
        "stripe.schema.output.description",
        "stripe.schema.output.ok.title",
        "stripe.schema.output.ok.description",
        "stripe.schema.output.error.title",
        "stripe.schema.output.error.description",
    ]
    .iter()
    .map(|k| (*k).to_string())
    .collect()
}

pub fn canonical_cbor_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    canonical::to_canonical_cbor(value).unwrap_or_default()
}

pub fn decode_cbor(bytes: &[u8]) -> Result<Value, String> {
    canonical::from_cbor(bytes).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
struct Component;

#[cfg(target_arch = "wasm32")]
impl node::Guest for Component {
    fn describe() -> node::ComponentDescriptor {
        node::ComponentDescriptor {
            name: COMPONENT_ID.to_string(),
            version: COMPONENT_VERSION.to_string(),
            summary: Some("Stripe billing and payments for Greentic flows".to_string()),
            // Declared so the host grants them: every operation makes an HTTPS call
            // and reads a credential.
            capabilities: vec!["host:http".to_string(), "host:secrets".to_string()],
            ops: OPS
                .iter()
                .map(|op| node::Op {
                    name: (*op).to_string(),
                    summary: Some(op_summary(op).to_string()),
                    input: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(
                            &input_schema(),
                        )),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    output: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(
                            &output_schema(),
                        )),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    examples: Vec::new(),
                })
                .collect(),
            schemas: Vec::new(),
            setup: None,
        }
    }

    fn invoke(
        op: String,
        envelope: node::InvocationEnvelope,
    ) -> Result<node::InvocationResult, node::NodeError> {
        // A malformed payload is reported through the envelope, not as a
        // NodeError: the flow can route on `ok == false`, whereas a trap takes
        // the whole run down with a message no operator can act on.
        let input: Value = match decode_cbor(&envelope.payload_cbor) {
            Ok(value) => value,
            Err(err) => {
                return Ok(node::InvocationResult {
                    ok: true,
                    output_cbor: canonical_cbor_bytes(&serde_json::json!({
                        "ok": false,
                        "error": format!("invalid input cbor: {err}")
                    })),
                    output_metadata_cbor: None,
                });
            }
        };

        Ok(node::InvocationResult {
            ok: true,
            output_cbor: canonical_cbor_bytes(&dispatch(&op, &input)),
            output_metadata_cbor: None,
        })
    }
}

#[cfg(target_arch = "wasm32")]
// Exports the four `greentic:component/node@0.6.0` symbols the runner looks
// for. Without this the wasm builds cleanly and the runner rejects it with
// "component exports neither node@0.5/0.4 nor component-runtime@0.6" — a
// failure that only appears at execution time.
greentic_interfaces_guest::export_component_v060!(Component);

/// The `component-v0-v6-v0` world exports `component-qa` and `component-i18n`
/// alongside `node`, so a component that implements only `node` fails to build
/// with "failed to find export of interface `greentic:component/component-qa`".
/// Every component in this repo satisfies them through this same inline-WIT
/// module.
///
/// Transform has no configuration and no secrets, so there is genuinely nothing
/// to ask: the QA spec is empty and `apply-answers` returns the config it was
/// handed. That is the honest answer, not a stub — inventing questions here
/// would put a setup step in front of an operation that needs none.
#[cfg(target_arch = "wasm32")]
mod qa_exports {
    wit_bindgen::generate!({
        inline: r#"
            package greentic:component@0.6.0;

            interface component-qa {
              enum qa-mode {
                default,
                setup,
                update,
                remove,
              }

              qa-spec: func(mode: qa-mode) -> list<u8>;
              apply-answers: func(mode: qa-mode, current-config: list<u8>, answers: list<u8>) -> list<u8>;
            }

            interface component-i18n {
              i18n-keys: func() -> list<string>;
            }

            world wizard-support {
              export component-qa;
              export component-i18n;
            }
        "#,
        world: "wizard-support",
    });

    pub struct WizardSupport;

    impl exports::greentic::component::component_qa::Guest for WizardSupport {
        fn qa_spec(_mode: exports::greentic::component::component_qa::QaMode) -> Vec<u8> {
            crate::canonical_cbor_bytes(&crate::empty_qa_spec())
        }

        fn apply_answers(
            _mode: exports::greentic::component::component_qa::QaMode,
            current_config: Vec<u8>,
            _answers: Vec<u8>,
        ) -> Vec<u8> {
            // Nothing is asked, so nothing can be applied: hand back the config
            // unchanged rather than an empty one, which would silently discard
            // whatever the caller already had.
            current_config
        }
    }

    impl exports::greentic::component::component_i18n::Guest for WizardSupport {
        fn i18n_keys() -> Vec<String> {
            crate::i18n_keys()
        }
    }

    export!(WizardSupport with_types_in self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_declared_op_dispatches_to_a_real_handler() {
        for op in OPS {
            let out = dispatch(op, &json!({}));
            assert!(
                !out["error"]
                    .as_str()
                    .unwrap_or("")
                    .contains("unsupported op"),
                "`{op}` has no dispatch arm"
            );
        }
    }

    #[test]
    fn every_declared_op_has_a_summary() {
        for op in OPS {
            assert_ne!(op_summary(op), "Unknown operation", "`{op}` has no summary");
        }
    }

    #[test]
    fn an_unknown_op_is_reported_not_swallowed() {
        let out = dispatch("stripe_wire_me_money", &json!({}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("unsupported op"));
    }

    /// Missing arguments are a VALUE a flow can route on, never a panic. All
    /// thirteen, because one handler unwrapping turns a bad payload into a dead
    /// run — and these operations move money.
    #[test]
    fn no_op_panics_on_an_empty_payload() {
        for op in OPS {
            let out = dispatch(op, &json!({}));
            assert_eq!(out["ok"], false, "`{op}` should report, not succeed");
        }
    }

    #[test]
    fn a_missing_secret_key_is_named_first() {
        let out = dispatch("stripe_customers", &json!({"operation": "list"}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("secret_key"));
    }

    #[test]
    fn cbor_round_trips_through_the_canonical_encoder() {
        let value = json!({ "a": 1 });
        let bytes = canonical_cbor_bytes(&value);
        assert_eq!(decode_cbor(&bytes).unwrap(), value);
    }
}
