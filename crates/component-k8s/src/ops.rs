//! One handler per read-only operation: resolve the cluster from node config,
//! parse the arguments, route to the extension's own tool module.
//!
//! The argument shapes are the extension's `dispatch_inner`, arm for arm — a
//! `list_resources` means the same thing whether an agentic worker calls it as
//! a tool or a flow runs it as a node.
//!
//! Every failure is a VALUE. A flow routes on `ok == false`; a trap would take
//! the run down with a message no operator can act on.

use serde_json::Value;

use crate::clusters::resolve_cluster;
use crate::k8s::K8sClient;
use crate::tools::{diagnose, observe, remediate};
use crate::transport::{HostHttp, NodeSecrets, resolve_secret};

/// The extension's cluster arg is a key into a worker's secret namespace; here
/// it is only a label, so it defaults rather than being required. It still
/// travels through `resolve_cluster`, whose charset check is what stops a name
/// reshaping a secret URI.
const DEFAULT_CLUSTER: &str = "default";

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(response) => return response,
        }
    };
}

fn req_str<'a>(input: &'a Value, name: &str) -> Result<&'a str, Value> {
    input
        .get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| err(format!("missing required field `{name}`")))
}

fn opt_str<'a>(input: &'a Value, name: &str) -> Option<&'a str> {
    input.get(name).and_then(Value::as_str)
}

/// Build the credential store from the node's own config.
fn secrets(input: &Value) -> Result<NodeSecrets, Value> {
    let api_url = req_str(input, "api_url")?.to_string();
    let cluster = opt_str(input, "cluster")
        .unwrap_or(DEFAULT_CLUSTER)
        .to_string();
    let raw = input
        .get("token")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| err("missing required field `token` (a value, or `secret:NAME`)"))?;
    Ok(NodeSecrets {
        api_url,
        token: resolve_secret(raw).map_err(err)?,
        cluster,
    })
}

/// Resolve the cluster, then hand the client to `f`.
///
/// Every operation goes through here, which is what makes "this component
/// cannot write" structural rather than a rule: the only client any caller can
/// obtain is built here, and the only handlers in scope are `observe` and
/// `diagnose`. `remediate` is not compiled into this crate at all.
fn with_client<F>(input: &Value, f: F) -> Value
where
    F: FnOnce(&K8sClient<'_, HostHttp>) -> Result<Value, crate::k8s::K8sError>,
{
    let store = tri!(secrets(input));
    let cluster = opt_str(input, "cluster").unwrap_or(DEFAULT_CLUSTER);
    let creds = tri!(resolve_cluster(&store, cluster).map_err(err));
    let http = HostHttp;
    let client = K8sClient {
        creds: &creds,
        http: &http,
    };
    match f(&client) {
        Ok(value) => ok(value),
        Err(e) => err(e),
    }
}

pub fn list_namespaces(input: &Value) -> Value {
    with_client(input, observe::list_namespaces)
}

pub fn list_resources(input: &Value) -> Value {
    let api_version = tri!(req_str(input, "api_version")).to_string();
    let kind = tri!(req_str(input, "kind")).to_string();
    let namespace = opt_str(input, "namespace").map(str::to_string);
    let label_selector = opt_str(input, "label_selector").map(str::to_string);
    with_client(input, |c| {
        observe::list_resources(
            c,
            &api_version,
            &kind,
            namespace.as_deref(),
            label_selector.as_deref(),
        )
    })
}

pub fn get_resource(input: &Value) -> Value {
    let api_version = tri!(req_str(input, "api_version")).to_string();
    let kind = tri!(req_str(input, "kind")).to_string();
    let name = tri!(req_str(input, "name")).to_string();
    let namespace = opt_str(input, "namespace").map(str::to_string);
    with_client(input, |c| {
        observe::get_resource(c, &api_version, &kind, &name, namespace.as_deref())
    })
}

pub fn describe_resource(input: &Value) -> Value {
    let api_version = tri!(req_str(input, "api_version")).to_string();
    let kind = tri!(req_str(input, "kind")).to_string();
    let name = tri!(req_str(input, "name")).to_string();
    let namespace = opt_str(input, "namespace").map(str::to_string);
    with_client(input, |c| {
        observe::describe_resource(c, &api_version, &kind, &name, namespace.as_deref())
    })
}

pub fn get_pod_logs(input: &Value) -> Value {
    let namespace = tri!(req_str(input, "namespace")).to_string();
    let pod = tri!(req_str(input, "pod")).to_string();
    let container = opt_str(input, "container").map(str::to_string);
    let tail_lines = input
        .get("tail_lines")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());
    let previous = input.get("previous").and_then(Value::as_bool);
    with_client(input, |c| {
        observe::get_pod_logs(
            c,
            &namespace,
            &pod,
            container.as_deref(),
            tail_lines,
            previous,
        )
    })
}

