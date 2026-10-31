#![allow(unsafe_op_in_unsafe_fn)]

//! `done-ops`: the flow-node half of the `greentic.done` extension.
//!
//! Two operations, exported through `greentic:component/node@0.6`:
//!
//! - `create_request`: generates a UUID v7, POSTs to the Done Core's
//!   `done_api_create` webhook trigger over HTTPS, answers `{ok, id, state}`.
//! - `read_request`: asks the Core's `done-reader` A2A agent for a request's
//!   state (a webhook trigger returns no body, so a read cannot be one).
//!
//! Failure is a value, `{ok:false, error:{code, message}}`, so a flow routes it
//! `on_error` only when it draws that edge; this component never traps on bad
//! input. See `ops` for the credential field names and why.

use serde::Serialize;
use serde_json::Value;

use greentic_types::cbor::canonical;

pub use describe::{SchemaIr, input_schema, output_schema};

mod describe;
pub mod env;
pub mod events;
pub mod mac;
pub mod ops;
pub mod remind;
pub mod token;
pub mod uuid7;
pub mod validate;

#[cfg(all(target_arch = "wasm32", feature = "host"))]
use greentic_interfaces_guest::component_v0_6::node;

#[cfg_attr(not(all(target_arch = "wasm32", feature = "host")), allow(dead_code))]
const COMPONENT_ID: &str = "done-ops";
#[cfg_attr(not(all(target_arch = "wasm32", feature = "host")), allow(dead_code))]
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every operation. CROSS-REPO contract: each string appears here, in
/// `describe()`, and as the `operation` of a node type in the extension's
/// describe.json. The runner dispatches on it and does not default.
pub const OPS: &[&str] = &[
    "done_api",
    "create_request",
    "read_request",
    "verify_token",
    "fanout_reminders",
    "plan_reminder",
    "deliver_reminder",
    "deliver_events",
];

/// Which of the two behaviours a `done_api` call means.
///
/// The designer wires a node's credentials by joining the node type's
/// `operation` to the tool of the same name, so both node types carry
/// `operation: "done_api"` and must say which behaviour they want in their own
/// config (`action`, a constant on each node type). `action` is the contract.
/// The fallback is for a form that did not write the default: `to` or `request`
/// can only mean create, an `id` alone can only mean read, and anything else is
/// refused rather than guessed.
pub fn action_of(input: &Value) -> Option<&'static str> {
    match input.get("action").and_then(Value::as_str) {
        Some("create_request") => Some("create_request"),
        Some("read_request") => Some("read_request"),
        Some(_) => None,
        None if input.get("to").is_some() || input.get("request").is_some() => {
            Some("create_request")
        }
        None if input.get("id").is_some() => Some("read_request"),
        None => None,
    }
}

/// Route an operation to its handler (off-wasm testable).
pub fn dispatch(env: &dyn env::Env, op: &str, input: &Value) -> Value {
    match op {
        "done_api" => match action_of(input) {
            Some("create_request") => ops::create_request(env, input),
            Some("read_request") => ops::read_request(env, input),
            _ => ops::err(
                "invalid_request",
                "`action` must be create_request or read_request",
            ),
        },
        "create_request" => ops::create_request(env, input),
        "read_request" => ops::read_request(env, input),
        "verify_token" => token::verify_token(env, input),
        "fanout_reminders" => remind::fanout_reminders(env, input),
        "plan_reminder" => remind::plan_reminder(env, input),
        "deliver_reminder" => remind::deliver_reminder(env, input),
        "deliver_events" => events::deliver_events(env, input),
        other => ops::err("invalid_request", format!("unsupported op: {other}")),
    }
}

pub fn op_summary(op: &str) -> &'static str {
    match op {
        "done_api" => "Create or read a Done request, chosen by `action`",
        "create_request" => "Ask a person to do something, tracked by Done",
        "read_request" => "Read the state of a Done request",
        "verify_token" => "Verify a signed email action link and report its fields",
        "fanout_reminders" => "Post one reminder firing per due reminder",
        "plan_reminder" => "Decide whether to send a reminder now and when the next is due",
        "deliver_reminder" => "Send a reminder to its recipient through the Core",
        "deliver_events" => "Deliver a signed Done event to every subscriber",
        _ => "Unknown operation",
    }
}

/// Nothing is asked at wizard time: credentials live on the tool's
/// `secret_requirements`, where the host resolves them once.
pub fn empty_qa_spec() -> Value {
    serde_json::json!({ "version": 1, "questions": [] })
}

