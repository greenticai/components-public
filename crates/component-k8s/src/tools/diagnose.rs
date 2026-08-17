//! Group B: composite `AIOps` diagnose tools.
//!
//! Each tool is split into:
//!
//! - A **pure aggregation function** taking already-fetched `serde_json::Value`(s)
//!   and returning a `Value`. These are unit-tested on fixtures with no HTTP.
//! - A **thin handler** that fetches the raw data via `K8sClient` / observe helpers,
//!   then delegates to the pure function.
//!
//! ## Design decisions
//!
//! ### Restart-count choice (`PodIssue::restart_count`)
//!
//! We **sum** `restartCount` across all entries in `containerStatuses`. This
//! captures total pod-level restart pressure including sidecars (istio-proxy,
//! log agents). When a single crashing container is the primary concern its
//! own count dominates the sum anyway; but ignoring sidecars would miss a
//! crashing injected proxy that is the real root cause.
//!
//! ### "Stuck" rollout (`rollout_status`)
//!
//! A rollout is **complete** when `updatedReplicas`, `readyReplicas`, and
//! `availableReplicas` all equal the desired `spec.replicas` (or 1 when the
//! field is absent). A rollout is **stuck** when desired > 0 and
//! `readyReplicas < desired` AND either:
//!
//! - the `Progressing` condition is absent, or
//! - its `reason` is `"ProgressDeadlineExceeded"`, or
//! - its `status` is not `"True"`.
//!
//! Everything else is **progressing** (actively rolling out, not yet complete).
//! `StatefulSets` and `DaemonSets` use the same replica-field convention; DS uses
//! `desiredNumberScheduled` instead of `spec.replicas`.

use crate::host::HttpTransport;
use crate::json;
use crate::k8s::{K8sClient, K8sError, valid_path_segment};
use crate::tools::observe::{parse_cpu_nanocores, parse_mem_bytes};
use serde_json::{Value, json};

