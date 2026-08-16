#![allow(unsafe_op_in_unsafe_fn)]

//! Pure-JSON transform component for Greentic flows.
//!
//! `greentic.transform` ships twelve JSON tools as a DESIGN extension, which
//! makes them reachable to an agentic worker and to nothing else: the flow
//! runner has no path to `greentic:extension-design/tools`, so none of them can
//! be a flow step. This crate is the other half — the same operation exported
//! as a flow node, through `greentic:component/node@0.6`.
//!
//! It carries ONE operation on purpose. The point of the first component is to
//! prove the whole path (palette -> flow builder -> pack build -> runner), and
//! a path that works for one operation works for twelve; twelve written before
//! the first one runs is twelve to redo.
//!
//! Why this integration first: `json_pick` needs no network and no secrets, so
//! nothing here depends on host capabilities. Whatever fails is the seam being
//! tested, not an HTTP or credential problem wearing its clothes.

use serde::Serialize;
use serde_json::Value;

use greentic_types::cbor::canonical;

pub use describe::{SchemaIr, input_schema, output_schema};
pub use pick::{handle_pick, pick_json};

mod describe;
mod pick;

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_ID: &str = "transform";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The operation name. It is the SAME string three places have to agree on:
/// this dispatch arm, the `describe()` op list, and the `operation` field of the
/// node type the extension's describe.json contributes. The runner dispatches on
/// it and does not default — a mismatch is a flow that builds green and dies on
/// its first run.
pub const OP_JSON_PICK: &str = "json_pick";

/// Route an operation name to its handler. Extracted from the WIT layer so it
/// is testable off-wasm; the `node::Guest` impl is a thin wrapper over it.
pub fn dispatch(op: &str, input: &Value) -> Value {
    match op {
        OP_JSON_PICK => handle_pick(input),
        other => serde_json::json!({
            "ok": false,
            "error": format!("unsupported op: {other}")
        }),
    }
}

pub fn canonical_cbor_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    canonical::to_canonical_cbor(value).unwrap_or_default()
}

pub fn decode_cbor(bytes: &[u8]) -> Result<Value, String> {
    canonical::from_cbor(bytes).map_err(|e| e.to_string())
}

/// The QA spec for a component that asks nothing. Kept as a named function so
/// the wasm export and the test below agree on what "nothing to ask" encodes to.
pub fn empty_qa_spec() -> serde_json::Value {
    serde_json::json!({ "version": 1, "questions": [] })
}

/// Every i18n key referenced by the schemas. Declared here so a key added to a
/// schema without a translation is visible in one place.
pub fn i18n_keys() -> Vec<String> {
    [
        "transform.schema.input.title",
        "transform.schema.input.description",
        "transform.schema.input.data.title",
        "transform.schema.input.data.description",
        "transform.schema.input.keys.title",
        "transform.schema.input.keys.description",
        "transform.schema.input.keys.item.title",
        "transform.schema.input.keys.item.description",
        "transform.schema.output.title",
        "transform.schema.output.description",
        "transform.schema.output.ok.title",
        "transform.schema.output.ok.description",
        "transform.schema.output.result.title",
        "transform.schema.output.result.description",
        "transform.schema.output.error.title",
        "transform.schema.output.error.description",
    ]
    .iter()
    .map(|k| (*k).to_string())
    .collect()
}

#[cfg(target_arch = "wasm32")]
struct Component;

#[cfg(target_arch = "wasm32")]
impl node::Guest for Component {
    fn describe() -> node::ComponentDescriptor {
        node::ComponentDescriptor {
            name: COMPONENT_ID.to_string(),
            version: COMPONENT_VERSION.to_string(),
            summary: Some("Pure-JSON transforms for Greentic flows".to_string()),
            // No host capabilities: every operation here is in-wasm compute.
            capabilities: Vec::new(),
            ops: vec![node::Op {
                name: OP_JSON_PICK.to_string(),
                summary: Some("Project a JSON object down to a listed set of keys".to_string()),
                input: node::IoSchema {
                    schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(&input_schema())),
                    content_type: "application/cbor".to_string(),
                    schema_version: None,
                },
                output: node::IoSchema {
                    schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(&output_schema())),
                    content_type: "application/cbor".to_string(),
                    schema_version: None,
                },
                examples: Vec::new(),
            }],
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
    fn dispatch_routes_the_declared_op() {
        let out = dispatch(OP_JSON_PICK, &json!({"data": {"a": 1}, "keys": ["a"]}));
        assert_eq!(out["ok"], true);
        assert_eq!(out["result"], json!({"a": 1}));
    }

    /// An unknown op reports a routable error rather than silently succeeding
    /// with an empty result — which is what a flow pinned to the wrong
    /// `operation` string would otherwise look like.
    #[test]
    fn an_unknown_op_is_reported_not_swallowed() {
        let out = dispatch("json_pickle", &json!({}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("unsupported op"));
    }

    /// The op name is a cross-repo contract: this component's dispatch arm, its
    /// `describe()` op list, and the `operation` on the node type the extension
    /// contributes must all be this exact string.
    #[test]
    fn the_op_name_is_the_one_the_node_type_declares() {
        assert_eq!(OP_JSON_PICK, "json_pick");
    }

    #[test]
    fn cbor_round_trips_through_the_canonical_encoder() {
        let value = json!({"data": {"a": 1, "b": 2}, "keys": ["b"]});
        let bytes = canonical_cbor_bytes(&value);
        assert_eq!(decode_cbor(&bytes).unwrap(), value);
    }
}
