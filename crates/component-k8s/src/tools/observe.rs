//! Group A: read/observe handlers.
use crate::host::HttpTransport;
use crate::json;
use crate::k8s::{K8sClient, K8sError, collection_path, valid_path_segment};
use serde_json::{Value, json};

/// Return a `K8sError::Http` carrying a rejection message for an invalid path
/// segment. Used by handlers to reject LLM-supplied arguments that contain
/// characters that could re-target the Kubernetes API request.
fn invalid_segment_error(field: &str) -> K8sError {
    K8sError::Http(format!("invalid path segment: {field}"))
}

/// List all namespaces in the cluster.
pub fn list_namespaces<T: HttpTransport>(client: &K8sClient<'_, T>) -> Result<Value, K8sError> {
    let raw = client.get("/api/v1/namespaces")?;
    let names: Vec<&str> = json::array(&raw, "items")
        .iter()
        .filter_map(|item| json::at(item, "metadata/name").as_str())
        .collect();
    Ok(json!({ "namespaces": names }))
}

/// List all registered cluster names from the index secret.
///
/// Reads `secret://k8s/_clusters` (comma-separated names). Returns an empty
/// list when the secret is absent — this is the only tool that does NOT
/// List resources of a given kind, optionally filtered by namespace/label.
pub fn list_resources<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    api_version: &str,
    kind_plural: &str,
    namespace: Option<&str>,
    label_selector: Option<&str>,
) -> Result<Value, K8sError> {
    if let Some(ns) = namespace
        && !valid_path_segment(ns)
    {
        return Err(invalid_segment_error("namespace"));
    }
    let path = collection_path(api_version, kind_plural, namespace);
    let query: Vec<(&str, &str)> = label_selector
        .map(|sel| vec![("labelSelector", sel)])
        .unwrap_or_default();
    let raw = client.send("GET", &path, &query, None, None)?;
    let items: Vec<Value> = json::array(&raw, "items")
        .iter()
        .map(|item| {
            let name = json::at(item, "metadata/name")
                .as_str()
                .unwrap_or("")
                .to_string();
            let ns = json::at(item, "metadata/namespace")
                .as_str()
                .unwrap_or("")
                .to_string();
            json!({ "name": name, "namespace": ns })
        })
        .collect();
    Ok(json!({ "items": items }))
}

/// Get the full manifest for a single named resource.
pub fn get_resource<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    api_version: &str,
    kind_plural: &str,
    name: &str,
    namespace: Option<&str>,
) -> Result<Value, K8sError> {
    if !valid_path_segment(name) {
        return Err(invalid_segment_error("name"));
    }
    if let Some(ns) = namespace
        && !valid_path_segment(ns)
    {
        return Err(invalid_segment_error("namespace"));
    }
    let base = collection_path(api_version, kind_plural, namespace);
    let path = format!("{base}/{name}");
    client.get(&path)
}

/// Composite: fetch the resource manifest + its events and return
/// `{status, conditions, recent_events}`.
pub fn describe_resource<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    api_version: &str,
    kind_plural: &str,
    name: &str,
    namespace: Option<&str>,
) -> Result<Value, K8sError> {
    if !valid_path_segment(name) {
        return Err(invalid_segment_error("name"));
    }
    if let Some(ns) = namespace
        && !valid_path_segment(ns)
    {
        return Err(invalid_segment_error("namespace"));
    }
    // Fetch manifest
    let base = collection_path(api_version, kind_plural, namespace);
    let resource_path = format!("{base}/{name}");
    let manifest = client.get(&resource_path)?;

    let status = json::at(&manifest, "status").clone();
    let conditions = json::array(&manifest, "status/conditions").to_vec();

    // Fetch events scoped by involvedObject.name
    let events_path = match namespace {
        Some(ns) => format!("/api/v1/namespaces/{ns}/events"),
        None => "/api/v1/events".to_string(),
    };
    let field_selector = format!("involvedObject.name={name}");
    let events_raw = client.send(
        "GET",
        &events_path,
        &[("fieldSelector", &field_selector)],
        None,
        None,
    )?;
    let recent_events: Vec<Value> = json::array(&events_raw, "items")
        .iter()
        .map(|ev| {
            json!({
                "type":           json::at(ev, "type").clone(),
                "reason":         json::at(ev, "reason").clone(),
                "message":        json::at(ev, "message").clone(),
                "object":         json::at(ev, "involvedObject/name").clone(),
                "count":          json::at(ev, "count").clone(),
                "last_timestamp": json::at(ev, "lastTimestamp").clone(),
            })
        })
        .collect();

    Ok(json!({
        "status":        status,
        "conditions":    conditions,
        "recent_events": recent_events,
    }))
}

