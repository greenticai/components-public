//! Group C: remediation / write handlers.
//!
//! Every handler receives `&K8sClient` (which already holds `&ClusterCreds`)
//! and parsed args. The **write gate** lives in `dispatch.rs` and is enforced
//! BEFORE any handler here is called. Handlers therefore never need to re-check
//! `allow_write`.
//!
//! # `dry_run`
//!
//! All handlers accept a `dry_run: bool` parameter. When `true` the query
//! `dryRun=All` is appended to the Kubernetes API request so the server
//! validates without committing.
//!
//! # confirm guard
//!
//! Destructive multi-step operations (`drain_node`, `delete_resource`) check
//! `args["confirm"].as_bool() != Some(true)` and return an error **before
//! issuing any HTTP call**.

use crate::host::HttpTransport;
use crate::k8s::{K8sClient, K8sError, collection_path, valid_path_segment};
use serde_json::{Value, json};

/// Return a `K8sError::Http` carrying a rejection message for an invalid path
/// segment. Used by handlers to reject LLM-supplied arguments that contain
/// characters that could re-target the Kubernetes API request.
fn invalid_segment_error(field: &str) -> K8sError {
    K8sError::Http(format!("invalid path segment: {field}"))
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Build the per-request query slice for dry-run mode.
///
/// Returns `[("dryRun", "All")]` when `dry_run` is `true`; empty otherwise.
/// The returned `Vec` must stay alive for the duration of the call.
fn dry_run_query(dry_run: bool) -> Vec<(&'static str, &'static str)> {
    if dry_run {
        vec![("dryRun", "All")]
    } else {
        vec![]
    }
}

// ── scale_workload ────────────────────────────────────────────────────────────

/// Scale a workload (Deployment, `StatefulSet`, `ReplicaSet`) to a desired
/// replica count by patching its `/scale` sub-resource with a merge patch.
///
/// # Arguments
/// - `namespace` — Kubernetes namespace
/// - `api_version` — e.g. `"apps/v1"`
/// - `kind` — lowercase plural, e.g. `"deployments"`
/// - `name` — resource name
/// - `replicas` — target replica count
/// - `dry_run` — when `true`, K8s validates but does not commit
pub fn scale_workload<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    namespace: &str,
    api_version: &str,
    kind: &str,
    name: &str,
    replicas: i64,
    dry_run: bool,
) -> Result<Value, K8sError> {
    if !valid_path_segment(namespace) {
        return Err(invalid_segment_error("namespace"));
    }
    if !valid_path_segment(name) {
        return Err(invalid_segment_error("name"));
    }
    let base = collection_path(api_version, kind, Some(namespace));
    let resource_path = format!("{base}/{name}/scale");
    let scale_body = json!({"spec": {"replicas": replicas}});
    let query = dry_run_query(dry_run);
    let query_refs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, *v)).collect();
    client.send(
        "PATCH",
        &resource_path,
        &query_refs,
        Some("application/merge-patch+json"),
        Some(serde_json::to_vec(&scale_body).map_err(|e| K8sError::Http(e.to_string()))?),
    )?;
    Ok(json!({
        "scaled": true,
        "name": name,
        "namespace": namespace,
        "replicas": replicas,
        "dry_run": dry_run,
    }))
}

// ── rollout_restart ───────────────────────────────────────────────────────────

const RESTART_NONCE_KEY: &str = "greentic.io/restart-nonce";

