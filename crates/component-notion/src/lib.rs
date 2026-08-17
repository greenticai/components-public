#![allow(unsafe_op_in_unsafe_fn)]

//! Notion API component for Greentic flows.
//!
//! `greentic.notion` ships eight tools as a DESIGN extension, which makes them
//! reachable to an agentic worker and to nothing else: the flow runner has no
//! path to `greentic:extension-design/tools`. This is the other half — the same
//! operations exported as flow nodes through `greentic:component/node@0.6`.
//!
//! The request builders and response parsers (`notion`, `notion_read`,
//! `notion_users`, `notion_write`) are the extension's modules copied verbatim.
//! They were already WIT-free and host-tested, which is what makes the copy
//! safe: a Notion call means the same thing whether a worker makes it as a tool
//! or a flow runs it as a node. Only `transport` and `ops` are new — the host
//! seam the extension gets from `extension-host/http` + `extension-host/secrets`
//! has to come from the guest crate instead.

use serde::Serialize;
use serde_json::Value;

use greentic_types::cbor::canonical;

pub use describe::{SchemaIr, input_schema, output_schema};

mod describe;
mod notion;
mod notion_read;
mod notion_users;
mod notion_write;
mod ops;
mod transport;

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_ID: &str = "notion";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every operation this component exposes. The strings are a CROSS-REPO
/// contract: each appears here, in the `describe()` op list, and as the
/// `operation` of a node type in `greentic.notion`'s describe.json. The runner
/// dispatches on it and does not default, so a mismatch is a flow that builds
/// green and dies on its first run.
pub const OPS: &[&str] = &[
    "notion_query_database",
    "notion_create_page",
    "notion_update_page",
    "notion_append_block",
    "notion_search",
    "notion_retrieve_block_children",
    "notion_list_users",
    "notion_create_comment",
];

/// Route an operation name to its handler. Extracted from the WIT layer so it
/// is testable off-wasm; the `node::Guest` impl is a thin wrapper over it.
pub fn dispatch(op: &str, input: &Value) -> Value {
    match op {
        "notion_query_database" => ops::query_database(input),
        "notion_create_page" => ops::create_page(input),
        "notion_update_page" => ops::update_page(input),
        "notion_append_block" => ops::append_block(input),
        "notion_search" => ops::search(input),
        "notion_retrieve_block_children" => ops::retrieve_block_children(input),
        "notion_list_users" => ops::list_users(input),
        "notion_create_comment" => ops::create_comment(input),
        other => ops::err(format!("unsupported op: {other}")),
    }
}

/// One line per operation, shown wherever the descriptor is rendered.
pub fn op_summary(op: &str) -> &'static str {
    match op {
        "notion_query_database" => "Query a Notion database",
        "notion_create_page" => "Create a page in a Notion database",
        "notion_update_page" => "Update a page's properties, or archive it",
        "notion_append_block" => "Append block children to a page or block",
        "notion_search" => "Search pages and databases in the workspace",
        "notion_retrieve_block_children" => "Read a page or block's children",
        "notion_list_users" => "List workspace users",
        "notion_create_comment" => "Comment on a page or reply to a discussion",
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
///
/// Notion needs exactly one input the operator must supply — the token — and
/// that is declared on the node type's `config_schema`, where it is authored
/// per node. Asking for it again in a setup wizard would create a second place
/// to put the same credential, and no rule for which one wins.
pub fn empty_qa_spec() -> serde_json::Value {
    serde_json::json!({ "version": 1, "questions": [] })
}

/// Every i18n key referenced by the schemas, in one place so a key added to a
/// schema without a translation is visible.
pub fn i18n_keys() -> Vec<String> {
    [
        "notion.schema.input.title",
        "notion.schema.input.description",
        "notion.schema.input.token.title",
        "notion.schema.input.token.description",
        "notion.schema.output.title",
        "notion.schema.output.description",
        "notion.schema.output.ok.title",
        "notion.schema.output.ok.description",
        "notion.schema.output.error.title",
        "notion.schema.output.error.description",
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
            summary: Some("Notion API access for Greentic flows".to_string()),
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
            let error = out["error"].as_str().unwrap_or("");
            assert!(
                !error.contains("unsupported op"),
                "`{op}` is declared in OPS but has no dispatch arm"
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
        let out = dispatch("notion_delete_everything", &json!({}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("unsupported op"));
    }

    /// Missing arguments are a VALUE a flow can route on, never a panic. Every
    /// op, because one handler unwrapping turns a bad payload into a dead run.
    #[test]
    fn no_op_panics_on_an_empty_payload() {
        for op in OPS {
            let out = dispatch(op, &json!({}));
            assert_eq!(out["ok"], false, "`{op}` should report, not succeed");
        }
    }

    /// The token is required BEFORE anything else is validated — an operator who
    /// forgot the credential should be told that, not given an id complaint.
    #[test]
    fn a_missing_token_is_named_first() {
        let out = dispatch("notion_query_database", &json!({"database_id": "abc"}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("token"));
    }

    /// A bad id fails in the BUILDER, before any network call — which is what
    /// makes these paths testable off-wasm at all.
    #[test]
    fn a_malformed_id_is_rejected_before_the_request_is_sent() {
        let out = dispatch(
            "notion_query_database",
            &json!({"token": "ntn_x", "database_id": "not a valid id!!"}),
        );
        assert_eq!(out["ok"], false);
        let e = out["error"].as_str().unwrap();
        assert!(
            !e.contains("http is unavailable"),
            "should fail validation, not reach transport: {e}"
        );
    }

    #[test]
    fn cbor_round_trips_through_the_canonical_encoder() {
        let value = json!({"token": "ntn_x", "database_id": "abc"});
        let bytes = canonical_cbor_bytes(&value);
        assert_eq!(decode_cbor(&bytes).unwrap(), value);
    }
}