/// Fetch raw pod log text.
pub fn get_pod_logs<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    namespace: &str,
    pod: &str,
    container: Option<&str>,
    tail_lines: Option<u32>,
    previous: Option<bool>,
) -> Result<Value, K8sError> {
    if !valid_path_segment(namespace) {
        return Err(invalid_segment_error("namespace"));
    }
    if !valid_path_segment(pod) {
        return Err(invalid_segment_error("pod"));
    }
    let path = format!("/api/v1/namespaces/{namespace}/pods/{pod}/log");

    // Build owned query pairs first to avoid lifetime issues with temporaries.
    let tail_str = tail_lines.unwrap_or(200).to_string();
    let previous_str = if previous.unwrap_or(false) {
        "true"
    } else {
        "false"
    };
    let mut query_owned: Vec<(String, String)> = vec![
        ("tailLines".into(), tail_str),
        ("previous".into(), previous_str.into()),
    ];
    if let Some(c) = container {
        query_owned.push(("container".into(), c.to_string()));
    }
    let query_refs: Vec<(&str, &str)> = query_owned
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let logs = client.get_text(&path, &query_refs)?;
    Ok(json!({ "logs": logs }))
}

/// List events, optionally scoped by namespace / involved object.
pub fn get_events<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    namespace: Option<&str>,
    involved_object: Option<&str>,
) -> Result<Value, K8sError> {
    let path = match namespace {
        Some(ns) => format!("/api/v1/namespaces/{ns}/events"),
        None => "/api/v1/events".to_string(),
    };
    let field_selector = involved_object.map(|obj| format!("involvedObject.name={obj}"));
    let query: Vec<(&str, &str)> = field_selector
        .as_deref()
        .map(|sel| vec![("fieldSelector", sel)])
        .unwrap_or_default();
    let raw = client.send("GET", &path, &query, None, None)?;
    let events: Vec<Value> = json::array(&raw, "items")
        .iter()
        .map(|ev| {
            json!({
                "type":           json::at(ev, "type").clone(),
                "reason":         json::at(ev, "reason").clone(),
                "message":        json::at(ev, "message").clone(),
                "object":         json::at(ev, "involvedObject/name").clone(),
                "count":          json::at(ev, "count").clone(),
                "last_timestamp": json::at(ev, "lastTimestamp").clone(),
            })
        })
        .collect();
    Ok(json!({ "events": events }))
}

/// Parse a Kubernetes CPU quantity string into nanocores.
///
/// metrics.k8s.io reports CPU with suffixes: `n` (nanocores), `u`
/// (microcores), `m` (millicores), or no suffix (whole cores). A bare integer
/// such as `"2"` means two full cores. Unparseable input maps to `0` so a
/// malformed entry never poisons the per-pod sum.
pub(crate) fn parse_cpu_nanocores(quantity: &str) -> u64 {
    let trimmed = quantity.trim();
    let (number, multiplier) = match trimmed.strip_suffix('n') {
        Some(rest) => (rest, 1),
        None => match trimmed.strip_suffix('u') {
            Some(rest) => (rest, 1_000),
            None => match trimmed.strip_suffix('m') {
                Some(rest) => (rest, 1_000_000),
                None => (trimmed, 1_000_000_000),
            },
        },
    };
    number.parse::<u64>().map_or(0, |value| value * multiplier)
}

/// Parse a Kubernetes memory quantity string into bytes.
///
/// metrics.k8s.io reports memory with binary suffixes (`Ki`, `Mi`, `Gi`, `Ti`)
/// or decimal suffixes (`k`, `M`, `G`), or no suffix (raw bytes). Unparseable
/// input maps to `0` so a malformed entry never poisons the per-pod sum.
pub(crate) fn parse_mem_bytes(quantity: &str) -> u64 {
    const SUFFIXES: &[(&str, u64)] = &[
        ("Ki", 1024),
        ("Mi", 1024 * 1024),
        ("Gi", 1024 * 1024 * 1024),
        ("Ti", 1024 * 1024 * 1024 * 1024),
        ("k", 1_000),
        ("M", 1_000_000),
        ("G", 1_000_000_000),
    ];
    let trimmed = quantity.trim();
    for (suffix, multiplier) in SUFFIXES {
        if let Some(rest) = trimmed.strip_suffix(suffix) {
            return rest.parse::<u64>().map_or(0, |value| value * multiplier);
        }
    }
    trimmed.parse::<u64>().unwrap_or(0)
}