/// Trigger a rolling restart of a workload by reading, incrementing, and
/// writing back a monotonic nonce annotation on the pod template
/// (`greentic.io/restart-nonce`) via a strategic-merge patch. Because the value
/// always changes, Kubernetes always detects a pod-template diff and
/// progressively recreates pods — even on repeated calls.
///
/// This is a **read-modify-increment** operation, deliberately clock-free: the
/// `design-extension` WIT world imports only `logging`, `http`, and `secrets`
/// (no `wasi:clocks`), so a wall-clock timestamp is unavailable in the host.
/// A monotonic nonce is both available via the existing HTTP capability and
/// deterministically testable.
///
/// # Steps
/// 1. GET the workload manifest.
/// 2. Read the current `greentic.io/restart-nonce` annotation on the pod
///    template; parse as `u64`, defaulting to `0` when absent or unparseable.
/// 3. PATCH strategic-merge with `next = current + 1`.
///
/// # Arguments
/// - `namespace` — Kubernetes namespace
/// - `kind` — lowercase plural, e.g. `"deployments"`, `"statefulsets"`, `"daemonsets"`
/// - `name` — resource name
/// - `dry_run` — when `true`, K8s validates but does not commit (still performs the GET)
pub fn rollout_restart<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    namespace: &str,
    kind: &str,
    name: &str,
    dry_run: bool,
) -> Result<Value, K8sError> {
    if !valid_path_segment(namespace) {
        return Err(invalid_segment_error("namespace"));
    }
    if !valid_path_segment(name) {
        return Err(invalid_segment_error("name"));
    }
    let base = collection_path("apps/v1", kind, Some(namespace));
    let resource_path = format!("{base}/{name}");

    // Step 1 + 2: read the current nonce from the pod-template annotations.
    // The annotation key contains a literal `/`, so read the `annotations`
    // object and index the key directly rather than via a `json::at` path.
    let manifest = client.get(&resource_path)?;
    let current_nonce: u64 = manifest["spec"]["template"]["metadata"]["annotations"]
        [RESTART_NONCE_KEY]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let next_nonce = current_nonce + 1;

    // Step 3: patch the incremented nonce so the value ALWAYS changes.
    let restart_patch = json!({
        "spec": {
            "template": {
                "metadata": {
                    "annotations": {
                        RESTART_NONCE_KEY: next_nonce.to_string()
                    }
                }
            }
        }
    });
    let query = dry_run_query(dry_run);
    let query_refs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, *v)).collect();
    client.send(
        "PATCH",
        &resource_path,
        &query_refs,
        Some("application/strategic-merge-patch+json"),
        Some(serde_json::to_vec(&restart_patch).map_err(|e| K8sError::Http(e.to_string()))?),
    )?;
    Ok(json!({
        "restarted": true,
        "name": name,
        "namespace": namespace,
        "restart_nonce": next_nonce,
        "dry_run": dry_run,
    }))
}

// ── rollout_undo ──────────────────────────────────────────────────────────────

/// Roll back a Deployment to its previous revision by reading its `ReplicaSet`
/// history and `PATCHing` the Deployment's pod template back to the previous
/// revision's pod template.
///
/// # Implementation details and limitations
///
/// Kubernetes does not expose a first-class "rollback" API on Deployments in
/// the `apps/v1` API group (the old `extensions/v1beta1` rollback sub-resource
/// was removed). The canonical approach used by `kubectl rollout undo` is:
///
/// 1. List all `ReplicaSets` owned by the Deployment (via `ownerReferences`).
/// 2. Find the `ReplicaSet` whose
///    `metadata.annotations["deployment.kubernetes.io/revision"]` is one less
///    than the Deployment's own revision.
/// 3. PATCH the Deployment's `spec.template` with the pod template of that
///    `ReplicaSet` (a strategic-merge patch) and bump the revision annotation.
///
/// **Limitations**:
/// - The revision number is parsed as an integer; Kubernetes does not guarantee
///   monotonically increasing integers (they are advisory), so "current − 1"
///   is a best-effort heuristic.
/// - Scaled-to-zero or garbage-collected `ReplicaSets` may no longer be present,
///   in which case this function returns `K8sError::Http("no previous revision
///   found")`.
/// - `dry_run` is forwarded to the final PATCH but NOT to the two reads.
/// - This is a **best-effort** implementation; for full revision-pinning use
///   `patch_resource` with an explicit pod-template diff.
pub fn rollout_undo<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    namespace: &str,
    name: &str,
    dry_run: bool,
) -> Result<Value, K8sError> {
    if !valid_path_segment(namespace) {
        return Err(invalid_segment_error("namespace"));
    }
    if !valid_path_segment(name) {
        return Err(invalid_segment_error("name"));
    }
    // Step 1: read the Deployment to get its current revision.
    let deployment_path = format!(
        "{}/{name}",
        collection_path("apps/v1", "deployments", Some(namespace))
    );
    let deployment = client.get(&deployment_path)?;
    let current_revision: i64 =
        deployment["metadata"]["annotations"]["deployment.kubernetes.io/revision"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

    // Step 2: list ReplicaSets in the namespace and filter by ownerReference.
    let rs_list_path = collection_path("apps/v1", "replicasets", Some(namespace));
    let rs_list = client.get(&rs_list_path)?;
    let items = rs_list["items"]
        .as_array()
        .map_or(&[] as &[Value], Vec::as_slice);

    let target_revision = current_revision - 1;
    let previous_rs = items.iter().find(|rs| {
        // Must be owned by this Deployment.
        let owned = rs["metadata"]["ownerReferences"]
            .as_array()
            .is_some_and(|owners| {
                owners.iter().any(|o| {
                    o["kind"].as_str() == Some("Deployment") && o["name"].as_str() == Some(name)
                })
            });
        if !owned {
            return false;
        }
        let revision: i64 = rs["metadata"]["annotations"]["deployment.kubernetes.io/revision"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(-1);
        revision == target_revision
    });

    let previous_rs = previous_rs
        .ok_or_else(|| K8sError::Http("no previous revision found in ReplicaSet history".into()))?;

    // Step 3: PATCH the Deployment's spec.template with the previous pod template.
    let pod_template = &previous_rs["spec"]["template"];
    let rollback_patch = json!({ "spec": { "template": pod_template } });
    let query = dry_run_query(dry_run);
    let query_refs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, *v)).collect();
    client.send(
        "PATCH",
        &deployment_path,
        &query_refs,
        Some("application/strategic-merge-patch+json"),
        Some(serde_json::to_vec(&rollback_patch).map_err(|e| K8sError::Http(e.to_string()))?),
    )?;

    Ok(json!({
        "rolled_back": true,
        "name": name,
        "namespace": namespace,
        "from_revision": current_revision,
        "to_revision": target_revision,
        "dry_run": dry_run,
        "limitation": "best-effort; uses current-1 revision heuristic"
    }))
}

