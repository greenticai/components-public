#![allow(unsafe_op_in_unsafe_fn)]

//! HubSpot CRM for Greentic flows.
//!
//! `greentic.hubspot` ships thirteen tools as a DESIGN extension, reachable to
//! an agentic worker and to nothing else. This is the other half.
//!
//! `hubspot`, `input`, `output`, `auth` and `tool_meta` are the extension's
//! modules, copied verbatim; they were already WIT-free.
//!
//! HubSpot has the most elaborate auth of the fifteen — an `auth_mode` selector
//! over five secrets. A node carries BOTH of its real paths: a Private App
//! token, and a brokerless refresh grant, which needs only HTTP and secrets.
//! Only the broker fallback is absent, because a flow component cannot import
//! `greentic:oauth-broker/broker-v1`.

use serde::Serialize;
use serde_json::Value;

use greentic_types::cbor::canonical;

pub use describe::{SchemaIr, input_schema, output_schema};

mod auth;
mod describe;
mod hubspot;
mod input;
mod ops;
mod output;
mod tool_meta;
mod transport;

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_ID: &str = "hubspot";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every operation this component exposes — a CROSS-REPO contract with the node
/// types in the extension's describe.json.
pub const OPS: &[&str] = &[
    "hubspot_contacts",
    "hubspot_deals",
    "hubspot_companies",
    "hubspot_tickets",
    "hubspot_notes",
    "hubspot_tasks",
    "hubspot_calls",
    "hubspot_meetings",
    "hubspot_emails",
    "hubspot_pipelines",
    "hubspot_owners",
    "hubspot_batch",
    "hubspot_associate",
];

/// Route an operation name to its handler, testable off-wasm.
pub fn dispatch(op: &str, input: &Value) -> Value {
    match op {
        "hubspot_contacts" => ops::hubspot_contacts(input),
        "hubspot_deals" => ops::hubspot_deals(input),
        "hubspot_companies" => ops::hubspot_companies(input),
        "hubspot_tickets" => ops::hubspot_tickets(input),
        "hubspot_notes" => ops::hubspot_notes(input),
        "hubspot_tasks" => ops::hubspot_tasks(input),
        "hubspot_calls" => ops::hubspot_calls(input),
        "hubspot_meetings" => ops::hubspot_meetings(input),
        "hubspot_emails" => ops::hubspot_emails(input),
        "hubspot_pipelines" => ops::hubspot_pipelines(input),
        "hubspot_owners" => ops::hubspot_owners(input),
        "hubspot_batch" => ops::hubspot_batch(input),
        "hubspot_associate" => ops::hubspot_associate(input),
        other => ops::err(format!("unsupported op: {other}")),
    }
}

/// One line per operation, shown wherever the descriptor is rendered.
pub fn op_summary(op: &str) -> &'static str {
    match op {
        "hubspot_contacts" => "Create, read, update, search and list contacts",
        "hubspot_deals" => "Manage deals",
        "hubspot_companies" => "Manage companies",
        "hubspot_tickets" => "Manage tickets",
        "hubspot_notes" => "Manage notes",
        "hubspot_tasks" => "Manage tasks",
        "hubspot_calls" => "Manage logged calls",
        "hubspot_meetings" => "Manage meetings",
        "hubspot_emails" => "Manage logged emails",
        "hubspot_pipelines" => "Read pipelines and their stages",
        "hubspot_owners" => "Read owners",
        "hubspot_batch" => "Batch read or write records",
        "hubspot_associate" => "Associate two records",
        _ => "Unknown operation",
    }
}

/// The QA spec for a component that asks nothing at wizard time.
pub fn empty_qa_spec() -> serde_json::Value {
    serde_json::json!({ "version": 1, "questions": [] })
}

/// Every i18n key referenced by the schemas, in one place.
pub fn i18n_keys() -> Vec<String> {
    [
        "hubspot.schema.input.title",
        "hubspot.schema.input.description",
        "hubspot.schema.input.token.title",
        "hubspot.schema.input.token.description",
        "hubspot.schema.output.title",
        "hubspot.schema.output.description",
        "hubspot.schema.output.ok.title",
        "hubspot.schema.output.ok.description",
        "hubspot.schema.output.error.title",
        "hubspot.schema.output.error.description",
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
            summary: Some("HubSpot CRM for Greentic flows".to_string()),
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
        let out = dispatch("definitely_not_an_op", &json!({}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("unsupported op"));
    }

    #[test]
    fn no_op_panics_on_an_empty_payload() {
        for op in OPS {
            let out = dispatch(op, &json!({}));
            assert_eq!(out["ok"], false, "`{op}` should report, not succeed");
        }
    }

    /// With five possible secrets, the error has to say which combination is
    /// acceptable — naming only one of them would send an operator who has an
    /// OAuth grant looking for a Private App token they do not need.
    #[test]
    fn a_missing_credential_names_both_accepted_paths() {
        let out = dispatch(OPS[0], &json!({}));
        assert_eq!(out["ok"], false);
        let e = out["error"].as_str().unwrap();
        assert!(e.contains("access_token"));
        assert!(e.contains("oauth_refresh_token"));
    }

    #[test]
    fn cbor_round_trips_through_the_canonical_encoder() {
        let value = json!({ "a": 1 });
        let bytes = canonical_cbor_bytes(&value);
        assert_eq!(decode_cbor(&bytes).unwrap(), value);
    }
}