/// Pod CPU/memory usage from the metrics API.
///
/// A pod's usage is the SUM across all of its containers' usage (sidecars such
/// as istio-proxy or log agents each count). CPU is reported in millicores
/// (`"<n>m"`) and memory in kibibytes (`"<n>Ki"`).
pub fn top_pods<T: HttpTransport>(
    client: &K8sClient<'_, T>,
    namespace: Option<&str>,
) -> Result<Value, K8sError> {
    let path = match namespace {
        Some(ns) => format!("/apis/metrics.k8s.io/v1beta1/namespaces/{ns}/pods"),
        None => "/apis/metrics.k8s.io/v1beta1/pods".to_string(),
    };
    let raw = client.get(&path)?;
    let pods: Vec<Value> = json::array(&raw, "items")
        .iter()
        .map(|pod| {
            let name = json::at(pod, "metadata/name")
                .as_str()
                .unwrap_or("")
                .to_string();
            // The metrics API returns `containers[*].usage.{cpu,memory}`; sum
            // across every container so multi-container pods are not undercounted.
            let containers = json::array(pod, "containers");
            let cpu_nanocores: u64 = containers
                .iter()
                .map(|c| parse_cpu_nanocores(json::at(c, "usage/cpu").as_str().unwrap_or("0")))
                .sum();
            let mem_bytes: u64 = containers
                .iter()
                .map(|c| parse_mem_bytes(json::at(c, "usage/memory").as_str().unwrap_or("0")))
                .sum();
            json!({
                "name": name,
                "cpu": format!("{}m", cpu_nanocores / 1_000_000),
                "memory": format!("{}Ki", mem_bytes / 1024),
            })
        })
        .collect();
    Ok(json!({ "pods": pods }))
}

/// Node CPU/memory usage from the metrics API.
pub fn top_nodes<T: HttpTransport>(client: &K8sClient<'_, T>) -> Result<Value, K8sError> {
    let raw = client.get("/apis/metrics.k8s.io/v1beta1/nodes")?;
    let nodes: Vec<Value> = json::array(&raw, "items")
        .iter()
        .map(|node| {
            let name = json::at(node, "metadata/name")
                .as_str()
                .unwrap_or("")
                .to_string();
            let cpu = json::at(node, "usage/cpu")
                .as_str()
                .unwrap_or("")
                .to_string();
            let memory = json::at(node, "usage/memory")
                .as_str()
                .unwrap_or("")
                .to_string();
            json!({ "name": name, "cpu": cpu, "memory": memory })
        })
        .collect();
    Ok(json!({ "nodes": nodes }))
}

