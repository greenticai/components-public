//! Typed Kubernetes REST access over the host HTTP transport.
use crate::clusters::ClusterCreds;
use crate::host::HttpTransport;
use serde_json::Value;
use std::fmt;

pub struct K8sClient<'a, T: HttpTransport> {
    pub creds: &'a ClusterCreds,
    pub http: &'a T,
}

#[derive(Debug)]
pub enum K8sError {
    Http(String),
    Status { code: u16, message: String },
    Decode(String),
}

impl fmt::Display for K8sError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(e) => write!(f, "http error: {e}"),
            Self::Status { code, message } => write!(f, "kubernetes API {code}: {message}"),
            Self::Decode(e) => write!(f, "decode error: {e}"),
        }
    }
}

/// Percent-encode a string per RFC 3986: leave the unreserved set
/// (`A-Za-z0-9` and `-` `_` `.` `~`) intact and escape every other byte as
/// uppercase `%XX`. Space becomes `%20` (never `+`). Needed because Kubernetes
/// selector values (`labelSelector`, `fieldSelector`) carry `=`, `,`, spaces,
/// `(`, `)` which would otherwise be misparsed by the API server.
fn percent_encode(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

fn encode_query(query: &[(&str, &str)]) -> String {
    if query.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = query
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect();
    format!("?{}", parts.join("&"))
}

/// Build the full request URL by appending `path` to the registered `api_url`
/// and optionally a query string. The `api_url` is the single allowlisted origin —
/// `path` is always appended to it, never used as a URL base.
#[must_use]
pub fn url_for(api_url: &str, path: &str, query: &[(&str, &str)]) -> String {
    format!("{api_url}{path}{}", encode_query(query))
}

impl<T: HttpTransport> K8sClient<'_, T> {
    fn build_headers(&self, content_type: Option<&str>) -> Vec<(String, String)> {
        let mut headers = vec![
            (
                "authorization".into(),
                format!("Bearer {}", self.creds.token),
            ),
            ("accept".into(), "application/json".into()),
        ];
        if let Some(ct) = content_type {
            headers.push(("content-type".into(), ct.into()));
        }
        headers
    }

    /// Send an arbitrary request to the Kubernetes API.
    pub fn send(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        content_type: Option<&str>,
        body: Option<Vec<u8>>,
    ) -> Result<Value, K8sError> {
        let url = url_for(&self.creds.api_url, path, query);
        let response = self
            .http
            .send(method, &url, &self.build_headers(content_type), body)
            .map_err(K8sError::Http)?;

        if !(200..300).contains(&response.status) {
            let message = String::from_utf8_lossy(&response.body).to_string();
            return Err(K8sError::Status {
                code: response.status,
                message,
            });
        }

        if response.body.is_empty() {
            return Ok(Value::Null);
        }

        serde_json::from_slice(&response.body).map_err(|e| K8sError::Decode(e.to_string()))
    }

    /// Convenience wrapper for GET requests with no query parameters.
    pub fn get(&self, path: &str) -> Result<Value, K8sError> {
        self.send("GET", path, &[], None, None)
    }

    /// GET a path and return the raw response body as a `String` (for non-JSON
    /// endpoints such as pod logs). Returns a UTF-8-lossy string for 2xx
    /// responses; maps non-2xx to `K8sError::Status`.
    pub fn get_text(&self, path: &str, query: &[(&str, &str)]) -> Result<String, K8sError> {
        let url = url_for(&self.creds.api_url, path, query);
        let response = self
            .http
            .send("GET", &url, &self.build_headers(None), None)
            .map_err(K8sError::Http)?;

        if !(200..300).contains(&response.status) {
            let message = String::from_utf8_lossy(&response.body).to_string();
            return Err(K8sError::Status {
                code: response.status,
                message,
            });
        }

        Ok(String::from_utf8_lossy(&response.body).to_string())
    }
}

/// Validate a caller-supplied path segment before it is placed into a Kubernetes
/// REST URL path.
///
/// Returns `true` only when the segment is non-empty, no longer than 253
/// characters, and every character is ASCII alphanumeric or one of `-`, `.`,
/// `_`. This allowlist mirrors the RFC 1123 label rules used by Kubernetes
/// resource names and explicitly rejects `/`, `?`, `#`, `%`, `:`, space, and
/// control characters that could re-target the request within the cluster API.
///
/// Note: `..` alone cannot cause path traversal without a `/` separator, so it
/// is allowed by this check (Kubernetes does not permit it as a resource name
/// anyway, but that is enforced server-side).
#[must_use]
pub fn valid_path_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
}

