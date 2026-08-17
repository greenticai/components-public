#![allow(unsafe_op_in_unsafe_fn)]

//! Calendly scheduling for Greentic flows.
//!
//! `greentic.calendly` ships its tools as a DESIGN extension, reachable to an
//! agentic worker and to nothing else — the flow runner has no path to
//! `greentic:extension-design/tools`. This is the other half.
//!
//! `auth`, `client`, `tool_meta`, `tools` and every `tools::*` module are the
//! extension's, copied verbatim; they were already WIT-free. Only `transport`
//! and `ops` are new.
//!
//! ONE capability is deliberately not carried over: the extension's `auth_mode`
//! secret can route token resolution through
//! `greentic:oauth-broker/broker-v1`, and a flow component cannot import that
//! world. The static-credential path — the extension's own default when
//! `auth_mode` is unset — is what a node offers. The broker path stays
//! available to an agentic worker through the tool surface.

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
const COMPONENT_ID: &str = "calendly";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every operation this component exposes. The strings are a CROSS-REPO
/// contract: each appears here, in the `describe()` op list, and as the
/// `operation` of a node type in the extension's describe.json.
pub const OPS: &[&str] = &[
    "calendly_me",
    "calendly_event_types",
    "calendly_events",
    "calendly_invitees",
    "calendly_scheduling_links",
    "calendly_availability",
    "calendly_webhooks",
];

/// Route an operation name to its handler. Extracted from the WIT layer so it
/// is testable off-wasm; the `node::Guest` impl is a thin wrapper over it.
pub fn dispatch(op: &str, input: &Value) -> Value {
    match op {
        "calendly_me" => ops::calendly_me(input),
        "calendly_event_types" => ops::calendly_event_types(input),
        "calendly_events" => ops::calendly_events(input),
        "calendly_invitees" => ops::calendly_invitees(input),
        "calendly_scheduling_links" => ops::calendly_scheduling_links(input),
        "calendly_availability" => ops::calendly_availability(input),
        "calendly_webhooks" => ops::calendly_webhooks(input),
        other => ops::err(format!("unsupported op: {other}")),
    }
}

/// One line per operation, shown wherever the descriptor is rendered.
pub fn op_summary(op: &str) -> &'static str {
    match op {
        "calendly_me" => "Read the authenticated user",
        "calendly_event_types" => "List and read event types",
        "calendly_events" => "List and read scheduled events",
        "calendly_invitees" => "Read and manage event invitees",
        "calendly_scheduling_links" => "Create single-use scheduling links",
        "calendly_availability" => "Read availability schedules",
        "calendly_webhooks" => "Manage webhook subscriptions",
        _ => "Unknown operation",
    }
}

/// The QA spec for a component that asks nothing at wizard time. The credential
/// is declared on the node type's `config_schema`, authored per node.
pub fn empty_qa_spec() -> serde_json::Value {
    serde_json::json!({ "version": 1, "questions": [] })
}

/// Every i18n key referenced by the schemas, in one place.
pub fn i18n_keys() -> Vec<String> {
    [
        "calendly.schema.input.title",
        "calendly.schema.input.description",
        "calendly.schema.input.token.title",
        "calendly.schema.input.token.description",
        "calendly.schema.output.title",
        "calendly.schema.output.description",
        "calendly.schema.output.ok.title",
        "calendly.schema.output.ok.description",
        "calendly.schema.output.error.title",
        "calendly.schema.output.error.description",
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
            summary: Some("Calendly scheduling for Greentic flows".to_string()),
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

    /// Missing arguments are a VALUE a flow can route on, never a panic.
    #[test]
    fn no_op_panics_on_an_empty_payload() {
        for op in OPS {
            let out = dispatch(op, &json!({}));
            assert_eq!(out["ok"], false, "`{op}` should report, not succeed");
        }
    }

    /// The credential is required BEFORE anything else is validated — an
    /// operator who forgot it should be told that, not given a field complaint.
    #[test]
    fn a_missing_credential_is_named_first() {
        let out = dispatch(OPS[0], &json!({}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("token"));
    }

    #[test]
    fn cbor_round_trips_through_the_canonical_encoder() {
        let value = json!({ "a": 1 });
        let bytes = canonical_cbor_bytes(&value);
        assert_eq!(decode_cbor(&bytes).unwrap(), value);
    }
}