/// GET `{api_url}/version` — the Kubernetes API server version.
///
/// Used as the extension's connection probe: a success means the configured
/// `api_url` + `token` can reach and authenticate to the cluster. This is the
/// one call in this module that targets the cluster ROOT rather than
/// `/api/v1` or `/apis/...` — `K8sClient::get` appends the given path
/// directly onto `api_url` with no group/version prefix, so the absolute
/// path `/version` reaches the correct endpoint unchanged.
pub fn get_server_version<T: HttpTransport>(client: &K8sClient<'_, T>) -> Result<Value, K8sError> {
    client.get("/version")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clusters::ClusterCreds;
    use crate::host::{HttpResponse, HttpTransport};
    use crate::k8s::K8sClient;
    use std::cell::RefCell;

    // ---------- transport fakes ----------

    /// Simple one-shot HTTP fake that always returns the same body.
    struct OkHttp(Vec<u8>);
    impl HttpTransport for OkHttp {
        fn send(
            &self,
            _m: &str,
            _u: &str,
            _h: &[(String, String)],
            _b: Option<Vec<u8>>,
        ) -> Result<HttpResponse, String> {
            Ok(HttpResponse {
                status: 200,
                body: self.0.clone(),
            })
        }
    }

    /// HTTP fake that captures the (method, url) of each call and serves a
    /// fixed response for every request.
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

    // ---------- helpers ----------

    fn creds() -> ClusterCreds {
        ClusterCreds {
            api_url: "https://api.example:6443".into(),
            token: "t".into(),
            allow_write: false,
        }
    }

    // ---------- list_namespaces (existing, kept for reference) ----------

    #[test]
    fn list_namespaces_returns_names() {
        let body =
            br#"{"items":[{"metadata":{"name":"default"}},{"metadata":{"name":"kube-system"}}]}"#
                .to_vec();
        let http = OkHttp(body);
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = list_namespaces(&client).unwrap();
        let names: Vec<&str> = out["namespaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["default", "kube-system"]);
    }

    // ---------- list_resources ----------

    #[test]
    fn list_resources_builds_namespaced_path_and_returns_items() {
        let body = br#"{
            "items": [
                {"metadata": {"name": "nginx", "namespace": "default"}},
                {"metadata": {"name": "redis", "namespace": "default"}}
            ]
        }"#
        .to_vec();
        let http = CapturingHttp::new(body);
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = list_resources(&client, "v1", "pods", Some("default"), None).unwrap();
        let items = out["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["name"], "nginx");
        let urls = http.captured_urls.borrow();
        assert!(
            urls[0].contains("/api/v1/namespaces/default/pods"),
            "unexpected url: {}",
            urls[0]
        );
    }

    #[test]
    fn list_resources_includes_label_selector_in_query() {
        let body = br#"{"items":[]}"#.to_vec();
        let http = CapturingHttp::new(body);
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        list_resources(&client, "apps/v1", "deployments", None, Some("app=web")).unwrap();
        let urls = http.captured_urls.borrow();
        assert!(
            urls[0].contains("labelSelector"),
            "expected labelSelector in url: {}",
            urls[0]
        );
    }

    // ---------- get_resource ----------

    #[test]
    fn get_resource_returns_full_manifest() {
        let body = br#"{"kind":"Pod","metadata":{"name":"nginx"}}"#.to_vec();
        let http = CapturingHttp::new(body);
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = get_resource(&client, "v1", "pods", "nginx", Some("default")).unwrap();
        assert_eq!(out["kind"], "Pod");
        assert_eq!(out["metadata"]["name"], "nginx");
        let urls = http.captured_urls.borrow();
        assert!(
            urls[0].ends_with("/api/v1/namespaces/default/pods/nginx"),
            "unexpected url: {}",
            urls[0]
        );
    }

    // ---------- describe_resource ----------

    #[test]
    fn describe_resource_combines_manifest_and_events() {
        // Two sequential responses: manifest, then events list.
        // Use thread-local index to avoid static counter races between tests.
        use crate::host::HttpResponse;
        use std::sync::{Arc, Mutex};

        struct SeqHttp {
            responses: Vec<Vec<u8>>,
            idx: Arc<Mutex<usize>>,
        }
        impl HttpTransport for SeqHttp {
            fn send(
                &self,
                _m: &str,
                _u: &str,
                _h: &[(String, String)],
                _b: Option<Vec<u8>>,
            ) -> Result<HttpResponse, String> {
                let mut idx = self.idx.lock().unwrap();
                let body = self
                    .responses
                    .get(*idx)
                    .unwrap_or_else(|| self.responses.last().unwrap())
                    .clone();
                *idx += 1;
                Ok(HttpResponse { status: 200, body })
            }
        }

        // Use well-formed JSON — duplicate keys are last-wins in serde_json
        let manifest = br#"{
            "status": {
                "phase": "Running",
                "conditions": [{"type": "Ready", "status": "True"}]
            }
        }"#
        .to_vec();
        let events = br#"{
            "items": [{
                "type": "Normal", "reason": "Started", "message": "pod started",
                "involvedObject": {"name": "nginx"},
                "count": 1, "lastTimestamp": "2024-01-01T00:00:00Z"
            }]
        }"#
        .to_vec();

        let http = SeqHttp {
            responses: vec![manifest, events],
            idx: Arc::new(Mutex::new(0)),
        };
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = describe_resource(&client, "v1", "pods", "nginx", Some("default")).unwrap();
        assert_eq!(out["status"]["phase"], "Running");
        let conditions = out["conditions"].as_array().unwrap();
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0]["type"], "Ready");
        let recent = out["recent_events"].as_array().unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0]["reason"], "Started");
    }

    // ---------- get_pod_logs ----------

    #[test]
    fn get_pod_logs_wraps_text_in_logs_field() {
        let http =
            CapturingHttp::new(b"2024-01-01 INFO app started\n2024-01-01 INFO ready".to_vec());
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = get_pod_logs(&client, "default", "mypod", Some("main"), Some(50), None).unwrap();
        assert!(
            out["logs"].as_str().unwrap().contains("INFO app started"),
            "unexpected logs: {}",
            out["logs"]
        );
        let urls = http.captured_urls.borrow();
        assert!(
            urls[0].contains("/pods/mypod/log"),
            "unexpected url: {}",
            urls[0]
        );
        assert!(
            urls[0].contains("tailLines"),
            "expected tailLines in url: {}",
            urls[0]
        );
    }

    // ---------- get_events ----------

    #[test]
    fn get_events_returns_shaped_events() {
        let body = br#"{
            "items": [{
                "type": "Warning", "reason": "OOMKilled", "message": "pod killed",
                "involvedObject": {"name": "crash-pod"},
                "count": 3, "lastTimestamp": "2024-06-01T10:00:00Z"
            }]
        }"#
        .to_vec();
        let http = OkHttp(body);
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = get_events(&client, Some("default"), Some("crash-pod")).unwrap();
        let events = out["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "Warning");
        assert_eq!(events[0]["reason"], "OOMKilled");
    }

    // ---------- top_pods ----------

    #[test]
    fn top_pods_returns_cpu_and_memory() {
        // Single container: 125m CPU stays 125m; 256Mi memory canonicalizes to
        // 256*1024 = 262144 Ki.
        let body = br#"{
            "items": [{
                "metadata": {"name": "api-pod"},
                "containers": [{"usage": {"cpu": "125m", "memory": "256Mi"}}]
            }]
        }"#
        .to_vec();
        let http = CapturingHttp::new(body);
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = top_pods(&client, Some("default")).unwrap();
        let pods = out["pods"].as_array().unwrap();
        assert_eq!(pods.len(), 1);
        assert_eq!(pods[0]["name"], "api-pod");
        assert_eq!(pods[0]["cpu"], "125m");
        assert_eq!(pods[0]["memory"], "262144Ki");
        let urls = http.captured_urls.borrow();
        assert!(
            urls[0].contains("/namespaces/default/pods"),
            "unexpected url: {}",
            urls[0]
        );
    }

    #[test]
    fn top_pods_sums_across_all_containers() {
        // Multi-container pod: usage is the SUM across every container so
        // sidecars (istio-proxy, log agents) are not silently dropped.
        // CPU: 100000000n + 50000000n = 150000000n = 150m
        // Mem: 4096Ki + 2048Ki = 6144Ki
        let body = br#"{
            "items": [{
                "metadata": {"name": "multi-pod"},
                "containers": [
                    {"usage": {"cpu": "100000000n", "memory": "4096Ki"}},
                    {"usage": {"cpu": "50000000n",  "memory": "2048Ki"}}
                ]
            }]
        }"#
        .to_vec();
        let http = CapturingHttp::new(body);
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = top_pods(&client, None).unwrap();
        let pods = out["pods"].as_array().unwrap();
        assert_eq!(pods.len(), 1);
        assert_eq!(pods[0]["name"], "multi-pod");
        assert_eq!(pods[0]["cpu"], "150m");
        assert_eq!(pods[0]["memory"], "6144Ki");
    }

    // ---------- quantity parsers ----------

    #[test]
    fn parse_cpu_nanocores_handles_all_suffixes() {
        assert_eq!(parse_cpu_nanocores("123456n"), 123_456);
        assert_eq!(parse_cpu_nanocores("5u"), 5_000);
        assert_eq!(parse_cpu_nanocores("250m"), 250_000_000);
        assert_eq!(parse_cpu_nanocores("2"), 2_000_000_000);
    }

    #[test]
    fn parse_cpu_nanocores_unparseable_is_zero() {
        assert_eq!(parse_cpu_nanocores(""), 0);
        assert_eq!(parse_cpu_nanocores("abc"), 0);
        assert_eq!(parse_cpu_nanocores("12.5m"), 0);
    }

    #[test]
    fn parse_mem_bytes_handles_binary_and_decimal_suffixes() {
        assert_eq!(parse_mem_bytes("4096Ki"), 4096 * 1024);
        assert_eq!(parse_mem_bytes("2Mi"), 2 * 1024 * 1024);
        assert_eq!(parse_mem_bytes("1Gi"), 1024 * 1024 * 1024);
        assert_eq!(parse_mem_bytes("3k"), 3_000);
        assert_eq!(parse_mem_bytes("512"), 512);
    }

    #[test]
    fn parse_mem_bytes_unparseable_is_zero() {
        assert_eq!(parse_mem_bytes(""), 0);
        assert_eq!(parse_mem_bytes("notanumber"), 0);
        assert_eq!(parse_mem_bytes("1.5Gi"), 0);
    }

    // ---------- top_nodes ----------

    #[test]
    fn top_nodes_returns_node_metrics() {
        let body = br#"{
            "items": [{
                "metadata": {"name": "node-1"},
                "usage": {"cpu": "500m", "memory": "4Gi"}
            }]
        }"#
        .to_vec();
        let http = CapturingHttp::new(body);
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = top_nodes(&client).unwrap();
        let nodes = out["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["name"], "node-1");
        assert_eq!(nodes[0]["cpu"], "500m");
        assert_eq!(nodes[0]["memory"], "4Gi");
        let urls = http.captured_urls.borrow();
        assert!(
            urls[0].ends_with("/metrics.k8s.io/v1beta1/nodes"),
            "unexpected url: {}",
            urls[0]
        );
    }

    // ---------- get_server_version ----------

    #[test]
    fn get_server_version_returns_parsed_body() {
        let body = br#"{"gitVersion":"v1.29.2","platform":"linux/arm64"}"#.to_vec();
        let http = CapturingHttp::new(body);
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = get_server_version(&client).unwrap();
        assert_eq!(out["gitVersion"], "v1.29.2");
        let urls = http.captured_urls.borrow();
        assert_eq!(
            urls[0], "https://api.example:6443/version",
            "unexpected url: {}",
            urls[0]
        );
    }

    // ---------- path-segment validation (FIX 2) ----------

    /// A `name` argument containing `/` must be rejected BEFORE any HTTP call.
    /// Asserts call-count == 0 (the fake transport tracks every send).
    #[test]
    fn get_resource_rejects_slash_in_name_and_issues_no_http_call() {
        let http = CapturingHttp::new(b"{}".to_vec());
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let err = get_resource(&client, "v1", "pods", "bad/name", Some("default")).unwrap_err();
        assert!(
            err.to_string().contains("invalid path segment"),
            "expected path-segment error, got: {err}"
        );
        // Security assertion: no HTTP call must have been issued.
        assert_eq!(
            http.captured_urls.borrow().len(),
            0,
            "get_resource must not issue any HTTP call when name is invalid"
        );
    }

    /// A dotted name like `my.config` is a valid K8s resource name and must
    /// NOT be over-rejected by the path-segment validator.
    #[test]
    fn get_resource_accepts_dotted_name() {
        let body = br#"{"kind":"ConfigMap","metadata":{"name":"my.config"}}"#.to_vec();
        let http = CapturingHttp::new(body);
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let out = get_resource(&client, "v1", "configmaps", "my.config", Some("default")).unwrap();
        assert_eq!(out["kind"], "ConfigMap");
        // Exactly one HTTP call should have been made.
        assert_eq!(
            http.captured_urls.borrow().len(),
            1,
            "get_resource should issue exactly one HTTP call for a valid dotted name"
        );
    }

    /// A `namespace` argument containing `?` must be rejected BEFORE any HTTP call.
    #[test]
    fn get_resource_rejects_question_mark_in_namespace_and_issues_no_http_call() {
        let http = CapturingHttp::new(b"{}".to_vec());
        let creds = creds();
        let client = K8sClient {
            creds: &creds,
            http: &http,
        };
        let err = get_resource(&client, "v1", "pods", "nginx", Some("ns?inject")).unwrap_err();
        assert!(
            err.to_string().contains("invalid path segment"),
            "expected path-segment error, got: {err}"
        );
        assert_eq!(
            http.captured_urls.borrow().len(),
            0,
            "get_resource must not issue any HTTP call when namespace is invalid"
        );
    }
}
