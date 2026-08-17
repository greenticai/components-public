#![allow(unsafe_op_in_unsafe_fn)]

//! Read-only Kubernetes component for Greentic flows.
//!
//! `greentic.k8s` ships its tools as a DESIGN extension, which makes them
//! reachable to an agentic worker and to nothing else: the flow runner has no
//! path to `greentic:extension-design/tools`. This is the other half — the same
//! observation and diagnosis operations exported as flow nodes through
//! `greentic:component/node@0.6`.
//!
//! **Only the read-only half is ported, and that is a decision, not an
//! omission.** The extension ships 26 tools, ten of which mutate a cluster
//! (`apply_manifest`, `delete_resource`, `patch_resource`, `scale_workload`,
//! `drain_node`, `rollout_undo`, `delete_pod`, `cordon_node`, `uncordon_node`,
//! `rollout_restart`) — and only two of those ten require `confirm: true`. As
//! AGENTIC-WORKER tools they sit behind a worker's guardrails and a per-cluster
//! `allow_write` secret. As FLOW STEPS they would be reachable from any flow,
//! against any endpoint the node names. That trade-off is an operator's call to
//! make explicitly, so nothing here makes it for them: the extension's
//! `remediate` module is not compiled into this crate at all, and the only
//! `K8sClient` any caller can obtain is built inside `ops::with_client`.
//!
//! `clusters`, `k8s`, `json` and both read-only tool modules are the
//! extension's own, copied verbatim — they were already WIT-free, because the
//! extension had put its host dependencies behind traits. Only `transport` (the
//! trait implementations) and `ops` are new.
//!
//! The extension's `catalog` — its `list_tools` definitions — is not ported
//! either. A component's operation list is `OPS` plus its `describe()`, and the
//! field-level schema an operator authors against is the node type's
//! `config_schema`; a second in-crate list of the same names would be a rival
//! source of truth with nothing detecting drift.
//!
//! `list_clusters` is deliberately not ported either. It enumerates the clusters
//! a WORKER was configured with, and a flow node IS its cluster, named in its
//! own config.

use serde::Serialize;
use serde_json::Value;

use greentic_types::cbor::canonical;

pub use describe::{SchemaIr, input_schema, output_schema};

mod clusters;
mod describe;
mod host;
mod json;
/// Public so the URL-path builders keep the extension's own doctests, which are
/// where the API-version/namespace path rules are actually pinned.
pub mod k8s;
mod ops;
mod tools;
mod transport;

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_ID: &str = "k8s";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every operation this component exposes. The strings are a CROSS-REPO
/// contract: each appears here, in the `describe()` op list, and as the
/// `operation` of a node type in `greentic.k8s`'s describe.json. The runner
/// dispatches on it and does not default, so a mismatch is a flow that builds
/// green and dies on its first run.
pub const OPS: &[&str] = &[
    "k8s_list_namespaces",
    "k8s_list_resources",
    "k8s_get_resource",
    "k8s_describe_resource",
    "k8s_get_pod_logs",
    "k8s_get_events",
    "k8s_top_pods",
    "k8s_top_nodes",
    "k8s_get_server_version",
    "k8s_find_unhealthy_pods",
    "k8s_triage_namespace",
    "k8s_triage_cluster",
    "k8s_analyze_crashloop",
    "k8s_get_resource_pressure",
    "k8s_check_rollout_status",
];

/// Route an operation name to its handler. Extracted from the WIT layer so it
/// is testable off-wasm; the `node::Guest` impl is a thin wrapper over it.
pub fn dispatch(op: &str, input: &Value) -> Value {
    match op {
        "k8s_list_namespaces" => ops::list_namespaces(input),
        "k8s_list_resources" => ops::list_resources(input),
        "k8s_get_resource" => ops::get_resource(input),
        "k8s_describe_resource" => ops::describe_resource(input),
        "k8s_get_pod_logs" => ops::get_pod_logs(input),
        "k8s_get_events" => ops::get_events(input),
        "k8s_top_pods" => ops::top_pods(input),
        "k8s_top_nodes" => ops::top_nodes(input),
        "k8s_get_server_version" => ops::get_server_version(input),
        "k8s_find_unhealthy_pods" => ops::find_unhealthy_pods(input),
        "k8s_triage_namespace" => ops::triage_namespace(input),
        "k8s_triage_cluster" => ops::triage_cluster(input),
        "k8s_analyze_crashloop" => ops::analyze_crashloop(input),
        "k8s_get_resource_pressure" => ops::get_resource_pressure(input),
        "k8s_check_rollout_status" => ops::check_rollout_status(input),
        other => ops::err(format!("unsupported op: {other}")),
    }
}