// ── delete_pod ────────────────────────────────────────────────────────────────

/// Delete a pod (force-terminates it; the controller will restart it if
/// managed by a `ReplicaSet`/Deployment).
///
/// # Arguments
/// - `namespace` — Pod namespace
/// - `name` — Pod name
/// - `dry_run` — when `true`, K8s validates but does not commit
pub fn delete_pod<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    namespace: &str,
    name: &str,
    dry_run: bool,
) -> Result<Value, K8sError> {
    if !valid_path_segment(namespace) {
        return Err(invalid_segment_error("namespace"));
    }
    if !valid_path_segment(name) {
        return Err(invalid_segment_error("name"));
    }
    let pod_path = format!("/api/v1/namespaces/{namespace}/pods/{name}");
    let query = dry_run_query(dry_run);
    let query_refs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, *v)).collect();
    client.send("DELETE", &pod_path, &query_refs, None, None)?;
    Ok(json!({
        "deleted": true,
        "kind": "pods",
        "name": name,
        "namespace": namespace,
        "dry_run": dry_run,
    }))
}

// ── cordon_node / uncordon_node ───────────────────────────────────────────────

/// Cordon a node by setting `spec.unschedulable = true` via a merge patch.
/// New pods will not be scheduled onto the node; existing pods are unaffected.
pub fn cordon_node<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    name: &str,
    dry_run: bool,
) -> Result<Value, K8sError> {
    patch_node_schedulable(client, name, true, dry_run)
}

/// Uncordon a node by setting `spec.unschedulable = false` via a merge patch.
pub fn uncordon_node<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    name: &str,
    dry_run: bool,
) -> Result<Value, K8sError> {
    patch_node_schedulable(client, name, false, dry_run)
}

fn patch_node_schedulable<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    name: &str,
    unschedulable: bool,
    dry_run: bool,
) -> Result<Value, K8sError> {
    if !valid_path_segment(name) {
        return Err(invalid_segment_error("name"));
    }
    let node_path = format!("/api/v1/nodes/{name}");
    let sched_patch = json!({"spec": {"unschedulable": unschedulable}});
    let query = dry_run_query(dry_run);
    let query_refs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, *v)).collect();
    client.send(
        "PATCH",
        &node_path,
        &query_refs,
        Some("application/merge-patch+json"),
        Some(serde_json::to_vec(&sched_patch).map_err(|e| K8sError::Http(e.to_string()))?),
    )?;
    Ok(json!({
        "node": name,
        "unschedulable": unschedulable,
        "dry_run": dry_run,
    }))
}