/// Build the collection path for a resource.
///
/// `api_version` is `"v1"` (core API group) or `"group/version"` for named
/// groups (e.g. `"apps/v1"`). `kind_plural` is the lowercase plural resource
/// type (e.g. `"pods"`, `"deployments"`). `namespace` scopes the path to a
/// specific namespace when `Some`.
///
/// # Examples
/// ```
/// use component_k8s::k8s::collection_path;
/// assert_eq!(collection_path("v1", "pods", Some("default")),
///            "/api/v1/namespaces/default/pods");
/// assert_eq!(collection_path("apps/v1", "deployments", None),
///            "/apis/apps/v1/deployments");
/// ```
#[must_use]
pub fn collection_path(api_version: &str, kind_plural: &str, namespace: Option<&str>) -> String {
    let base = if api_version == "v1" {
        "/api/v1".to_string()
    } else {
        format!("/apis/{api_version}")
    };
    match namespace {
        Some(ns) => format!("{base}/namespaces/{ns}/{kind_plural}"),
        None => format!("{base}/{kind_plural}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clusters::ClusterCreds;
    use crate::host::{HttpResponse, HttpTransport};
    use std::cell::RefCell;

    struct FakeHttp {
        captured: RefCell<Option<(String, String)>>, // (method, url)
        response: HttpResponse,
    }
    impl HttpTransport for FakeHttp {
        fn send(
            &self,
            method: &str,
            url: &str,
            _h: &[(String, String)],
            _b: Option<Vec<u8>>,
        ) -> Result<HttpResponse, String> {
            *self.captured.borrow_mut() = Some((method.to_string(), url.to_string()));
            Ok(HttpResponse {
                status: self.response.status,
                body: self.response.body.clone(),
            })
        }
    }
    fn creds() -> ClusterCreds {
        ClusterCreds {
            api_url: "https://api.example:6443".into(),
            token: "t".into(),
            allow_write: true,
        }
    }

    #[test]
    fn get_builds_url_and_decodes_json() {
        let http = FakeHttp {
            captured: RefCell::new(None),
            response: HttpResponse {
                status: 200,
                body: br#"{"kind":"PodList"}"#.to_vec(),
            },
        };
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        let v = client.get("/api/v1/namespaces/default/pods").unwrap();
        assert_eq!(v["kind"], "PodList");
        let (method, url) = http.captured.borrow().clone().unwrap();
        assert_eq!(method, "GET");
        assert_eq!(
            url,
            "https://api.example:6443/api/v1/namespaces/default/pods"
        );
    }

    #[test]
    fn non_2xx_is_structured_status_error() {
        let http = FakeHttp {
            captured: RefCell::new(None),
            response: HttpResponse {
                status: 404,
                body: br#"{"message":"not found"}"#.to_vec(),
            },
        };
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        match client.get("/api/v1/x").unwrap_err() {
            K8sError::Status { code, message } => {
                assert_eq!(code, 404);
                assert!(message.contains("not found"));
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn dry_run_query_is_appended() {
        let http = FakeHttp {
            captured: RefCell::new(None),
            response: HttpResponse {
                status: 200,
                body: b"{}".to_vec(),
            },
        };
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        client
            .send(
                "PATCH",
                "/apis/apps/v1/namespaces/default/deployments/web",
                &[("dryRun", "All")],
                Some("application/strategic-merge-patch+json"),
                Some(b"{}".to_vec()),
            )
            .unwrap();
        let (_m, url) = http.captured.borrow().clone().unwrap();
        assert!(url.ends_with("/deployments/web?dryRun=All"), "got {url}");
    }

    #[test]
    fn selector_query_value_is_percent_encoded() {
        let url = url_for(
            "https://api.example:6443",
            "/api/v1/pods",
            &[("labelSelector", "app=nginx,tier=frontend")],
        );
        assert!(
            url.ends_with("?labelSelector=app%3Dnginx%2Ctier%3Dfrontend"),
            "got {url}"
        );
    }

    #[test]
    fn space_in_query_value_encodes_to_percent_20() {
        let url = url_for(
            "https://api.example:6443",
            "/api/v1/pods",
            &[("fieldSelector", "status.phase = Running")],
        );
        assert!(
            url.ends_with("?fieldSelector=status.phase%20%3D%20Running"),
            "got {url}"
        );
    }

    // --- collection_path tests ---

    #[test]
    fn collection_path_core_namespaced() {
        assert_eq!(
            super::collection_path("v1", "pods", Some("default")),
            "/api/v1/namespaces/default/pods"
        );
    }

    #[test]
    fn collection_path_core_cluster_scoped() {
        assert_eq!(super::collection_path("v1", "nodes", None), "/api/v1/nodes");
    }

    #[test]
    fn collection_path_grouped_namespaced() {
        assert_eq!(
            super::collection_path("apps/v1", "deployments", Some("kube-system")),
            "/apis/apps/v1/namespaces/kube-system/deployments"
        );
    }

    #[test]
    fn collection_path_grouped_cluster_scoped() {
        assert_eq!(
            super::collection_path("apps/v1", "deployments", None),
            "/apis/apps/v1/deployments"
        );
    }

    // --- get_text tests ---

    #[test]
    fn get_text_returns_raw_string_for_2xx() {
        let http = FakeHttp {
            captured: RefCell::new(None),
            response: HttpResponse {
                status: 200,
                body: b"line1\nline2\nline3".to_vec(),
            },
        };
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        let text = client
            .get_text("/api/v1/namespaces/default/pods/mypod/log", &[])
            .unwrap();
        assert_eq!(text, "line1\nline2\nline3");
        let (_m, url) = http.captured.borrow().clone().unwrap();
        assert!(url.ends_with("/pods/mypod/log"), "unexpected url: {url}");
    }

    #[test]
    fn get_text_errors_on_non_2xx() {
        let http = FakeHttp {
            captured: RefCell::new(None),
            response: HttpResponse {
                status: 404,
                body: b"not found".to_vec(),
            },
        };
        let c = creds();
        let client = K8sClient {
            creds: &c,
            http: &http,
        };
        match client.get_text("/api/v1/x", &[]).unwrap_err() {
            K8sError::Status { code, .. } => assert_eq!(code, 404),
            other => panic!("expected Status, got {other:?}"),
        }
    }
}