pub fn i18n_keys() -> Vec<String> {
    [
        "done.schema.input.title",
        "done.schema.input.description",
        "done.schema.input.api_key.title",
        "done.schema.input.api_key.description",
        "done.schema.output.title",
        "done.schema.output.description",
        "done.schema.output.ok.title",
        "done.schema.output.ok.description",
        "done.schema.output.error.title",
        "done.schema.output.error.description",
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

#[cfg(all(target_arch = "wasm32", feature = "host"))]
struct Component;

#[cfg(all(target_arch = "wasm32", feature = "host"))]
impl node::Guest for Component {
    fn describe() -> node::ComponentDescriptor {
        node::ComponentDescriptor {
            name: COMPONENT_ID.to_string(),
            version: COMPONENT_VERSION.to_string(),
            summary: Some("Done @ Greentic: create and read requests".to_string()),
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
        let output = match decode_cbor(&envelope.payload_cbor) {
            Ok(input) => dispatch(&env::HostEnv, &op, &input),
            Err(err) => ops::err("invalid_request", format!("invalid input cbor: {err}")),
        };
        Ok(node::InvocationResult {
            ok: true,
            output_cbor: canonical_cbor_bytes(&output),
            output_metadata_cbor: None,
        })
    }
}

#[cfg(all(target_arch = "wasm32", feature = "host"))]
greentic_interfaces_guest::export_component_v060!(Component);

/// `component-v0-v6-v0` also exports `component-qa` and `component-i18n`; a
/// component implementing only `node` fails to build.
#[cfg(all(target_arch = "wasm32", feature = "host"))]
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
            // Nothing is asked, so nothing is applied: hand the config back.
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
    use crate::env::{Env, HttpReq, Response};
    use serde_json::json;

    struct NoNet;
    impl Env for NoNet {
        fn secret(&self, _: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn send(&self, _: HttpReq) -> Result<Response, String> {
            Err("no network".into())
        }
        fn now_ms(&self) -> u64 {
            0
        }
        fn random(&self) -> [u8; 10] {
            [0; 10]
        }
    }

    #[test]
    fn every_declared_op_dispatches_to_a_real_handler() {
        for op in OPS {
            let out = dispatch(&NoNet, op, &json!({}));
            assert!(
                !out["error"]["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("unsupported op"),
                "`{op}`"
            );
        }
    }

    #[test]
    fn every_declared_op_has_a_summary() {
        for op in OPS {
            assert_ne!(op_summary(op), "Unknown operation", "`{op}`");
        }
    }

    #[test]
    fn an_unknown_op_and_an_empty_payload_report_instead_of_panicking() {
        assert_eq!(dispatch(&NoNet, "nope", &json!({}))["ok"], false);
        for op in OPS {
            let out = dispatch(&NoNet, op, &json!({}));
            assert_eq!(out["ok"], false);
            assert!(out["error"]["code"].is_string());
        }
    }

    #[test]
    fn every_error_code_the_ops_can_emit_is_a_declared_one() {
        let out = dispatch(&NoNet, "create_request", &json!({}));
        assert!(ops::ERROR_CODES.contains(&out["error"]["code"].as_str().unwrap()));
    }

    #[test]
    fn action_wins_and_the_fallback_never_guesses() {
        assert_eq!(
            action_of(&json!({ "action": "read_request", "to": "x" })),
            Some("read_request")
        );
        assert_eq!(action_of(&json!({ "to": "x" })), Some("create_request"));
        assert_eq!(
            action_of(&json!({ "request": "x", "id": "i" })),
            Some("create_request")
        );
        assert_eq!(action_of(&json!({ "id": "i" })), Some("read_request"));
        assert_eq!(action_of(&json!({ "action": "bogus", "to": "x" })), None);
        assert_eq!(action_of(&json!({})), None);
    }

    #[test]
    fn done_api_routes_by_action() {
        let out = dispatch(&NoNet, "done_api", &json!({ "action": "read_request" }));
        assert_eq!(out["error"]["code"], "unauthorised"); // reached read, stopped at credentials
        let out = dispatch(&NoNet, "done_api", &json!({}));
        assert_eq!(out["error"]["code"], "invalid_request");
    }

    #[test]
    fn cbor_round_trips_through_the_canonical_encoder() {
        let value = json!({ "a": 1 });
        assert_eq!(decode_cbor(&canonical_cbor_bytes(&value)).unwrap(), value);
    }
}
