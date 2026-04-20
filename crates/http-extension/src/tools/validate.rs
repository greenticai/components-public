//! `validate_http_config` — diagnostics for a generated YGTc HTTP node.
use http_core::auth::AuthType;
use http_core::url::validate_url;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub fn validate_http_config(node: &Value) -> (bool, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let cfg = node.get("config").cloned().unwrap_or(Value::Null);

    // base_url
    match cfg.get("base_url").and_then(Value::as_str) {
        Some(u) => {
            if let Err(e) = validate_url(u) {
                diags.push(err(
                    "url:invalid-scheme",
                    format!("{e}"),
                    Some("config.base_url"),
                ));
            }
        }
        None => diags.push(err(
            "url:missing",
            "base_url is required".into(),
            Some("config.base_url"),
        )),
    }

    // auth_type + token
    let auth_type_str = cfg
        .get("auth_type")
        .and_then(Value::as_str)
        .unwrap_or("none");
    match AuthType::from_str(auth_type_str) {
        None => diags.push(err(
            "auth:unsupported-type",
            format!("unsupported auth_type: {auth_type_str}"),
            Some("config.auth_type"),
        )),
        Some(AuthType::None) => {}
        Some(_) => match cfg.get("auth_token").and_then(Value::as_str) {
            None => diags.push(err(
                "auth:missing-token",
                "auth_token required for this auth_type".into(),
                Some("config.auth_token"),
            )),
            Some(t) if !t.starts_with("secret:") => {
                diags.push(warn(
                    "auth:bare-token",
                    "auth_token looks like raw token; use secret:NAME reference".into(),
                    Some("config.auth_token"),
                ));
            }
            _ => {}
        },
    }

    // timeout
    if let Some(t) = cfg.get("timeout_ms").and_then(Value::as_u64) {
        if t == 0 {
            diags.push(err(
                "timeout:zero",
                "timeout_ms must be > 0".into(),
                Some("config.timeout_ms"),
            ));
        }
        if t > 60_000 {
            diags.push(warn(
                "timeout:too-long",
                format!("timeout_ms {t} exceeds recommended 60000"),
                Some("config.timeout_ms"),
            ));
        }
    }

    let valid = !diags.iter().any(|d| d.severity == Severity::Error);
    (valid, diags)
}

fn err(code: &str, msg: String, path: Option<&str>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        code: code.into(),
        message: msg,
        path: path.map(String::from),
    }
}
fn warn(code: &str, msg: String, path: Option<&str>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        code: code.into(),
        message: msg,
        path: path.map(String::from),
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use serde_json::json;

    fn has_code(diags: &[Diagnostic], code: &str) -> bool {
        diags.iter().any(|d| d.code == code)
    }

    #[test]
    fn rejects_invalid_scheme() {
        let node = json!({
            "node_id": "x",
            "component": "oci://r:1",
            "config": { "base_url": "file:///etc/passwd", "auth_type": "none", "timeout_ms": 15000 },
            "inputs": {}
        });
        let (valid, diags) = validate_http_config(&node);
        assert!(!valid);
        assert!(has_code(&diags, "url:invalid-scheme"));
    }

    #[test]
    fn warns_on_bare_token() {
        let node = json!({
            "config": { "base_url": "https://x", "auth_type": "bearer", "auth_token": "rawtokenABCDEFG", "timeout_ms": 15000 },
            "node_id": "x", "component": "oci://r", "inputs": {}
        });
        let (_valid, diags) = validate_http_config(&node);
        assert!(has_code(&diags, "auth:bare-token"));
        assert_eq!(
            diags
                .iter()
                .find(|d| d.code == "auth:bare-token")
                .unwrap()
                .severity,
            Severity::Warning
        );
    }

    #[test]
    fn warns_on_excessive_timeout() {
        let node = json!({
            "config": { "base_url": "https://x", "auth_type": "none", "timeout_ms": 120000 },
            "node_id": "x", "component": "oci://r", "inputs": {}
        });
        let (_valid, diags) = validate_http_config(&node);
        assert!(has_code(&diags, "timeout:too-long"));
    }

    #[test]
    fn valid_node_has_no_errors() {
        let node = json!({
            "node_id": "post_users",
            "component": "oci://ghcr.io/greenticai/component/component-http:0.1.0",
            "config": { "base_url": "https://api.example.com", "auth_type": "bearer", "auth_token": "secret:HTTP_TOKEN", "timeout_ms": 15000 },
            "inputs": { "method": "POST", "path": "/users" }
        });
        let (valid, diags) = validate_http_config(&node);
        assert!(valid, "diagnostics: {diags:?}");
    }
}