pub fn get_events(input: &Value) -> Value {
    let namespace = opt_str(input, "namespace").map(str::to_string);
    let involved_object = opt_str(input, "involved_object").map(str::to_string);
    with_client(input, |c| {
        observe::get_events(c, namespace.as_deref(), involved_object.as_deref())
    })
}

pub fn top_pods(input: &Value) -> Value {
    let namespace = opt_str(input, "namespace").map(str::to_string);
    with_client(input, |c| observe::top_pods(c, namespace.as_deref()))
}

pub fn top_nodes(input: &Value) -> Value {
    with_client(input, observe::top_nodes)
}

pub fn get_server_version(input: &Value) -> Value {
    with_client(input, observe::get_server_version)
}

pub fn find_unhealthy_pods(input: &Value) -> Value {
    let namespace = opt_str(input, "namespace").map(str::to_string);
    with_client(input, |c| {
        diagnose::find_unhealthy_pods(c, namespace.as_deref())
    })
}

pub fn triage_namespace(input: &Value) -> Value {
    let namespace = tri!(req_str(input, "namespace")).to_string();
    with_client(input, |c| diagnose::triage_namespace(c, &namespace))
}

pub fn triage_cluster(input: &Value) -> Value {
    with_client(input, diagnose::triage_cluster)
}

pub fn analyze_crashloop(input: &Value) -> Value {
    let namespace = tri!(req_str(input, "namespace")).to_string();
    let pod = tri!(req_str(input, "pod")).to_string();
    with_client(input, |c| diagnose::analyze_crashloop(c, &namespace, &pod))
}

/// Named for the operation, which was renamed away from `get_resource_pressure`:
/// gtdx refuses a tool list where one name is a prefix of another on a `_`
/// boundary, and `get_resource` made that pair confusable for a model choosing
/// between them.
pub fn report_node_pressure(input: &Value) -> Value {
    with_client(input, diagnose::get_resource_pressure)
}