/// One line per operation, shown wherever the descriptor is rendered.
pub fn op_summary(op: &str) -> &'static str {
    match op {
        "k8s_list_namespaces" => "List the namespaces in a cluster",
        "k8s_list_resources" => {
            "List resources of a kind, optionally filtered by namespace or label"
        }
        "k8s_get_resource" => "Fetch one resource by kind and name",
        "k8s_describe_resource" => "Describe a resource: spec, status, and its recent events",
        "k8s_get_pod_logs" => "Read a pod's container logs",
        "k8s_get_events" => "List cluster events, optionally scoped to a namespace or object",
        "k8s_top_pods" => "Report per-pod CPU and memory usage",
        "k8s_top_nodes" => "Report per-node CPU and memory usage",
        "k8s_get_server_version" => "Read the cluster's API server version",
        "k8s_find_unhealthy_pods" => "Find pods that are not healthy, with a reason for each",
        "k8s_triage_namespace" => "Summarise a namespace's health from its pods and events",
        "k8s_triage_cluster" => "Summarise cluster health from nodes, pods, and metrics",
        "k8s_analyze_crashloop" => {
            "Explain why a pod is crash-looping, from its previous logs and events"
        }
        "k8s_get_resource_pressure" => "Report nodes under CPU or memory pressure",
        "k8s_check_rollout_status" => "Report whether a workload's rollout has completed",
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
        "k8s.schema.input.title",
        "k8s.schema.input.description",
        "k8s.schema.input.api_url.title",
        "k8s.schema.input.api_url.description",
        "k8s.schema.input.token.title",
        "k8s.schema.input.token.description",
        "k8s.schema.output.title",
        "k8s.schema.output.description",
        "k8s.schema.output.ok.title",
        "k8s.schema.output.ok.description",
        "k8s.schema.output.error.title",
        "k8s.schema.output.error.description",
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
            summary: Some("Read-only Kubernetes observation and diagnosis".to_string()),
            // Declared so the host grants them: every operation makes an HTTPS
            // call to a cluster API and reads a bearer token.
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
/// The cluster's API URL and token are declared on the node type's
/// `config_schema`, where they are authored per node. Asking for them again in
/// a setup wizard would create a second place to put the same credential, and
/// no rule for which one wins.
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
        let out = dispatch("k8s_nope", &serde_json::json!({}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("unsupported op"));
    }

    #[test]
    fn every_declared_op_has_a_summary() {
        for op in OPS {
            assert_ne!(op_summary(op), "Unknown operation", "{op}");
        }
    }

    /// The whole point of this crate is that it cannot mutate a cluster. Naming
    /// the ten refused operations here means a later "just add scale_workload"
    /// has to delete an assertion that says why it was left out, rather than
    /// simply extending a match arm.
    #[test]
    fn no_mutating_operation_is_reachable() {
        for op in [
            "k8s_apply_manifest",
            "k8s_delete_resource",
            "k8s_patch_resource",
            "k8s_scale_workload",
            "k8s_drain_node",
            "k8s_rollout_undo",
            "k8s_delete_pod",
            "k8s_cordon_node",
            "k8s_uncordon_node",
            "k8s_rollout_restart",
        ] {
            assert!(
                !OPS.contains(&op),
                "{op} must not be exposed as a flow step"
            );
            let out = dispatch(op, &serde_json::json!({}));
            assert_eq!(out["ok"], false, "{op}");
            assert!(out["error"].as_str().unwrap().contains("unsupported op"));
        }
    }
}
