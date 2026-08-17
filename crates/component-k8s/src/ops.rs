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
use crate::tools::{diagnose, observe};
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
    let raw = input
        .get("token")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| err("missing required field `token` (a value, or `secret:NAME`)"))?;
    Ok(NodeSecrets {
        api_url,
        token: resolve_secret(raw).map_err(err)?,
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
}