// ── drain_node ────────────────────────────────────────────────────────────────

/// Drain a node: cordon it, then POST an eviction for every non-DaemonSet
/// pod currently running on it.
///
/// **Requires `confirm: true`** — checked before any HTTP call is issued.
///
/// # Implementation details
///
/// 1. Cordon the node (`spec.unschedulable = true`).
/// 2. List all pods with `fieldSelector=spec.nodeName={node}`.
/// 3. Skip DaemonSet-owned pods (they cannot be evicted).
/// 4. POST `policy/v1` evictions for each remaining pod.
///
/// `dry_run` is forwarded to the cordon PATCH but NOT to the eviction POSTs
/// because Kubernetes does not support `dryRun` on evictions prior to
/// `policy/v1`.
pub struct PatchArgs<'a> {
    pub namespace: Option<&'a str>,
    pub api_version: &'a str,
    pub kind: &'a str,
    pub name: &'a str,
    pub patch_doc: &'a Value,
    pub patch_type: &'a str,
    pub dry_run: bool,
}

/// Apply an arbitrary patch to a Kubernetes resource.
///
/// # Patch types
/// - `"merge"` (default) → `application/merge-patch+json`
/// - `"strategic"` → `application/strategic-merge-patch+json`
/// - `"json"` → `application/json-patch+json`
pub fn patch_resource<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    args: &PatchArgs<'_>,
) -> Result<Value, K8sError> {
    if !valid_path_segment(args.name) {
        return Err(invalid_segment_error("name"));
    }
    if let Some(ns) = args.namespace
        && !valid_path_segment(ns)
    {
        return Err(invalid_segment_error("namespace"));
    }
    let content_type = match args.patch_type {
        "strategic" => "application/strategic-merge-patch+json",
        "json" => "application/json-patch+json",
        _ => "application/merge-patch+json", // default: "merge"
    };
    let base = collection_path(args.api_version, args.kind, args.namespace);
    let resource_path = format!("{base}/{}", args.name);
    let query = dry_run_query(args.dry_run);
    let query_refs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, *v)).collect();
    let response = client.send(
        "PATCH",
        &resource_path,
        &query_refs,
        Some(content_type),
        Some(serde_json::to_vec(args.patch_doc).map_err(|e| K8sError::Http(e.to_string()))?),
    )?;
    Ok(json!({
        "patched": true,
        "name": args.name,
        "patch_type": content_type,
        "dry_run": args.dry_run,
        "result": response,
    }))
}

// ── apply_manifest ────────────────────────────────────────────────────────────

/// Server-side apply a manifest. Extracts the resource name and namespace from
/// the manifest's `metadata` and issues:
///
/// ```text
/// PATCH <collection>/{name}?fieldManager=greentic-k8s&force=true
/// Content-Type: application/apply-patch+yaml
/// ```
///
/// Kubernetes accepts a JSON body with the `application/apply-patch+yaml`
/// content-type for server-side apply.
///
/// # Resource kind (plural) resolution
///
/// The collection path needs the lowercase **plural** resource name (e.g.
/// `"deployments"`, `"ingresses"`). When `kind_plural` is `Some`, it is used
/// verbatim — the caller supplies the correct plural, matching every other
/// tool in this module. When it is `None`, a best-effort fallback derives the
/// plural from the manifest's `kind` field as `<lowercase>s`. That fallback is
/// only correct for regular plurals (Deployment→deployments) and is wrong for
/// irregular ones (Ingress→ingresses, NetworkPolicy→networkpolicies,
/// Endpoints→endpoints); pass `kind_plural` explicitly for those.
pub fn apply_manifest<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    manifest: &Value,
    kind_plural: Option<&str>,
    dry_run: bool,
) -> Result<Value, K8sError> {
    let api_version = manifest["apiVersion"]
        .as_str()
        .ok_or_else(|| K8sError::Http("manifest missing apiVersion".into()))?;
    let kind_raw = manifest["kind"]
        .as_str()
        .ok_or_else(|| K8sError::Http("manifest missing kind".into()))?;
    // Prefer the caller-supplied lowercase plural (correct for irregular
    // plurals). Fall back to the naive `<kind>s` derivation, which is only
    // correct for regular plurals — see the doc comment.
    let kind_plural =
        kind_plural.map_or_else(|| format!("{}s", kind_raw.to_lowercase()), str::to_string);
    let res_name = manifest["metadata"]["name"]
        .as_str()
        .ok_or_else(|| K8sError::Http("manifest missing metadata.name".into()))?;
    let namespace = manifest["metadata"]["namespace"].as_str();

    let base = collection_path(api_version, &kind_plural, namespace);
    let resource_path = format!("{base}/{res_name}");

    let mut query: Vec<(&str, &str)> = vec![("fieldManager", "greentic-k8s"), ("force", "true")];
    if dry_run {
        query.push(("dryRun", "All"));
    }

    client.send(
        "PATCH",
        &resource_path,
        &query,
        Some("application/apply-patch+yaml"),
        Some(serde_json::to_vec(manifest).map_err(|e| K8sError::Http(e.to_string()))?),
    )?;

    Ok(json!({
        "applied": true,
        "kind": kind_raw,
        "name": res_name,
        "namespace": namespace,
        "dry_run": dry_run,
    }))
}

