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
pub use select::pick_json;

mod core;
mod describe;
mod flatten;
mod ops;
mod output;
mod patch;
mod select;
mod sort;

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
/// Every operation this component exposes. The strings are a CROSS-REPO
/// contract: each one appears here, in the `describe()` op list, and as the
/// `operation` of a node type in `greentic.transform`'s describe.json. The
/// runner dispatches on it and does not default, so a mismatch is a flow that
/// builds green and dies on its first run.
pub const OPS: &[&str] = &[
    "json_validate",
    "json_merge_patch",
    "json_flatten",
    "json_unflatten",
    "json_dedup",
    "json_pick",
    "json_omit",
    "json_patch",
    "json_diff",
    "json_sort",
];

/// Route an operation name to its handler. Extracted from the WIT layer so it
/// is testable off-wasm; the `node::Guest` impl is a thin wrapper over it.
pub fn dispatch(op: &str, input: &Value) -> Value {
    match op {
        "json_validate" => ops::json_validate(input),
        "json_merge_patch" => ops::json_merge_patch(input),
        "json_flatten" => ops::json_flatten(input),
        "json_unflatten" => ops::json_unflatten(input),
        "json_dedup" => ops::json_dedup(input),
        "json_pick" => ops::json_pick(input),
        "json_omit" => ops::json_omit(input),
        "json_patch" => ops::json_patch(input),
        "json_diff" => ops::json_diff(input),
        "json_sort" => ops::json_sort(input),
        other => ops::err(format!("unsupported op: {other}")),
    }
}

/// One line per operation, shown wherever the descriptor is rendered.
///
/// Note these ops all declare the SAME open input schema. That is deliberate:
/// the authoritative, field-level schema an operator authors against is the node
/// type's `config_schema` in `greentic.transform`'s describe.json, which is the
/// TOOL's own `input_schema` copied verbatim. Writing a second, hand-built
/// SchemaIr per operation here would create a rival source of truth for the same
/// twelve argument shapes, and nothing would detect the two drifting apart.
pub fn op_summary(op: &str) -> &'static str {
    match op {
        "json_validate" => "Validate JSON against a JSON Schema",
        "json_merge_patch" => "Apply an RFC 7386 JSON Merge Patch",
        "json_flatten" => "Flatten nested JSON to delimiter-joined keys",
        "json_unflatten" => "Rebuild nested JSON from delimiter-joined keys",
        "json_dedup" => "Remove duplicate entries from an array",
        "json_pick" => "Project a JSON object down to a listed set of keys",
        "json_omit" => "Drop a listed set of keys from a JSON object",
        "json_patch" => "Apply an RFC 6902 JSON Patch",
        "json_diff" => "Produce an RFC 6902 patch between two documents",
        "json_sort" => "Sort an array, optionally by a field",
        _ => "Unknown operation",
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
            assert_ne!(
                op_summary(op),
                "Unknown operation",
                "`{op}` is declared in OPS with no summary"
            );
        }
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

    /// Missing arguments are a VALUE a flow can route on, never a panic. Run
    /// against every op, because one handler unwrapping is all it takes to turn
    /// a bad payload into a dead run.
    #[test]
    fn no_op_panics_on_an_empty_payload() {
        for op in OPS {
            let out = dispatch(op, &json!({}));
            assert_eq!(out["ok"], false, "`{op}` should report, not succeed");
        }
    }

    #[test]
    fn json_pick_projects_by_membership() {
        let out = dispatch(
            "json_pick",
            &json!({"data": {"a": 1, "b": 2}, "keys": ["b"]}),
        );
        assert_eq!(out["ok"], true);
        assert_eq!(out["result"], json!({"b": 2}));
    }

    #[test]
    fn json_omit_drops_the_listed_keys() {
        let out = dispatch(
            "json_omit",
            &json!({"data": {"a": 1, "b": 2}, "keys": ["b"]}),
        );
        assert_eq!(out["ok"], true);
        assert_eq!(out["result"], json!({"a": 1}));
    }

    #[test]
    fn json_merge_patch_applies_rfc_7386() {
        let out = dispatch(
            "json_merge_patch",
            &json!({"target": {"a": 1, "b": 2}, "patch": {"b": null, "c": 3}}),
        );
        assert_eq!(out["ok"], true);
        assert_eq!(out["result"], json!({"a": 1, "c": 3}));
    }

    #[test]
    fn json_flatten_and_unflatten_round_trip() {
        let nested = json!({"a": {"b": 1}});
        let flat = dispatch("json_flatten", &json!({"data": nested}));
        assert_eq!(flat["ok"], true);
        let back = dispatch("json_unflatten", &json!({"data": flat["result"]}));
        assert_eq!(back["ok"], true);
        assert_eq!(back["result"], json!({"a": {"b": 1}}));
    }

    #[test]
    fn json_validate_reports_a_schema_mismatch_without_failing_the_node() {
        let out = dispatch(
            "json_validate",
            &json!({"schema": {"type": "object", "required": ["a"]}, "data": {}}),
        );
        assert_eq!(
            out["ok"], true,
            "a failed VALIDATION is still a successful CALL"
        );
        assert_eq!(out["result"]["valid"], false);
    }

    #[test]
    fn cbor_round_trips_through_the_canonical_encoder() {
        let value = json!({"data": {"a": 1, "b": 2}, "keys": ["b"]});
        let bytes = canonical_cbor_bytes(&value);
        assert_eq!(decode_cbor(&bytes).unwrap(), value);
    }
}