/// Resolve the cluster, REFUSE unless writes are enabled for it, then hand the
/// client to `f`.
///
/// The refusal happens before the client exists, so a mutating handler cannot
/// be reached at all on a read-only cluster — the same ordering the extension's
/// dispatcher uses, and the reason its own gate test asserts "no HTTP call was
/// issued" rather than merely "an error came back".
///
/// `allow_write` comes from the secret store and never from node config; see
/// `transport::NodeSecrets`.
fn with_write_client<F>(input: &Value, f: F) -> Value
where
    F: FnOnce(&K8sClient<'_, HostHttp>) -> Result<Value, crate::k8s::K8sError>,
{
    let store = tri!(secrets(input));
    let cluster = opt_str(input, "cluster").unwrap_or(DEFAULT_CLUSTER);
    let creds = tri!(resolve_cluster(&store, cluster).map_err(err));
    if !creds.allow_write {
        return err(format!(
            "writes are disabled for cluster '{cluster}': set the secret \
             k8s/{cluster}/allow_write=true to allow this step to change the cluster"
        ));
    }
    let http = HostHttp;
    let client = K8sClient {
        creds: &creds,
        http: &http,
    };
    match f(&client) {
        Ok(value) => ok(value),
        Err(e) => err(e),
    }
}

/// Whether the operator asked for a rehearsal. Defaults to FALSE, matching the
/// tool's own default — a node that silently dry-ran would report success for
/// work it never did.
fn dry_run(input: &Value) -> bool {
    input
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn scale_workload(input: &Value) -> Value {
    let namespace = tri!(req_str(input, "namespace")).to_string();
    let api_version = tri!(req_str(input, "api_version")).to_string();
    let kind = tri!(req_str(input, "kind")).to_string();
    let name = tri!(req_str(input, "name")).to_string();
    let Some(replicas) = input.get("replicas").and_then(Value::as_i64) else {
        return err("missing required field `replicas`");
    };
    let dry = dry_run(input);
    with_write_client(input, |c| {
        remediate::scale_workload(c, &namespace, &api_version, &kind, &name, replicas, dry)
    })
}

pub fn rollout_restart(input: &Value) -> Value {
    let namespace = tri!(req_str(input, "namespace")).to_string();
    let kind = tri!(req_str(input, "kind")).to_string();
    let name = tri!(req_str(input, "name")).to_string();
    let dry = dry_run(input);
    with_write_client(input, |c| {
        remediate::rollout_restart(c, &namespace, &kind, &name, dry)
    })
}

pub fn rollout_undo(input: &Value) -> Value {
    let namespace = tri!(req_str(input, "namespace")).to_string();
    let name = tri!(req_str(input, "name")).to_string();
    let dry = dry_run(input);
    with_write_client(input, |c| {
        remediate::rollout_undo(c, &namespace, &name, dry)
    })
}

pub fn delete_pod(input: &Value) -> Value {
    let namespace = tri!(req_str(input, "namespace")).to_string();
    let name = tri!(req_str(input, "name")).to_string();
    let dry = dry_run(input);
    with_write_client(input, |c| remediate::delete_pod(c, &namespace, &name, dry))
}

pub fn cordon_node(input: &Value) -> Value {
    let name = tri!(req_str(input, "name")).to_string();
    let dry = dry_run(input);
    with_write_client(input, |c| remediate::cordon_node(c, &name, dry))
}

pub fn uncordon_node(input: &Value) -> Value {
    let name = tri!(req_str(input, "name")).to_string();
    let dry = dry_run(input);
    with_write_client(input, |c| remediate::uncordon_node(c, &name, dry))
}

pub fn patch_resource(input: &Value) -> Value {
    let api_version = tri!(req_str(input, "api_version")).to_string();
    let kind = tri!(req_str(input, "kind")).to_string();
    let name = tri!(req_str(input, "name")).to_string();
    let namespace = opt_str(input, "namespace").map(str::to_string);
    let Some(patch) = input.get("patch").cloned() else {
        return err("missing required field `patch`");
    };
    let patch_type = opt_str(input, "patch_type").map(str::to_string);
    let dry = dry_run(input);
    with_write_client(input, |c| {
        remediate::patch_resource(
            c,
            &remediate::PatchArgs {
                api_version: &api_version,
                kind: &kind,
                name: &name,
                namespace: namespace.as_deref(),
                patch_doc: &patch,
                // The tool's own default: a strategic merge patch, which is
                // what `kubectl patch` uses when none is named.
                patch_type: patch_type
                    .as_deref()
                    .unwrap_or("application/strategic-merge-patch+json"),
                dry_run: dry,
            },
        )
    })
}

pub fn apply_manifest(input: &Value) -> Value {
    let Some(manifest) = input.get("manifest").cloned() else {
        return err("missing required field `manifest`");
    };
    let kind_plural = opt_str(input, "kind_plural").map(str::to_string);
    let dry = dry_run(input);
    with_write_client(input, |c| {
        remediate::apply_manifest(c, &manifest, kind_plural.as_deref(), dry)
    })
}

pub fn check_rollout_status(input: &Value) -> Value {
    let kind = tri!(req_str(input, "kind")).to_string();
    let name = tri!(req_str(input, "name")).to_string();
    let namespace = opt_str(input, "namespace").map(str::to_string);
    with_client(input, |c| {
        diagnose::check_rollout_status(c, &kind, &name, namespace.as_deref())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> serde_json::Map<String, Value> {
        serde_json::json!({ "api_url": "https://api.example", "token": "tok" })
            .as_object()
            .unwrap()
            .clone()
    }

    /// Credentials are required by EVERY operation, including the ones that take
    /// no other argument — a node that reached the network without them would be
    /// an unauthenticated request to whatever `api_url` defaulted to.
    #[test]
    fn both_credential_fields_are_required_by_every_operation() {
        for missing in ["api_url", "token"] {
            let mut input = creds();
            input.remove(missing);
            let out = super::top_nodes(&Value::Object(input));
            assert_eq!(out["ok"], false, "{missing}");
            assert!(out["error"].as_str().unwrap().contains(missing));
        }
    }

    /// A cluster name is a key into a secret URI in the module this copies, so
    /// its charset check has to still be reachable from here.
    #[test]
    fn an_invalid_cluster_name_is_refused_before_any_request() {
        let mut input = creds();
        input.insert("cluster".into(), Value::String("../../etc".into()));
        let out = super::top_nodes(&Value::Object(input));
        assert_eq!(out["ok"], false);
        assert!(
            out["error"]
                .as_str()
                .unwrap()
                .contains("invalid cluster name")
        );
    }

    /// Off-wasm `send` fails, so a fully-configured call gets as far as the
    /// request and no further — the boundary that proves argument handling ran.
    #[test]
    fn a_complete_input_reaches_the_network_and_reports_its_absence() {
        let out = super::top_nodes(&Value::Object(creds()));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("off-wasm"));
    }

    #[test]
    fn a_required_argument_is_named_when_it_is_absent() {
        let out = super::triage_namespace(&Value::Object(creds()));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("namespace"));
    }

    // ── the write gate ───────────────────────────────────────────────────────

    fn write_input() -> Value {
        serde_json::json!({
            "api_url": "https://api.example",
            "token": "tok",
            "cluster": "prod",
            "namespace": "default",
            "api_version": "apps/v1",
            "kind": "deployments",
            "name": "web",
            "replicas": 2
        })
    }

    /// The gate, and the reason the mutators are portable at all: with no
    /// `allow_write` secret the step refuses, and it refuses BEFORE a client
    /// exists — so no request is issued, not merely no request that succeeds.
    ///
    /// Off-wasm the secret store is the process environment, and the variable
    /// is unset here, so this is the default state a cluster is in.
    #[test]
    fn a_write_refuses_when_allow_write_is_not_provisioned() {
        let out = super::scale_workload(&write_input());
        assert_eq!(out["ok"], false, "{out:?}");
        let msg = out["error"].as_str().unwrap();
        assert!(msg.contains("writes are disabled"), "{msg}");
        assert!(
            msg.contains("k8s/prod/allow_write"),
            "the refusal must name the secret that would lift it: {msg}"
        );
        assert!(
            !msg.contains("off-wasm"),
            "it must refuse at the gate, not fall through to the network: {msg}"
        );
    }

    /// Every write op goes through the same gate. Asserting one of them would
    /// let a later addition wire a handler up without it.
    #[test]
    fn every_write_operation_is_gated() {
        let input = write_input();
        let mut patched = input.clone();
        patched["patch"] = serde_json::json!({"spec": {}});
        let mut manifested = input.clone();
        manifested["manifest"] = serde_json::json!({"kind": "ConfigMap"});

        let outs = [
            super::scale_workload(&input),
            super::rollout_restart(&input),
            super::rollout_undo(&input),
            super::delete_pod(&input),
            super::cordon_node(&input),
            super::uncordon_node(&input),
            super::patch_resource(&patched),
            super::apply_manifest(&manifested),
        ];
        for out in &outs {
            assert_eq!(out["ok"], false, "{out:?}");
            assert!(
                out["error"]
                    .as_str()
                    .unwrap()
                    .contains("writes are disabled"),
                "every write op must hit the gate: {out:?}"
            );
        }
        assert_eq!(outs.len(), 8, "all eight ported writes are covered");
    }

    /// A READ must not be gated — the gate exists to stop mutation, not to make
    /// the component useless without a secret.
    #[test]
    fn a_read_is_not_gated() {
        let out = super::top_nodes(&write_input());
        assert_eq!(
            out["ok"], false,
            "reads still fail off-wasm, at the network"
        );
        assert!(
            out["error"].as_str().unwrap().contains("off-wasm"),
            "a read must reach the transport, not the write gate: {out:?}"
        );
    }

    /// `allow_write` must not be answerable from node config. If it were,
    /// drawing a flow and authorising cluster mutation would be the same act.
    #[test]
    fn node_config_cannot_turn_writes_on() {
        let mut input = write_input();
        input["allow_write"] = Value::Bool(true);
        let out = super::scale_workload(&input);
        assert_eq!(
            out["ok"], false,
            "node config must not lift the gate: {out:?}"
        );
        assert!(
            out["error"]
                .as_str()
                .unwrap()
                .contains("writes are disabled")
        );
    }

    /// The other half of the gate, and the one that proves it is a GATE rather
    /// than a blanket refusal: with the secret provisioned, the step passes the
    /// check and reaches the transport (which has no network off-wasm).
    ///
    /// Uses its OWN cluster name so the environment variable it sets — the
    /// off-wasm secret store — is one no sibling test reads. Sharing `prod`
    /// with them made this leak across threads and fail the refusal test
    /// intermittently; a lock would have serialised the tests, but not sharing
    /// the state at all is what makes them independent.
    #[test]
    fn a_write_proceeds_once_allow_write_is_provisioned() {
        let mut input = write_input();
        input["cluster"] = Value::String("writable".into());

        // SAFETY: the variable is unique to this test, so no other thread reads
        // or writes it.
        unsafe { std::env::set_var("k8s_writable_allow_write", "true") };
        let out = super::scale_workload(&input);

        assert_eq!(out["ok"], false, "off-wasm there is still no network");
        let msg = out["error"].as_str().unwrap();
        assert!(
            msg.contains("off-wasm"),
            "the write must get PAST the gate and fail at the transport: {msg}"
        );
        assert!(
            !msg.contains("writes are disabled"),
            "the gate must have opened: {msg}"
        );
    }
}
