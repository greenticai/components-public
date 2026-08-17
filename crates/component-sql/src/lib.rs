#![allow(unsafe_op_in_unsafe_fn)]

//! Text-to-SQL component for Greentic flows.
//!
//! `greentic.sql` ships its tools as a DESIGN extension, which makes them
//! reachable to an agentic worker and to nothing else: the flow runner has no
//! path to `greentic:extension-design/tools`. This is the other half — the same
//! question-to-rows operation exported as a flow node through
//! `greentic:component/node@0.6`.
//!
//! `guard` and `protocol` are the extension's modules copied verbatim. They
//! were already WIT-free and host-tested, which is what makes the copy safe: the
//! SELECT-only guard means the same thing whether a worker calls it as a tool or
//! a flow runs it as a node. Only `transport` and `ops` are new — the host seam
//! the extension gets from `extension-host/http` + `extension-host/secrets` has
//! to come from the guest crate instead.
//!
//! The extension's second tool, `sql_list_connections`, is deliberately NOT
//! ported. It enumerates the connections a WORKER was configured with; a flow
//! node IS its connection, named in the node's own config, so there is nothing
//! for it to list.

use serde::Serialize;
use serde_json::Value;

use greentic_types::cbor::canonical;

pub use describe::{SchemaIr, input_schema, output_schema};

mod describe;
mod guard;
mod ops;
mod protocol;
mod transport;

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_ID: &str = "sql";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every operation this component exposes. The strings are a CROSS-REPO
/// contract: each appears here, in the `describe()` op list, and as the
/// `operation` of a node type in `greentic.sql`'s describe.json. The runner
/// dispatches on it and does not default, so a mismatch is a flow that builds
/// green and dies on its first run.
pub const OPS: &[&str] = &["sql_ask"];

/// Route an operation name to its handler. Extracted from the WIT layer so it
/// is testable off-wasm; the `node::Guest` impl is a thin wrapper over it.
pub fn dispatch(op: &str, input: &Value) -> Value {
    match op {
        "sql_ask" => ops::ask(input),
        other => ops::err(format!("unsupported op: {other}")),
    }
}

/// One line per operation, shown wherever the descriptor is rendered.
pub fn op_summary(op: &str) -> &'static str {
    match op {
        "sql_ask" => "Ask a database a natural-language question; returns rows plus the SQL",
        _ => "Unknown operation",
    }
}

pub fn canonical_cbor_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    canonical::to_canonical_cbor(value).unwrap_or_default()
}

pub fn decode_cbor(bytes: &[u8]) -> Result<Value, String> {
    canonical::from_cbor(bytes).map_err(|e| e.to_string())
}

/// The QA spec for a component that asks nothing at wizard time.
pub fn empty_qa_spec() -> serde_json::Value {
    serde_json::json!({ "version": 1, "questions": [] })
}

/// Every i18n key referenced by the schemas, in one place so a key added to a
/// schema without a translation is visible.
pub fn i18n_keys() -> Vec<String> {
    [
        "sql.schema.input.title",
        "sql.schema.input.description",
        "sql.schema.input.gateway_token.title",
        "sql.schema.input.gateway_token.description",
        "sql.schema.input.llm_api_key.title",
        "sql.schema.input.llm_api_key.description",
        "sql.schema.output.title",
        "sql.schema.output.description",
        "sql.schema.output.ok.title",
        "sql.schema.output.ok.description",
        "sql.schema.output.error.title",
        "sql.schema.output.error.description",
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
            summary: Some("Text-to-SQL over a Greentic SQL gateway".to_string()),
            // Declared so the host grants them: the operation makes HTTPS calls
            // to two services and reads a credential for each.
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
/// SQL's two credentials are declared on the node type's `config_schema`, where
/// they are authored per node. Asking for them again in a setup wizard would
/// create a second place to put the same secrets, and no rule for which wins.
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

    /// The op list is a cross-repo contract: `dispatch` must answer every name
    /// in `OPS`, and answer nothing outside it.
    #[test]
    fn every_declared_op_dispatches_and_an_unknown_one_is_an_error_value() {
        for op in OPS {
            let out = dispatch(op, &serde_json::json!({}));
            assert!(out.get("ok").is_some(), "{op} must return an envelope");
            assert!(
                !out["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("unsupported op"),
                "{op} is declared but not routed"
            );
        }
        let out = dispatch("sql_delete_everything", &serde_json::json!({}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("unsupported op"));
    }

    #[test]
    fn every_declared_op_has_a_summary() {
        for op in OPS {
            assert_ne!(op_summary(op), "Unknown operation", "{op}");
        }
    }

    /// The guard is the reason this operation is safe to expose as a flow step
    /// at all, so it is asserted here and not only in `guard`'s own tests.
    #[test]
    fn the_read_only_guard_travels_with_the_component() {
        assert!(guard::ensure_read_only("SELECT 1").is_ok());
        for statement in [
            "DELETE FROM users",
            "SELECT 1; DROP TABLE users",
            "INSERT INTO t VALUES (1)",
        ] {
            assert!(guard::ensure_read_only(statement).is_err(), "{statement}");
        }
    }
}