// `drain_node` and `delete_resource` are deliberately absent — see `tools/mod`.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clusters::ClusterCreds;
    use crate::host::{HttpResponse, HttpTransport};
    use crate::k8s::K8sClient;
    use std::sync::{Arc, Mutex};

    // ── Transport fakes ───────────────────────────────────────────────────────

    /// Full-featured fake that records every call's method, URL, `content_type`
    /// header, and `body_json`. Returns a configurable fixed response.
    struct RecordingHttp {
        calls: Arc<Mutex<Vec<RecordedCall>>>,
        response: HttpResponse,
    }

    #[derive(Debug, Clone)]
    struct RecordedCall {
        method: String,
        url: String,
        content_type: Option<String>,
        body: Option<Vec<u8>>,
    }

    impl RecordingHttp {
        fn new(status: u16, body: Vec<u8>) -> Self {
            Self {
                calls: Arc::new(Mutex::new(vec![])),
                response: HttpResponse { status, body },
            }
        }

        fn first_call(&self) -> RecordedCall {
            self.calls.lock().unwrap()[0].clone()
        }
    }

    impl HttpTransport for RecordingHttp {
        fn send(
            &self,
            method: &str,
            url: &str,
            headers: &[(String, String)],
            body: Option<Vec<u8>>,
        ) -> Result<HttpResponse, String> {
            let content_type = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| v.clone());
            self.calls.lock().unwrap().push(RecordedCall {
                method: method.to_string(),
                url: url.to_string(),
                content_type,
                body,
            });
            Ok(HttpResponse {
                status: self.response.status,
                body: self.response.body.clone(),
            })
        }
    }

    /// Sequenced HTTP fake: serves responses from a queue, repeating the last
    /// entry once the queue is exhausted.
    struct SeqHttp {
        responses: Vec<Vec<u8>>,
        idx: Arc<Mutex<usize>>,
        calls: Arc<Mutex<Vec<RecordedCall>>>,
    }

    impl SeqHttp {
        fn new(responses: Vec<Vec<u8>>) -> Self {
            Self {
                responses,
                idx: Arc::new(Mutex::new(0)),
                calls: Arc::new(Mutex::new(vec![])),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl HttpTransport for SeqHttp {
        fn send(
            &self,
            method: &str,
            url: &str,
            headers: &[(String, String)],
            body: Option<Vec<u8>>,
        ) -> Result<HttpResponse, String> {
            let content_type = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| v.clone());
            self.calls.lock().unwrap().push(RecordedCall {
                method: method.to_string(),
                url: url.to_string(),
                content_type,
                body,
            });
            let mut idx = self.idx.lock().unwrap();
            let response_body = self
                .responses
                .get(*idx)
                .unwrap_or_else(|| self.responses.last().unwrap())
                .clone();
            *idx += 1;
            Ok(HttpResponse {
                status: 200,
                body: response_body,
            })
        }
    }

    // ── Credential helpers ────────────────────────────────────────────────────

    fn write_creds() -> ClusterCreds {
        ClusterCreds {
            api_url: "https://api.example:6443".into(),
            token: "t".into(),
            allow_write: true,
        }
    }

    // ── scale_workload ────────────────────────────────────────────────────────

    /// (a) `scale_workload` issues PATCH to `/scale` with merge content-type
    /// and a body containing `{"spec":{"replicas":N}}`.
    #[test]
    fn scale_workload_patches_scale_subresource_with_merge_body() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let result = scale_workload(
            &client,
            "default",
            "apps/v1",
            "deployments",
            "web",
            3,
            false,
        )
        .unwrap();
        assert_eq!(result["scaled"], true);

        let call = http.first_call();
        assert_eq!(call.method, "PATCH");
        assert!(
            call.url.ends_with("/deployments/web/scale"),
            "unexpected url: {}",
            call.url
        );
        assert_eq!(
            call.content_type.as_deref(),
            Some("application/merge-patch+json")
        );
        let body: Value = serde_json::from_slice(&call.body.unwrap()).unwrap();
        assert_eq!(body["spec"]["replicas"], 3);
    }

    /// (b) `dry_run:true` appends `dryRun=All` to the URL.
    #[test]
    fn scale_workload_dry_run_appends_dry_run_query() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        scale_workload(&client, "default", "apps/v1", "deployments", "web", 1, true).unwrap();
        let call = http.first_call();
        assert!(
            call.url.contains("dryRun=All"),
            "expected dryRun=All in url: {}",
            call.url
        );
    }

    // ── rollout_restart (read-modify-increment nonce) ─────────────────────────

    /// A GET returning a manifest whose nonce annotation is `"5"` must produce a
    /// PATCH that sets the nonce to `"6"` (strategic-merge). This proves
    /// repeated calls always change the value.
    #[test]
    fn rollout_restart_increments_existing_nonce_5_to_6() {
        let manifest = serde_json::to_vec(&json!({
            "spec": {
                "template": {
                    "metadata": {
                        "annotations": {"greentic.io/restart-nonce": "5"}
                    }
                }
            }
        }))
        .unwrap();
        // GET returns the manifest, PATCH returns empty.
        let http = SeqHttp::new(vec![manifest, b"{}".to_vec()]);
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let result = rollout_restart(&client, "default", "deployments", "web", false).unwrap();
        assert_eq!(result["restart_nonce"], 6);

        // 2 HTTP calls: GET manifest, then PATCH.
        assert_eq!(http.call_count(), 2);
        let calls = http.calls.lock().unwrap();
        assert_eq!(calls[0].method, "GET");
        assert_eq!(calls[1].method, "PATCH");
        assert_eq!(
            calls[1].content_type.as_deref(),
            Some("application/strategic-merge-patch+json")
        );
        let patch: Value = serde_json::from_slice(calls[1].body.as_ref().unwrap()).unwrap();
        assert_eq!(
            patch["spec"]["template"]["metadata"]["annotations"]["greentic.io/restart-nonce"],
            "6"
        );
    }

    /// A GET returning a manifest with NO nonce annotation must default to `0`
    /// and PATCH the nonce to `"1"`.
    #[test]
    fn rollout_restart_absent_nonce_defaults_to_1() {
        let manifest = serde_json::to_vec(&json!({
            "spec": {"template": {"metadata": {}}}
        }))
        .unwrap();
        let http = SeqHttp::new(vec![manifest, b"{}".to_vec()]);
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let result = rollout_restart(&client, "default", "deployments", "web", false).unwrap();
        assert_eq!(result["restart_nonce"], 1);

        let calls = http.calls.lock().unwrap();
        let patch: Value = serde_json::from_slice(calls[1].body.as_ref().unwrap()).unwrap();
        assert_eq!(
            patch["spec"]["template"]["metadata"]["annotations"]["greentic.io/restart-nonce"],
            "1"
        );
    }

    /// `dry_run:true` still performs the GET, then appends `dryRun=All` to the
    /// PATCH.
    #[test]
    fn rollout_restart_dry_run_appends_query_to_patch_only() {
        let manifest = serde_json::to_vec(&json!({
            "spec": {"template": {"metadata": {"annotations": {}}}}
        }))
        .unwrap();
        let http = SeqHttp::new(vec![manifest, b"{}".to_vec()]);
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        rollout_restart(&client, "default", "deployments", "web", true).unwrap();
        let calls = http.calls.lock().unwrap();
        // GET must NOT carry dryRun; PATCH must.
        assert!(
            !calls[0].url.contains("dryRun"),
            "GET should not carry dryRun: {}",
            calls[0].url
        );
        assert!(
            calls[1].url.contains("dryRun=All"),
            "PATCH should carry dryRun=All: {}",
            calls[1].url
        );
    }

    // ── rollout_undo ──────────────────────────────────────────────────────────

    #[test]
    fn rollout_undo_patches_deployment_with_previous_pod_template() {
        // Sequence: GET deployment, GET replicasets, PATCH deployment.
        let deployment = serde_json::to_vec(&json!({
            "metadata": {
                "name": "web",
                "namespace": "default",
                "annotations": {"deployment.kubernetes.io/revision": "3"}
            },
            "spec": {}
        }))
        .unwrap();

        let rs_list = serde_json::to_vec(&json!({
            "items": [
                {
                    "metadata": {
                        "name": "web-abc",
                        "annotations": {"deployment.kubernetes.io/revision": "2"},
                        "ownerReferences": [{"kind":"Deployment","name":"web"}]
                    },
                    "spec": {"template": {"spec": {"containers": [{"image": "nginx:1.24"}]}}}
                },
                {
                    "metadata": {
                        "name": "web-def",
                        "annotations": {"deployment.kubernetes.io/revision": "3"},
                        "ownerReferences": [{"kind":"Deployment","name":"web"}]
                    },
                    "spec": {"template": {"spec": {"containers": [{"image": "nginx:1.25"}]}}}
                }
            ]
        }))
        .unwrap();

        let http = SeqHttp::new(vec![deployment, rs_list, b"{}".to_vec()]);
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let result = rollout_undo(&client, "default", "web", false).unwrap();
        assert_eq!(result["rolled_back"], true);
        assert_eq!(result["to_revision"], 2);

        // 3 HTTP calls: GET deployment, GET RS list, PATCH deployment.
        assert_eq!(http.call_count(), 3);
        let calls = http.calls.lock().unwrap();
        assert_eq!(calls[2].method, "PATCH");
        assert_eq!(
            calls[2].content_type.as_deref(),
            Some("application/strategic-merge-patch+json")
        );
        let undo_patch: Value = serde_json::from_slice(calls[2].body.as_ref().unwrap()).unwrap();
        assert_eq!(
            undo_patch["spec"]["template"]["spec"]["containers"][0]["image"],
            "nginx:1.24"
        );
    }

    #[test]
    fn rollout_undo_returns_error_when_no_previous_rs_found() {
        let deployment = serde_json::to_vec(&json!({
            "metadata": {
                "name": "web",
                "namespace": "default",
                "annotations": {"deployment.kubernetes.io/revision": "1"}
            }
        }))
        .unwrap();
        let rs_list = serde_json::to_vec(&json!({"items": []})).unwrap();

        let http = SeqHttp::new(vec![deployment, rs_list]);
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let err = rollout_undo(&client, "default", "web", false).unwrap_err();
        assert!(
            err.to_string().contains("no previous revision"),
            "unexpected error: {err}"
        );
    }

    // ── delete_pod ────────────────────────────────────────────────────────────

    #[test]
    fn delete_pod_issues_delete_to_pod_path() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        delete_pod(&client, "default", "crash-pod", false).unwrap();
        let call = http.first_call();
        assert_eq!(call.method, "DELETE");
        assert!(
            call.url
                .ends_with("/api/v1/namespaces/default/pods/crash-pod"),
            "unexpected url: {}",
            call.url
        );
    }

    // ── cordon / uncordon ─────────────────────────────────────────────────────

    #[test]
    fn cordon_node_patches_unschedulable_true() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        cordon_node(&client, "node-1", false).unwrap();
        let call = http.first_call();
        assert_eq!(call.method, "PATCH");
        assert!(
            call.url.ends_with("/api/v1/nodes/node-1"),
            "url: {}",
            call.url
        );
        assert_eq!(
            call.content_type.as_deref(),
            Some("application/merge-patch+json")
        );
        let body: Value = serde_json::from_slice(&call.body.unwrap()).unwrap();
        assert_eq!(body["spec"]["unschedulable"], true);
    }

    #[test]
    fn uncordon_node_patches_unschedulable_false() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        uncordon_node(&client, "node-1", false).unwrap();
        let body: Value = serde_json::from_slice(&http.first_call().body.unwrap()).unwrap();
        assert_eq!(body["spec"]["unschedulable"], false);
    }

    // ── drain_node ────────────────────────────────────────────────────────────

    // ── patch_resource ────────────────────────────────────────────────────────

    #[test]
    fn patch_resource_uses_correct_content_type_for_strategic() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let patch_doc = json!({"spec": {"replicas": 2}});
        patch_resource(
            &client,
            &PatchArgs {
                namespace: Some("default"),
                api_version: "apps/v1",
                kind: "deployments",
                name: "web",
                patch_doc: &patch_doc,
                patch_type: "strategic",
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(
            http.first_call().content_type.as_deref(),
            Some("application/strategic-merge-patch+json")
        );
    }

    #[test]
    fn patch_resource_defaults_to_merge_patch() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let patch_doc = json!({});
        patch_resource(
            &client,
            &PatchArgs {
                namespace: Some("default"),
                api_version: "apps/v1",
                kind: "deployments",
                name: "web",
                patch_doc: &patch_doc,
                patch_type: "merge",
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(
            http.first_call().content_type.as_deref(),
            Some("application/merge-patch+json")
        );
    }

    // ── apply_manifest ────────────────────────────────────────────────────────

    #[test]
    fn apply_manifest_uses_server_side_apply_content_type_and_query() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let manifest = json!({
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "metadata": {"name": "web", "namespace": "default"},
            "spec": {}
        });
        apply_manifest(&client, &manifest, Some("deployments"), false).unwrap();
        let call = http.first_call();
        assert_eq!(call.method, "PATCH");
        assert_eq!(
            call.content_type.as_deref(),
            Some("application/apply-patch+yaml")
        );
        assert!(
            call.url.contains("fieldManager=greentic-k8s"),
            "expected fieldManager in url: {}",
            call.url
        );
        assert!(
            call.url.contains("force=true"),
            "expected force=true in url: {}",
            call.url
        );
    }

    #[test]
    fn apply_manifest_dry_run_adds_dry_run_query() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let manifest = json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {"name": "cfg", "namespace": "default"}
        });
        apply_manifest(&client, &manifest, Some("configmaps"), true).unwrap();
        assert!(
            http.first_call().url.contains("dryRun=All"),
            "expected dryRun=All in url: {}",
            http.first_call().url
        );
    }

    /// An explicit irregular plural (`kind:"ingresses"`) must produce the
    /// `/ingresses/` path — not the naive `/ingresss/` that the fallback would
    /// derive from `Ingress`.
    #[test]
    fn apply_manifest_uses_explicit_plural_for_irregular_kinds() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let manifest = json!({
            "apiVersion": "networking.k8s.io/v1",
            "kind": "Ingress",
            "metadata": {"name": "web-ing", "namespace": "default"},
            "spec": {}
        });
        apply_manifest(&client, &manifest, Some("ingresses"), false).unwrap();
        let call = http.first_call();
        assert!(
            call.url.contains("/ingresses/web-ing"),
            "expected /ingresses/ path, got: {}",
            call.url
        );
        assert!(
            !call.url.contains("/ingresss/"),
            "must not use naive fallback plural: {}",
            call.url
        );
    }

    /// When no explicit plural is given, the best-effort `<kind>s` fallback
    /// applies (correct only for regular plurals like Deployment→deployments).
    #[test]
    fn apply_manifest_falls_back_to_naive_plural_when_kind_absent() {
        let http = RecordingHttp::new(200, b"{}".to_vec());
        let creds = write_creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let manifest = json!({
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "metadata": {"name": "web", "namespace": "default"},
            "spec": {}
        });
        apply_manifest(&client, &manifest, None, false).unwrap();
        assert!(
            http.first_call().url.contains("/deployments/web"),
            "expected fallback /deployments/ path, got: {}",
            http.first_call().url
        );
    }

    // ── delete_resource ───────────────────────────────────────────────────────

    // The extension's write-gate tests drove `dispatch::invoke`, which this
    // crate does not have — the gate here is `ops::with_write_client`, and it
    // is tested there against the store it actually consults.
}