/// Return a `K8sError::Http` carrying a rejection message for an invalid path
/// segment. Used by handlers to reject LLM-supplied arguments that contain
/// characters that could re-target the Kubernetes API request.
fn invalid_segment_error(field: &str) -> K8sError {
    K8sError::Http(format!("invalid path segment: {field}"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared types
// ─────────────────────────────────────────────────────────────────────────────

/// A concise description of why a pod is considered unhealthy.
#[derive(Debug, PartialEq)]
pub struct PodIssue {
    /// Pod name (from `metadata.name`).
    pub name: String,
    /// Pod namespace (from `metadata.namespace`).
    pub namespace: String,
    /// Short reason token, e.g. `"CrashLoopBackOff"`, `"OOMKilled"`,
    /// `"ImagePullBackOff"`, `"Pending"`, `"NotReady"`.
    pub reason: String,
    /// Sum of `restartCount` across all `containerStatuses`.
    pub restart_count: u64,
    /// Human-readable detail, e.g. the waiting message or phase string.
    pub message: String,
}

impl PodIssue {
    /// Convert to a compact JSON object for embedding in tool output.
    fn to_value(&self) -> Value {
        json!({
            "name":          self.name,
            "namespace":     self.namespace,
            "reason":        self.reason,
            "restart_count": self.restart_count,
            "message":       self.message,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure aggregation functions (unit-testable, no HTTP)
// ─────────────────────────────────────────────────────────────────────────────

/// Inspect one container status and return `Some((reason, message))` if it is
/// in a recognised failure state, or `None` otherwise.
///
/// Checks, in order: `state.waiting.reason` for `CrashLoopBackOff`,
/// `ImagePullBackOff`, or `ErrImagePull`; then `lastState.terminated.reason`
/// for `OOMKilled`. Shared by the main-container and init-container passes.
fn detect_container_issue(cs: &Value) -> Option<(String, String)> {
    if let Some(reason) = json::at(cs, "state/waiting/reason").as_str()
        && matches!(
            reason,
            "CrashLoopBackOff" | "ImagePullBackOff" | "ErrImagePull"
        )
    {
        let message = json::at(cs, "state/waiting/message")
            .as_str()
            .unwrap_or("")
            .to_string();
        return Some((reason.to_string(), message));
    }
    if json::at(cs, "lastState/terminated/reason").as_str() == Some("OOMKilled") {
        let message = json::at(cs, "lastState/terminated/message")
            .as_str()
            .unwrap_or("")
            .to_string();
        return Some(("OOMKilled".to_string(), message));
    }
    None
}

/// Classify a single pod JSON object and return `Some(PodIssue)` if it is
/// unhealthy, or `None` if it appears healthy.
///
/// Detection order (first match wins):
/// 1. `containerStatuses[*]`: `CrashLoopBackOff`, `ImagePullBackOff` /
///    `ErrImagePull` (from `state.waiting.reason`), or `OOMKilled` (from
///    `lastState.terminated.reason`).
/// 2. `initContainerStatuses[*]` with the same failure states — checked before
///    the Pending phase so a failing init container reports its specific reason
///    rather than the generic `"Pending"`; the message is prefixed with
///    `(init)`. A failing init container keeps the pod in `phase=Pending` with
///    an empty `containerStatuses`, so without this pass the real cause is lost.
/// 3. `status.phase` == `"Pending"`.
/// 4. `status.conditions[type==Ready].status` != `"True"` (`NotReady`).
///
/// `restart_count` is the **sum** across all `containerStatuses[*].restartCount`.
#[must_use]
pub fn classify_pod(pod: &Value) -> Option<PodIssue> {
    let name = json::at(pod, "metadata/name")
        .as_str()
        .unwrap_or("")
        .to_string();
    let namespace = json::at(pod, "metadata/namespace")
        .as_str()
        .unwrap_or("")
        .to_string();

    let container_statuses = json::array(pod, "status/containerStatuses");

    // Sum restart counts across all containers.
    let restart_count: u64 = container_statuses
        .iter()
        .filter_map(|cs| cs["restartCount"].as_u64())
        .sum();

    // 1. Main containers — first failing container wins.
    for cs in container_statuses {
        if let Some((reason, message)) = detect_container_issue(cs) {
            return Some(PodIssue {
                name,
                namespace,
                reason,
                restart_count,
                message,
            });
        }
    }

    // 2. Init containers — checked before the Pending phase so the specific
    //    reason is reported. Message is annotated with "(init)".
    for cs in json::array(pod, "status/initContainerStatuses") {
        if let Some((reason, message)) = detect_container_issue(cs) {
            return Some(PodIssue {
                name,
                namespace,
                reason,
                restart_count,
                message: format!("(init) {message}"),
            });
        }
    }

    // 3. Pending phase.
    if json::at(pod, "status/phase").as_str() == Some("Pending") {
        return Some(PodIssue {
            name,
            namespace,
            reason: "Pending".into(),
            restart_count,
            message: "Pod is in Pending phase".into(),
        });
    }

    // 4. NotReady — Ready condition status != "True".
    for condition in json::array(pod, "status/conditions") {
        if json::at(condition, "type").as_str() == Some("Ready")
            && json::at(condition, "status").as_str() != Some("True")
        {
            let message = json::at(condition, "message")
                .as_str()
                .unwrap_or("")
                .to_string();
            return Some(PodIssue {
                name,
                namespace,
                reason: "NotReady".into(),
                restart_count,
                message,
            });
        }
    }

    None
}

/// Summarize a namespace by classifying all pods and collecting warning events.
///
/// # Output shape
/// ```json
/// {
///   "unhealthy_pods": [<PodIssue>, ...],
///   "warning_events": [{"reason","message","object","count","last_timestamp"}, ...],
///   "total_restarts": <u64>,
///   "unhealthy_count": <usize>
/// }
/// ```
///
/// `pods` must be a Kubernetes `PodList` (`{"items":[…]}`).
/// `events` must be a Kubernetes `EventList` (`{"items":[…]}`).
#[must_use]
pub fn summarize_namespace(pods: &Value, events: &Value) -> Value {
    let pod_items = json::array(pods, "items");

    let unhealthy: Vec<Value> = pod_items
        .iter()
        .filter_map(|pod| classify_pod(pod).map(|issue| issue.to_value()))
        .collect();

    let total_restarts: u64 = pod_items
        .iter()
        .flat_map(|pod| json::array(pod, "status/containerStatuses"))
        .filter_map(|cs| cs["restartCount"].as_u64())
        .sum();

    let warning_events: Vec<Value> = json::array(events, "items")
        .iter()
        .filter(|ev| json::at(ev, "type").as_str() == Some("Warning"))
        .map(|ev| {
            json!({
                "reason":         json::at(ev, "reason").clone(),
                "message":        json::at(ev, "message").clone(),
                "object":         json::at(ev, "involvedObject/name").clone(),
                "count":          json::at(ev, "count").clone(),
                "last_timestamp": json::at(ev, "lastTimestamp").clone(),
            })
        })
        .collect();

    let unhealthy_count = unhealthy.len();
    json!({
        "unhealthy_pods":  unhealthy,
        "warning_events":  warning_events,
        "total_restarts":  total_restarts,
        "unhealthy_count": unhealthy_count,
    })
}

/// Summarize cluster-wide health: node conditions, failed pods, top consumers.
///
/// # Output shape
/// ```json
/// {
///   "node_issues": [{"name","conditions":[{"type","status","reason"}]}, ...],
///   "unhealthy_pods": [<PodIssue>, ...],
///   "top_cpu_consumers": [{"name","namespace","cpu_nanocores"}, ...],
///   "top_mem_consumers": [{"name","namespace","mem_bytes"}, ...],
///   "node_count": <usize>,
///   "unhealthy_pod_count": <usize>
/// }
/// ```
///
/// `nodes` is a `NodeList`. `pods` is a `PodList` (cluster-scoped).
/// `node_metrics` is a `NodeMetricsList` from `metrics.k8s.io/v1beta1/nodes`.
/// Top-consumer lists are sorted descending and capped at 5 entries each.
#[must_use]
pub fn summarize_cluster(nodes: &Value, pods: &Value, node_metrics: &Value) -> Value {
    // ── node issues ──────────────────────────────────────────────────────────
    let node_issues: Vec<Value> = json::array(nodes, "items")
        .iter()
        .filter_map(|node| {
            let node_name = json::at(node, "metadata/name")
                .as_str()
                .unwrap_or("")
                .to_string();
            let problem_conditions: Vec<Value> = json::array(node, "status/conditions")
                .iter()
                .filter(|cond| {
                    // For nodes, "Ready=False/Unknown" is bad; pressure conditions True is bad.
                    let cond_type = json::at(cond, "type").as_str().unwrap_or("");
                    let cond_status = json::at(cond, "status").as_str().unwrap_or("");
                    matches!(
                        (cond_type, cond_status),
                        ("Ready", "False" | "Unknown")
                            | (
                                "MemoryPressure"
                                    | "DiskPressure"
                                    | "PIDPressure"
                                    | "NetworkUnavailable",
                                "True",
                            )
                    )
                })
                .map(|cond| {
                    json!({
                        "type":   json::at(cond, "type").clone(),
                        "status": json::at(cond, "status").clone(),
                        "reason": json::at(cond, "reason").clone(),
                    })
                })
                .collect();
            if problem_conditions.is_empty() {
                None
            } else {
                Some(json!({ "name": node_name, "conditions": problem_conditions }))
            }
        })
        .collect();

    // ── unhealthy pods ───────────────────────────────────────────────────────
    let unhealthy_pods: Vec<Value> = json::array(pods, "items")
        .iter()
        .filter_map(|pod| classify_pod(pod).map(|issue| issue.to_value()))
        .collect();

    // ── top consumers from node metrics ─────────────────────────────────────
    // Node-level metrics; we build per-node CPU/mem lists and return top-5.
    let mut node_usage: Vec<(String, u64, u64)> = json::array(node_metrics, "items")
        .iter()
        .map(|item| {
            let node_name = json::at(item, "metadata/name")
                .as_str()
                .unwrap_or("")
                .to_string();
            let cpu = parse_cpu_nanocores(json::at(item, "usage/cpu").as_str().unwrap_or("0"));
            let mem = parse_mem_bytes(json::at(item, "usage/memory").as_str().unwrap_or("0"));
            (node_name, cpu, mem)
        })
        .collect();

    // Sort by CPU descending, cap at 5.
    node_usage.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    let top_cpu_consumers: Vec<Value> = node_usage
        .iter()
        .take(5)
        .map(|(name, cpu, _)| json!({ "name": name, "cpu_nanocores": cpu }))
        .collect();

    // Sort by mem descending, cap at 5.
    node_usage.sort_by_key(|entry| std::cmp::Reverse(entry.2));
    let top_mem_consumers: Vec<Value> = node_usage
        .iter()
        .take(5)
        .map(|(name, _, mem)| json!({ "name": name, "mem_bytes": mem }))
        .collect();

    let node_count = json::array(nodes, "items").len();
    let unhealthy_pod_count = unhealthy_pods.len();

    json!({
        "node_issues":         node_issues,
        "unhealthy_pods":      unhealthy_pods,
        "top_cpu_consumers":   top_cpu_consumers,
        "top_mem_consumers":   top_mem_consumers,
        "node_count":          node_count,
        "unhealthy_pod_count": unhealthy_pod_count,
    })
}

/// Build a `CrashLoop` diagnosis report for a single pod.
///
/// # Output shape
/// ```json
/// {
///   "pod": "<name>",
///   "namespace": "<ns>",
///   "phase": "<phase>",
///   "last_termination": {"reason","exit_code","message","finished_at"},
///   "previous_logs": "<truncated log text>",
///   "warning_events": [{"reason","message","count","last_timestamp"}, ...]
/// }
/// ```
///
/// `pod` is a single Pod object. `prev_logs` is the raw previous-container log
/// text (may be empty). `events` is an `EventList` scoped to this pod.
#[must_use]
pub fn crashloop_report(pod: &Value, prev_logs: &str, events: &Value) -> Value {
    let pod_name = json::at(pod, "metadata/name")
        .as_str()
        .unwrap_or("")
        .to_string();
    let namespace = json::at(pod, "metadata/namespace")
        .as_str()
        .unwrap_or("")
        .to_string();
    let phase = json::at(pod, "status/phase")
        .as_str()
        .unwrap_or("Unknown")
        .to_string();

    // Find the last termination info from the first container that has it.
    let last_termination = json::array(pod, "status/containerStatuses")
        .iter()
        .find_map(|cs| {
            let terminated = &cs["lastState"]["terminated"];
            if terminated.is_null() {
                None
            } else {
                Some(json!({
                    "reason":      terminated["reason"].clone(),
                    "exit_code":   terminated["exitCode"].clone(),
                    "message":     terminated["message"].clone(),
                    "finished_at": terminated["finishedAt"].clone(),
                }))
            }
        })
        .unwrap_or(Value::Null);

    // Collect only Warning events scoped to this pod.
    let warning_events: Vec<Value> = json::array(events, "items")
        .iter()
        .filter(|ev| json::at(ev, "type").as_str() == Some("Warning"))
        .map(|ev| {
            json!({
                "reason":         json::at(ev, "reason").clone(),
                "message":        json::at(ev, "message").clone(),
                "count":          json::at(ev, "count").clone(),
                "last_timestamp": json::at(ev, "lastTimestamp").clone(),
            })
        })
        .collect();

    json!({
        "pod":              pod_name,
        "namespace":        namespace,
        "phase":            phase,
        "last_termination": last_termination,
        "previous_logs":    prev_logs,
        "warning_events":   warning_events,
    })
}

/// Assess resource pressure across nodes.
///
/// A node is considered "pressured" if:
/// - it has a `MemoryPressure=True` or `DiskPressure=True` condition, OR
/// - its memory usage exceeds 85 % of capacity (if capacity data is available
///   via the node object), OR
/// - its CPU usage exceeds 85 % of capacity.
///
/// # Output shape
/// ```json
/// {
///   "pressured_nodes": [
///     {
///       "name": "<node>",
///       "conditions": [{"type","status"}],
///       "cpu_nanocores": <u64>,
///       "mem_bytes": <u64>,
///       "cpu_capacity_nanocores": <u64>,   // 0 when unknown
///       "mem_capacity_bytes": <u64>        // 0 when unknown
///     }
///   ],
///   "total_nodes": <usize>
/// }
/// ```
///
/// `nodes` is a `NodeList` (includes `.status.capacity`).
/// `node_metrics` is a `NodeMetricsList` from `metrics.k8s.io/v1beta1/nodes`.
#[must_use]
pub fn pressure_report(nodes: &Value, node_metrics: &Value) -> Value {
    // Build a map of node name → usage.
    let mut usage_map: std::collections::HashMap<String, (u64, u64)> =
        std::collections::HashMap::new();
    for item in json::array(node_metrics, "items") {
        let node_name = json::at(item, "metadata/name")
            .as_str()
            .unwrap_or("")
            .to_string();
        let cpu = parse_cpu_nanocores(json::at(item, "usage/cpu").as_str().unwrap_or("0"));
        let mem = parse_mem_bytes(json::at(item, "usage/memory").as_str().unwrap_or("0"));
        usage_map.insert(node_name, (cpu, mem));
    }

    let pressured_nodes: Vec<Value> = json::array(nodes, "items")
        .iter()
        .filter_map(|node| {
            let node_name = json::at(node, "metadata/name")
                .as_str()
                .unwrap_or("")
                .to_string();

            // Check condition-based pressure.
            let pressure_conditions: Vec<Value> = json::array(node, "status/conditions")
                .iter()
                .filter(|cond| {
                    let cond_type = json::at(cond, "type").as_str().unwrap_or("");
                    let cond_status = json::at(cond, "status").as_str().unwrap_or("");
                    matches!(
                        (cond_type, cond_status),
                        ("MemoryPressure" | "DiskPressure", "True")
                    )
                })
                .map(|cond| {
                    json!({
                        "type":   json::at(cond, "type").clone(),
                        "status": json::at(cond, "status").clone(),
                    })
                })
                .collect();

            // Capacity from the node object.
            let cpu_capacity = parse_cpu_nanocores(
                json::at(node, "status/capacity/cpu")
                    .as_str()
                    .unwrap_or("0"),
            );
            let mem_capacity = parse_mem_bytes(
                json::at(node, "status/capacity/memory")
                    .as_str()
                    .unwrap_or("0"),
            );

            let (cpu_usage, mem_usage) = usage_map.get(&node_name).copied().unwrap_or((0, 0));

            // Check usage-based pressure (>85 % of capacity).
            let cpu_over_threshold = cpu_capacity > 0 && cpu_usage > (cpu_capacity * 85 / 100);
            let mem_over_threshold = mem_capacity > 0 && mem_usage > (mem_capacity * 85 / 100);

            if pressure_conditions.is_empty() && !cpu_over_threshold && !mem_over_threshold {
                return None;
            }

            Some(json!({
                "name":                   node_name,
                "conditions":             pressure_conditions,
                "cpu_nanocores":          cpu_usage,
                "mem_bytes":              mem_usage,
                "cpu_capacity_nanocores": cpu_capacity,
                "mem_capacity_bytes":     mem_capacity,
            }))
        })
        .collect();

    let total_nodes = json::array(nodes, "items").len();
    json!({
        "pressured_nodes": pressured_nodes,
        "total_nodes":     total_nodes,
    })
}

/// Assess rollout status for a Deployment, `StatefulSet`, or `DaemonSet` object.
///
/// # Output shape
/// ```json
/// {
///   "name": "<name>",
///   "kind": "<kind>",
///   "namespace": "<ns>",
///   "status": "complete" | "progressing" | "stuck",
///   "desired":   <u64>,
///   "ready":     <u64>,
///   "updated":   <u64>,
///   "available": <u64>,
///   "reason":    "<detail>"
/// }
/// ```
///
/// **Complete**: `updated == desired && ready == desired && available == desired`.
///
/// **Stuck** is **kind-aware**: only a Deployment can be reported stuck, and only
/// when its `Progressing` condition has `status != "True"` or
/// `reason == "ProgressDeadlineExceeded"`. Deployments are the only workload with
/// a progress-deadline controller that makes the `Progressing` condition
/// meaningful. `StatefulSets` and `DaemonSets` do not reliably emit a
/// `Progressing` condition during a healthy rolling update, so a missing/absent
/// condition is **never** treated as stuck for them (that would be a
/// false-positive) — they fall through to `progressing`.
///
/// **Progressing**: not complete and not Deployment-stuck (actively rolling out).
///
/// `DaemonSets` use `desiredNumberScheduled` / `numberReady` / `updatedNumberScheduled`.
#[must_use]
pub fn rollout_status(obj: &Value) -> Value {
    let res_name = json::at(obj, "metadata/name")
        .as_str()
        .unwrap_or("")
        .to_string();
    let namespace = json::at(obj, "metadata/namespace")
        .as_str()
        .unwrap_or("")
        .to_string();
    let kind = json::at(obj, "kind")
        .as_str()
        .unwrap_or("Unknown")
        .to_string();

    let (desired, ready, updated, available) = if kind == "DaemonSet" {
        let desired = obj["status"]["desiredNumberScheduled"]
            .as_u64()
            .unwrap_or(0);
        let ready = obj["status"]["numberReady"].as_u64().unwrap_or(0);
        let updated = obj["status"]["updatedNumberScheduled"]
            .as_u64()
            .unwrap_or(0);
        let available = obj["status"]["numberAvailable"].as_u64().unwrap_or(0);
        (desired, ready, updated, available)
    } else {
        // Deployment / StatefulSet
        let desired = obj["spec"]["replicas"].as_u64().unwrap_or(1);
        let ready = obj["status"]["readyReplicas"].as_u64().unwrap_or(0);
        let updated = obj["status"]["updatedReplicas"].as_u64().unwrap_or(0);
        let available = obj["status"]["availableReplicas"].as_u64().unwrap_or(0);
        (desired, ready, updated, available)
    };

    // The Progressing condition is only meaningful for Deployments (the only
    // workload with a progress-deadline controller). StatefulSets/DaemonSets
    // don't reliably emit it during a healthy roll, so we never use its absence
    // to infer "stuck" for them.
    let progressing_condition = json::array(obj, "status/conditions")
        .iter()
        .find(|cond| json::at(cond, "type").as_str() == Some("Progressing"))
        .cloned();

    let (rollout_state, reason) = if desired == 0 {
        ("complete".to_string(), "No replicas desired".to_string())
    } else if updated == desired && ready == desired && available == desired {
        (
            "complete".to_string(),
            format!("All {desired} replicas ready"),
        )
    } else {
        // Not complete. "stuck" is Deployment-only and requires an explicit
        // Progressing condition signalling failure; everything else is just a
        // roll in progress.
        let deployment_stuck = kind == "Deployment"
            && progressing_condition.as_ref().is_some_and(|cond| {
                let status = json::at(cond, "status").as_str().unwrap_or("");
                let prog_reason = json::at(cond, "reason").as_str().unwrap_or("");
                status != "True" || prog_reason == "ProgressDeadlineExceeded"
            });

        if deployment_stuck {
            let detail = progressing_condition
                .as_ref()
                .and_then(|c| json::at(c, "reason").as_str().map(str::to_string))
                .unwrap_or_else(|| "Progressing condition failing".into());
            ("stuck".to_string(), detail)
        } else {
            let detail = progressing_condition
                .as_ref()
                .and_then(|c| json::at(c, "message").as_str().map(str::to_string))
                .unwrap_or_else(|| format!("{ready}/{desired} ready, {updated} updated"));
            ("progressing".to_string(), detail)
        }
    };

    json!({
        "name":      res_name,
        "kind":      kind,
        "namespace": namespace,
        "status":    rollout_state,
        "desired":   desired,
        "ready":     ready,
        "updated":   updated,
        "available": available,
        "reason":    reason,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Thin handlers (fetch → call pure aggregator → return)
// ─────────────────────────────────────────────────────────────────────────────

/// Handler for `find_unhealthy_pods`: list all pods in the cluster or namespace,
/// classify each, and return those that are unhealthy.
pub fn find_unhealthy_pods<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    namespace: Option<&str>,
) -> Result<Value, K8sError> {
    if let Some(ns) = namespace
        && !valid_path_segment(ns)
    {
        return Err(invalid_segment_error("namespace"));
    }
    let path = match namespace {
        Some(ns) => format!("/api/v1/namespaces/{ns}/pods"),
        None => "/api/v1/pods".to_string(),
    };
    let pods_raw = client.get(&path)?;

    let unhealthy: Vec<Value> = json::array(&pods_raw, "items")
        .iter()
        .filter_map(|pod| classify_pod(pod).map(|issue| issue.to_value()))
        .collect();

    let count = unhealthy.len();
    Ok(json!({ "unhealthy_pods": unhealthy, "count": count }))
}

/// Handler for `triage_namespace`: fetch pods + events for one namespace,
/// classify pods, filter warning events, return summary.
pub fn triage_namespace<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    namespace: &str,
) -> Result<Value, K8sError> {
    if !valid_path_segment(namespace) {
        return Err(invalid_segment_error("namespace"));
    }
    let pods_raw = client.get(&format!("/api/v1/namespaces/{namespace}/pods"))?;
    let events_raw = client.get(&format!("/api/v1/namespaces/{namespace}/events"))?;
    Ok(summarize_namespace(&pods_raw, &events_raw))
}

/// Handler for `triage_cluster`: fetch nodes, all pods, node metrics, summarize.
pub fn triage_cluster<T: HttpTransport>(client: &K8sClient<'_, T>) -> Result<Value, K8sError> {
    let nodes = client.get("/api/v1/nodes")?;
    let pods = client.get("/api/v1/pods")?;
    let node_metrics = client.get("/apis/metrics.k8s.io/v1beta1/nodes")?;
    Ok(summarize_cluster(&nodes, &pods, &node_metrics))
}

/// Handler for `analyze_crashloop`: fetch pod manifest, previous logs, events,
/// return crash diagnosis report.
pub fn analyze_crashloop<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    namespace: &str,
    pod_name: &str,
) -> Result<Value, K8sError> {
    if !valid_path_segment(namespace) {
        return Err(invalid_segment_error("namespace"));
    }
    if !valid_path_segment(pod_name) {
        return Err(invalid_segment_error("pod"));
    }
    let pod = client.get(&format!("/api/v1/namespaces/{namespace}/pods/{pod_name}"))?;

    // Previous logs — best-effort; return empty string on failure.
    let prev_logs = client
        .get_text(
            &format!("/api/v1/namespaces/{namespace}/pods/{pod_name}/log"),
            &[("previous", "true"), ("tailLines", "100")],
        )
        .unwrap_or_default();

    let field_selector = format!("involvedObject.name={pod_name}");
    let events = client.send(
        "GET",
        &format!("/api/v1/namespaces/{namespace}/events"),
        &[("fieldSelector", &field_selector)],
        None,
        None,
    )?;

    Ok(crashloop_report(&pod, &prev_logs, &events))
}

/// Handler for `get_resource_pressure`: fetch nodes + node metrics, return
/// pressure summary.
pub fn get_resource_pressure<T: HttpTransport>(
    client: &K8sClient<'_, T>,
) -> Result<Value, K8sError> {
    let nodes = client.get("/api/v1/nodes")?;
    let node_metrics = client.get("/apis/metrics.k8s.io/v1beta1/nodes")?;
    Ok(pressure_report(&nodes, &node_metrics))
}

/// Handler for `check_rollout_status`: fetch a single Deployment / `StatefulSet` /
/// `DaemonSet` manifest and return its rollout assessment.
pub fn check_rollout_status<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    kind_plural: &str,
    name: &str,
    namespace: Option<&str>,
) -> Result<Value, K8sError> {
    if !valid_path_segment(kind_plural) {
        return Err(invalid_segment_error("kind"));
    }
    if !valid_path_segment(name) {
        return Err(invalid_segment_error("name"));
    }
    if let Some(ns) = namespace
        && !valid_path_segment(ns)
    {
        return Err(invalid_segment_error("namespace"));
    }
    let path = match namespace {
        Some(ns) => format!("/apis/apps/v1/namespaces/{ns}/{kind_plural}/{name}"),
        None => format!("/apis/apps/v1/{kind_plural}/{name}"),
    };
    let obj = client.get(&path)?;
    Ok(rollout_status(&obj))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clusters::ClusterCreds;
    use crate::host::{HttpResponse, HttpTransport};
    use crate::k8s::K8sClient;
    use serde_json::json;
    use std::cell::RefCell;

    // ── transport fakes ──────────────────────────────────────────────────────

    struct CapturingHttp {
        captured_urls: RefCell<Vec<String>>,
        response: Vec<u8>,
    }
    impl CapturingHttp {
        fn new(response: Vec<u8>) -> Self {
            Self {
                captured_urls: RefCell::new(vec![]),
                response,
            }
        }
    }
    impl HttpTransport for CapturingHttp {
        fn send(
            &self,
            _m: &str,
            url: &str,
            _h: &[(String, String)],
            _b: Option<Vec<u8>>,
        ) -> Result<HttpResponse, String> {
            self.captured_urls.borrow_mut().push(url.to_string());
            Ok(HttpResponse {
                status: 200,
                body: self.response.clone(),
            })
        }
    }

    fn creds() -> ClusterCreds {
        ClusterCreds {
            api_url: "https://api.example:6443".into(),
            token: "t".into(),
            allow_write: false,
        }
    }

    // ── classify_pod — brief's three verbatim tests ──────────────────────────

    #[test]
    fn detects_crashloopbackoff() {
        let pod = json!({
            "metadata": {"name":"web-1","namespace":"prod"},
            "status": {"phase":"Running","containerStatuses":[
                {"name":"web","restartCount":7,"state":{"waiting":{"reason":"CrashLoopBackOff"}}}
            ]}
        });
        let issue = classify_pod(&pod).unwrap();
        assert_eq!(issue.name, "web-1");
        assert_eq!(issue.reason, "CrashLoopBackOff");
        assert_eq!(issue.restart_count, 7);
    }

    #[test]
    fn detects_oomkilled_from_last_state() {
        let pod = json!({
            "metadata": {"name":"api-2","namespace":"prod"},
            "status": {"phase":"Running","containerStatuses":[
                {"name":"api","restartCount":3,"lastState":{"terminated":{"reason":"OOMKilled"}},"state":{"running":{}}}
            ]}
        });
        assert_eq!(classify_pod(&pod).unwrap().reason, "OOMKilled");
    }

    #[test]
    fn healthy_pod_returns_none() {
        let pod = json!({
            "metadata": {"name":"ok","namespace":"prod"},
            "status": {"phase":"Running","conditions":[{"type":"Ready","status":"True"}],
                "containerStatuses":[{"name":"c","restartCount":0,"state":{"running":{}}}]}
        });
        assert!(classify_pod(&pod).is_none());
    }

    // ── classify_pod — additional cases ──────────────────────────────────────

    #[test]
    fn detects_image_pull_backoff() {
        let pod = json!({
            "metadata": {"name":"bad-img","namespace":"default"},
            "status": {"phase":"Pending","containerStatuses":[
                {"name":"app","restartCount":0,"state":{"waiting":{"reason":"ImagePullBackOff","message":"pull failed"}}}
            ]}
        });
        let issue = classify_pod(&pod).unwrap();
        assert_eq!(issue.reason, "ImagePullBackOff");
        assert_eq!(issue.message, "pull failed");
    }

    #[test]
    fn detects_err_image_pull() {
        let pod = json!({
            "metadata": {"name":"bad-img2","namespace":"default"},
            "status": {"phase":"Pending","containerStatuses":[
                {"name":"app","restartCount":0,"state":{"waiting":{"reason":"ErrImagePull"}}}
            ]}
        });
        assert_eq!(classify_pod(&pod).unwrap().reason, "ErrImagePull");
    }

    #[test]
    fn detects_pending_phase() {
        let pod = json!({
            "metadata": {"name":"unscheduled","namespace":"staging"},
            "status": {"phase":"Pending"}
        });
        let issue = classify_pod(&pod).unwrap();
        assert_eq!(issue.reason, "Pending");
        assert_eq!(issue.name, "unscheduled");
        assert_eq!(issue.namespace, "staging");
    }

    #[test]
    fn detects_not_ready_condition() {
        let pod = json!({
            "metadata": {"name":"not-ready","namespace":"prod"},
            "status": {
                "phase":"Running",
                "conditions":[{"type":"Ready","status":"False","message":"liveness probe failed"}],
                "containerStatuses":[{"name":"app","restartCount":0,"state":{"running":{}}}]
            }
        });
        let issue = classify_pod(&pod).unwrap();
        assert_eq!(issue.reason, "NotReady");
        assert!(issue.message.contains("liveness probe failed"));
    }

    #[test]
    fn multi_container_restart_count_is_summed() {
        // Two containers — restartCounts should be summed.
        let pod = json!({
            "metadata": {"name":"multi","namespace":"prod"},
            "status": {
                "phase":"Running",
                "containerStatuses":[
                    {"name":"app","restartCount":3,"state":{"waiting":{"reason":"CrashLoopBackOff"}}},
                    {"name":"sidecar","restartCount":2,"state":{"running":{}}}
                ]
            }
        });
        let issue = classify_pod(&pod).unwrap();
        assert_eq!(issue.restart_count, 5);
        assert_eq!(issue.reason, "CrashLoopBackOff");
    }

    #[test]
    fn crashloop_takes_priority_over_oomkilled_in_last_state() {
        // One container is CrashLoopBackOff (current state) while also having
        // OOMKilled in lastState — CrashLoopBackOff should win (it is checked first).
        let pod = json!({
            "metadata": {"name":"mixed","namespace":"prod"},
            "status": {
                "phase":"Running",
                "containerStatuses":[{
                    "name":"app",
                    "restartCount":5,
                    "state":{"waiting":{"reason":"CrashLoopBackOff"}},
                    "lastState":{"terminated":{"reason":"OOMKilled"}}
                }]
            }
        });
        assert_eq!(classify_pod(&pod).unwrap().reason, "CrashLoopBackOff");
    }

    #[test]
    fn missing_container_statuses_falls_through_to_pending() {
        // No containerStatuses at all but phase is Pending.
        let pod = json!({
            "metadata": {"name":"empty","namespace":"prod"},
            "status": {"phase":"Pending"}
        });
        assert_eq!(classify_pod(&pod).unwrap().reason, "Pending");
    }

    #[test]
    fn detects_init_container_crashloop_over_pending() {
        // A failing init container keeps the pod in phase=Pending with an empty
        // containerStatuses array. The specific reason (CrashLoopBackOff) must be
        // reported, NOT the generic "Pending".
        let pod = json!({
            "metadata": {"name":"migrate","namespace":"prod"},
            "status": {
                "phase":"Pending",
                "containerStatuses": [],
                "initContainerStatuses":[
                    {"name":"db-migrate","restartCount":5,"state":{"waiting":{"reason":"CrashLoopBackOff","message":"migration failed"}}}
                ]
            }
        });
        let issue = classify_pod(&pod).unwrap();
        assert_eq!(issue.reason, "CrashLoopBackOff");
        assert!(
            issue.message.contains("(init)"),
            "expected (init) annotation, got: {}",
            issue.message
        );
    }

    #[test]
    fn detects_init_container_oomkilled() {
        let pod = json!({
            "metadata": {"name":"bootstrap","namespace":"prod"},
            "status": {
                "phase":"Pending",
                "containerStatuses": [],
                "initContainerStatuses":[
                    {"name":"init","restartCount":2,"lastState":{"terminated":{"reason":"OOMKilled"}},"state":{"waiting":{"reason":"PodInitializing"}}}
                ]
            }
        });
        assert_eq!(classify_pod(&pod).unwrap().reason, "OOMKilled");
    }

    // ── summarize_namespace ──────────────────────────────────────────────────

    #[test]
    fn summarize_namespace_counts_unhealthy_and_restarts() {
        let pods = json!({
            "items": [
                {
                    "metadata": {"name":"crash","namespace":"prod"},
                    "status": {"phase":"Running","containerStatuses":[
                        {"name":"app","restartCount":5,"state":{"waiting":{"reason":"CrashLoopBackOff"}}}
                    ]}
                },
                {
                    "metadata": {"name":"ok","namespace":"prod"},
                    "status": {"phase":"Running","conditions":[{"type":"Ready","status":"True"}],
                        "containerStatuses":[{"name":"c","restartCount":0,"state":{"running":{}}}]}
                }
            ]
        });
        let events = json!({
            "items": [
                {"type":"Warning","reason":"BackOff","message":"restarting","involvedObject":{"name":"crash"},"count":10,"lastTimestamp":"2024-01-01T00:00:00Z"},
                {"type":"Normal","reason":"Pulled","message":"image pulled","involvedObject":{"name":"ok"},"count":1,"lastTimestamp":"2024-01-01T00:00:00Z"}
            ]
        });
        let summary = summarize_namespace(&pods, &events);
        assert_eq!(summary["unhealthy_count"], 1);
        assert_eq!(summary["total_restarts"], 5);
        assert_eq!(summary["warning_events"].as_array().unwrap().len(), 1);
        assert_eq!(summary["warning_events"][0]["reason"], "BackOff");
        assert_eq!(summary["unhealthy_pods"][0]["name"], "crash");
    }

    #[test]
    fn summarize_namespace_empty_when_all_healthy() {
        let pods = json!({
            "items": [{
                "metadata": {"name":"ok","namespace":"prod"},
                "status": {"phase":"Running","conditions":[{"type":"Ready","status":"True"}],
                    "containerStatuses":[{"name":"c","restartCount":0,"state":{"running":{}}}]}
            }]
        });
        let events = json!({ "items": [] });
        let summary = summarize_namespace(&pods, &events);
        assert_eq!(summary["unhealthy_count"], 0);
        assert!(summary["unhealthy_pods"].as_array().unwrap().is_empty());
    }

    // ── summarize_cluster ────────────────────────────────────────────────────

    #[test]
    fn summarize_cluster_detects_node_pressure() {
        let nodes = json!({
            "items": [{
                "metadata": {"name":"node-1"},
                "status": {
                    "conditions": [
                        {"type":"MemoryPressure","status":"True","reason":"KubeletHasSufficientMemory"},
                        {"type":"Ready","status":"True","reason":"KubeletReady"}
                    ],
                    "capacity": {"cpu":"4","memory":"8Gi"}
                }
            }]
        });
        let pods = json!({ "items": [] });
        let metrics = json!({
            "items": [{
                "metadata": {"name":"node-1"},
                "usage": {"cpu":"500m","memory":"4Gi"}
            }]
        });
        let summary = summarize_cluster(&nodes, &pods, &metrics);
        assert_eq!(summary["node_issues"].as_array().unwrap().len(), 1);
        assert_eq!(summary["node_issues"][0]["name"], "node-1");
        assert_eq!(summary["node_count"], 1);
        assert_eq!(summary["unhealthy_pod_count"], 0);
    }

    #[test]
    fn summarize_cluster_top_consumers_sorted_desc() {
        let nodes = json!({ "items": [
            {"metadata":{"name":"node-a"},"status":{"conditions":[],"capacity":{"cpu":"8","memory":"32Gi"}}},
            {"metadata":{"name":"node-b"},"status":{"conditions":[],"capacity":{"cpu":"8","memory":"32Gi"}}}
        ] });
        let pods = json!({ "items": [] });
        let metrics = json!({
            "items": [
                {"metadata":{"name":"node-a"},"usage":{"cpu":"3000m","memory":"16Gi"}},
                {"metadata":{"name":"node-b"},"usage":{"cpu":"500m","memory":"4Gi"}}
            ]
        });
        let summary = summarize_cluster(&nodes, &pods, &metrics);
        // Top CPU consumer should be node-a (3000m > 500m).
        assert_eq!(summary["top_cpu_consumers"][0]["name"], "node-a");
        // Top memory consumer should be node-a (16Gi > 4Gi).
        assert_eq!(summary["top_mem_consumers"][0]["name"], "node-a");
    }

    // ── crashloop_report ─────────────────────────────────────────────────────

    #[test]
    fn crashloop_report_extracts_last_termination() {
        let pod = json!({
            "metadata": {"name":"crash-pod","namespace":"prod"},
            "status": {
                "phase": "Running",
                "containerStatuses": [{
                    "name": "app",
                    "restartCount": 8,
                    "lastState": {"terminated": {
                        "reason":     "OOMKilled",
                        "exitCode":   137,
                        "message":    "container killed",
                        "finishedAt": "2024-01-01T00:00:00Z"
                    }},
                    "state": {"waiting": {"reason": "CrashLoopBackOff"}}
                }]
            }
        });
        let events = json!({
            "items": [{
                "type": "Warning", "reason": "BackOff",
                "message": "back-off restarting",
                "involvedObject": {"name": "crash-pod"},
                "count": 5, "lastTimestamp": "2024-01-01T00:01:00Z"
            }]
        });
        let report = crashloop_report(&pod, "PANIC: out of memory\n", &events);
        assert_eq!(report["pod"], "crash-pod");
        assert_eq!(report["phase"], "Running");
        assert_eq!(report["last_termination"]["reason"], "OOMKilled");
        assert_eq!(report["last_termination"]["exit_code"], 137);
        assert_eq!(report["previous_logs"], "PANIC: out of memory\n");
        assert_eq!(report["warning_events"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn crashloop_report_handles_missing_last_state() {
        let pod = json!({
            "metadata": {"name":"pod","namespace":"ns"},
            "status": {"phase":"Running","containerStatuses":[
                {"name":"app","restartCount":0,"state":{"running":{}}}
            ]}
        });
        let events = json!({ "items": [] });
        let report = crashloop_report(&pod, "", &events);
        assert!(report["last_termination"].is_null());
        assert!(report["warning_events"].as_array().unwrap().is_empty());
    }

    // ── pressure_report ──────────────────────────────────────────────────────

    #[test]
    fn pressure_report_detects_disk_pressure_condition() {
        let nodes = json!({
            "items": [{
                "metadata": {"name":"disk-node"},
                "status": {
                    "conditions": [{"type":"DiskPressure","status":"True"}],
                    "capacity": {"cpu":"4","memory":"16Gi"}
                }
            }]
        });
        let metrics = json!({
            "items": [{"metadata":{"name":"disk-node"},"usage":{"cpu":"500m","memory":"4Gi"}}]
        });
        let report = pressure_report(&nodes, &metrics);
        assert_eq!(report["pressured_nodes"].as_array().unwrap().len(), 1);
        assert_eq!(report["pressured_nodes"][0]["name"], "disk-node");
        assert_eq!(
            report["pressured_nodes"][0]["conditions"][0]["type"],
            "DiskPressure"
        );
        assert_eq!(report["total_nodes"], 1);
    }

    #[test]
    fn pressure_report_detects_high_memory_usage() {
        // 90% memory usage should trigger the >85% threshold.
        // capacity = 8Gi = 8 * 1024^3 bytes; 90% usage.
        let capacity_bytes: u64 = 8 * 1024 * 1024 * 1024;
        let usage_bytes: u64 = capacity_bytes * 90 / 100;
        // Express capacity as raw bytes string and usage in Ki.
        let capacity_str = format!("{capacity_bytes}");
        let usage_ki = usage_bytes / 1024;
        let usage_str = format!("{usage_ki}Ki");

        let nodes = json!({
            "items": [{
                "metadata": {"name":"mem-node"},
                "status": {
                    "conditions": [{"type":"MemoryPressure","status":"False"}],
                    "capacity": {"cpu":"4","memory": capacity_str}
                }
            }]
        });
        let metrics = json!({
            "items": [{"metadata":{"name":"mem-node"},"usage":{"cpu":"500m","memory": usage_str}}]
        });
        let report = pressure_report(&nodes, &metrics);
        assert_eq!(
            report["pressured_nodes"].as_array().unwrap().len(),
            1,
            "expected node to be flagged for high memory usage"
        );
    }

    #[test]
    fn pressure_report_excludes_healthy_nodes() {
        let nodes = json!({
            "items": [{
                "metadata": {"name":"healthy"},
                "status": {
                    "conditions": [
                        {"type":"Ready","status":"True"},
                        {"type":"MemoryPressure","status":"False"},
                        {"type":"DiskPressure","status":"False"}
                    ],
                    "capacity": {"cpu":"4","memory":"16Gi"}
                }
            }]
        });
        let metrics = json!({
            "items": [{"metadata":{"name":"healthy"},"usage":{"cpu":"200m","memory":"1Gi"}}]
        });
        let report = pressure_report(&nodes, &metrics);
        assert!(report["pressured_nodes"].as_array().unwrap().is_empty());
    }

    // ── rollout_status ───────────────────────────────────────────────────────

    #[test]
    fn rollout_complete_when_all_replicas_ready() {
        let obj = json!({
            "kind": "Deployment",
            "metadata": {"name":"web","namespace":"prod"},
            "spec": {"replicas": 3},
            "status": {
                "readyReplicas": 3,
                "updatedReplicas": 3,
                "availableReplicas": 3,
                "conditions": [{"type":"Progressing","status":"True","reason":"NewReplicaSetAvailable","message":"done"}]
            }
        });
        let status = rollout_status(&obj);
        assert_eq!(status["status"], "complete");
        assert_eq!(status["desired"], 3);
        assert_eq!(status["ready"], 3);
    }

    #[test]
    fn rollout_progressing_when_update_in_flight() {
        let obj = json!({
            "kind": "Deployment",
            "metadata": {"name":"web","namespace":"prod"},
            "spec": {"replicas": 3},
            "status": {
                "readyReplicas": 1,
                "updatedReplicas": 2,
                "availableReplicas": 1,
                "conditions": [{
                    "type":"Progressing","status":"True",
                    "reason":"ReplicaSetUpdated",
                    "message":"rolling update 2/3"
                }]
            }
        });
        let status = rollout_status(&obj);
        assert_eq!(status["status"], "progressing");
    }

    #[test]
    fn rollout_stuck_on_progress_deadline_exceeded() {
        let obj = json!({
            "kind": "Deployment",
            "metadata": {"name":"web","namespace":"prod"},
            "spec": {"replicas": 3},
            "status": {
                "readyReplicas": 0,
                "updatedReplicas": 0,
                "availableReplicas": 0,
                "conditions": [{
                    "type":"Progressing","status":"False",
                    "reason":"ProgressDeadlineExceeded",
                    "message":"timed out"
                }]
            }
        });
        let status = rollout_status(&obj);
        assert_eq!(status["status"], "stuck");
    }

    #[test]
    fn rollout_statefulset_without_progressing_condition_is_progressing_not_stuck() {
        // StatefulSets don't reliably emit a Progressing condition during a
        // healthy rolling update, so a missing condition must NOT be reported as
        // "stuck" — that would be a false-positive. It is a roll in progress.
        let obj = json!({
            "kind": "StatefulSet",
            "metadata": {"name":"db","namespace":"prod"},
            "spec": {"replicas": 3},
            "status": {
                "readyReplicas": 0,
                "updatedReplicas": 1,
                "availableReplicas": 0,
                "conditions": []
            }
        });
        let status = rollout_status(&obj);
        assert_eq!(status["status"], "progressing");
        assert_eq!(status["kind"], "StatefulSet");
    }

    #[test]
    fn rollout_daemonset_without_progressing_condition_is_progressing_not_stuck() {
        // Same reasoning as StatefulSet: DaemonSets don't emit a meaningful
        // Progressing condition, so a partial roll is "progressing", not "stuck".
        let obj = json!({
            "kind": "DaemonSet",
            "metadata": {"name":"agent","namespace":"monitoring"},
            "status": {
                "desiredNumberScheduled": 5,
                "numberReady": 2,
                "updatedNumberScheduled": 3,
                "numberAvailable": 2,
                "conditions": []
            }
        });
        assert_eq!(rollout_status(&obj)["status"], "progressing");
    }

    #[test]
    fn rollout_deployment_stuck_when_progressing_status_false() {
        // A Deployment WITH a failing Progressing condition (status:False) is
        // genuinely stuck — Deployments are the only kind with a progress-deadline
        // controller that makes the condition meaningful.
        let obj = json!({
            "kind": "Deployment",
            "metadata": {"name":"web","namespace":"prod"},
            "spec": {"replicas": 3},
            "status": {
                "readyReplicas": 1,
                "updatedReplicas": 1,
                "availableReplicas": 1,
                "conditions": [{
                    "type":"Progressing","status":"False",
                    "reason":"ProgressDeadlineExceeded",
                    "message":"deployment exceeded its progress deadline"
                }]
            }
        });
        assert_eq!(rollout_status(&obj)["status"], "stuck");
    }

    #[test]
    fn rollout_zero_replicas_is_complete() {
        let obj = json!({
            "kind": "Deployment",
            "metadata": {"name":"paused","namespace":"prod"},
            "spec": {"replicas": 0},
            "status": {"readyReplicas": 0,"updatedReplicas": 0,"availableReplicas": 0,"conditions":[]}
        });
        assert_eq!(rollout_status(&obj)["status"], "complete");
    }

    #[test]
    fn rollout_daemonset_uses_desired_number_scheduled() {
        let obj = json!({
            "kind": "DaemonSet",
            "metadata": {"name":"logshipper","namespace":"monitoring"},
            "status": {
                "desiredNumberScheduled": 3,
                "numberReady": 3,
                "updatedNumberScheduled": 3,
                "numberAvailable": 3,
                "conditions": []
            }
        });
        assert_eq!(rollout_status(&obj)["status"], "complete");
    }

    // ── handler wiring tests ─────────────────────────────────────────────────

    #[test]
    fn find_unhealthy_pods_calls_correct_namespaced_path() {
        let body = br#"{"items":[{
            "metadata":{"name":"bad","namespace":"prod"},
            "status":{"phase":"Running","containerStatuses":[
                {"name":"app","restartCount":3,"state":{"waiting":{"reason":"CrashLoopBackOff"}}}
            ]}
        }]}"#
            .to_vec();
        let http = CapturingHttp::new(body);
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        let out = find_unhealthy_pods(&client, Some("prod")).unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["unhealthy_pods"][0]["name"], "bad");
        let urls = http.captured_urls.borrow();
        assert!(
            urls[0].contains("/namespaces/prod/pods"),
            "url was: {}",
            urls[0]
        );
    }

    #[test]
    fn triage_namespace_fetches_pods_and_events() {
        let body = br#"{"items":[]}"#.to_vec();
        let http = CapturingHttp::new(body);
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        let out = triage_namespace(&client, "staging").unwrap();
        assert_eq!(out["unhealthy_count"], 0);
        let urls = http.captured_urls.borrow();
        assert_eq!(urls.len(), 2);
        let pods_url = urls.iter().any(|u| u.contains("/namespaces/staging/pods"));
        let events_url = urls
            .iter()
            .any(|u| u.contains("/namespaces/staging/events"));
        assert!(pods_url, "expected pods URL, got: {urls:?}");
        assert!(events_url, "expected events URL, got: {urls:?}");
    }

    #[test]
    fn check_rollout_status_handler_fetches_deployment() {
        let body = serde_json::to_vec(&json!({
            "kind": "Deployment",
            "metadata": {"name":"web","namespace":"prod"},
            "spec": {"replicas": 2},
            "status": {
                "readyReplicas": 2, "updatedReplicas": 2, "availableReplicas": 2,
                "conditions": [{"type":"Progressing","status":"True","reason":"NewReplicaSetAvailable","message":"done"}]
            }
        }))
        .unwrap();
        let http = CapturingHttp::new(body);
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        let out = check_rollout_status(&client, "deployments", "web", Some("prod")).unwrap();
        assert_eq!(out["status"], "complete");
        let urls = http.captured_urls.borrow();
        assert!(urls[0].contains("/deployments/web"), "url was: {}", urls[0]);
    }

    // ── path-segment validation (FIX 2 — diagnose.rs) ────────────────────────

    /// `analyze_crashloop` with a `pod` containing `/` must be rejected BEFORE
    /// any HTTP call. Asserts call-count == 0.
    #[test]
    fn analyze_crashloop_rejects_slash_in_pod_and_issues_no_http_call() {
        let http = CapturingHttp::new(b"{}".to_vec());
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        let err = analyze_crashloop(&client, "prod", "bad/pod").unwrap_err();
        assert!(
            err.to_string().contains("invalid path segment"),
            "expected path-segment error, got: {err}"
        );
        assert_eq!(
            http.captured_urls.borrow().len(),
            0,
            "analyze_crashloop must not issue any HTTP call when pod is invalid"
        );
    }

    /// `check_rollout_status` with a `name` containing `?` must be rejected
    /// BEFORE any HTTP call. Asserts call-count == 0.
    #[test]
    fn check_rollout_status_rejects_question_mark_in_name_and_issues_no_http_call() {
        let http = CapturingHttp::new(b"{}".to_vec());
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        let err =
            check_rollout_status(&client, "deployments", "web?inject", Some("prod")).unwrap_err();
        assert!(
            err.to_string().contains("invalid path segment"),
            "expected path-segment error, got: {err}"
        );
        assert_eq!(
            http.captured_urls.borrow().len(),
            0,
            "check_rollout_status must not issue any HTTP call when name is invalid"
        );
    }

    /// `triage_namespace` with a `namespace` containing `%` must be rejected
    /// BEFORE any HTTP call. Asserts call-count == 0.
    #[test]
    fn triage_namespace_rejects_percent_in_namespace_and_issues_no_http_call() {
        let http = CapturingHttp::new(b"{}".to_vec());
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        let err = triage_namespace(&client, "ns%2e%2e").unwrap_err();
        assert!(
            err.to_string().contains("invalid path segment"),
            "expected path-segment error, got: {err}"
        );
        assert_eq!(
            http.captured_urls.borrow().len(),
            0,
            "triage_namespace must not issue any HTTP call when namespace is invalid"
        );
    }

    /// A valid dotted name like `my.workload` must NOT be over-rejected by the
    /// path-segment validator (positive test).
    #[test]
    fn check_rollout_status_accepts_dotted_name() {
        let body = serde_json::to_vec(&json!({
            "kind": "Deployment",
            "metadata": {"name":"my.workload","namespace":"prod"},
            "spec": {"replicas": 1},
            "status": {
                "readyReplicas": 1, "updatedReplicas": 1, "availableReplicas": 1,
                "conditions": [{"type":"Progressing","status":"True","reason":"NewReplicaSetAvailable","message":"done"}]
            }
        }))
        .unwrap();
        let http = CapturingHttp::new(body);
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        let out =
            check_rollout_status(&client, "deployments", "my.workload", Some("prod")).unwrap();
        assert_eq!(out["status"], "complete");
        assert_eq!(
            http.captured_urls.borrow().len(),
            1,
            "check_rollout_status should issue exactly one HTTP call for a valid dotted name"
        );
    }
}
