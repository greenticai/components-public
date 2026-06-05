//! `validate_webhook_config` — diagnostics for a webhook trigger config block.
//!
//! Input shape:
//!   { "node": { "config": { method, path, auth, signature_validation, allowed_sources } } }
//!
//! Field semantics mirror the JSON Schema in `describe.json` — keep these in sync.

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

const ALLOWED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH"];
const ALLOWED_AUTH_TYPES: &[&str] = &["none", "bearer", "hmac", "basic"];
const ALLOWED_SIG_ALGOS: &[&str] = &["hmac-sha256", "hmac-sha1"];

pub fn validate_webhook_config(node: &Value) -> (bool, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let cfg = node.get("config").cloned().unwrap_or(Value::Null);

    validate_method(&cfg, &mut diags);
    validate_path(&cfg, &mut diags);
    validate_auth(&cfg, &mut diags);
    validate_signature(&cfg, &mut diags);
    validate_allowed_sources(&cfg, &mut diags);

    let valid = !diags.iter().any(|d| d.severity == Severity::Error);
    (valid, diags)
}

fn validate_method(cfg: &Value, diags: &mut Vec<Diagnostic>) {
    match cfg.get("method").and_then(Value::as_str) {
        None => diags.push(err(
            "method:missing",
            "method is required".into(),
            Some("config.method"),
        )),
        Some(m) if !ALLOWED_METHODS.contains(&m) => diags.push(err(
            "method:unsupported",
            format!("method '{m}' is not in {:?}", ALLOWED_METHODS),
            Some("config.method"),
        )),
        _ => {}
    }
}

fn validate_path(cfg: &Value, diags: &mut Vec<Diagnostic>) {
    let path = match cfg.get("path").and_then(Value::as_str) {
        Some(p) => p,
        None => {
            diags.push(err(
                "path:missing",
                "path is required".into(),
                Some("config.path"),
            ));
            return;
        }
    };

    if !path.starts_with('/') {
        diags.push(err(
            "path:no-leading-slash",
            format!("path '{path}' must start with '/'"),
            Some("config.path"),
        ));
    }
    if path.chars().any(char::is_whitespace) {
        diags.push(err(
            "path:whitespace",
            "path must not contain whitespace".into(),
            Some("config.path"),
        ));
    }
    if path.contains('?') || path.contains('#') {
        diags.push(err(
            "path:has-query-or-fragment",
            "path must not contain '?' or '#'; runtime matches on path only".into(),
            Some("config.path"),
        ));
    }
    if path.len() > 1 && path.ends_with('/') {
        diags.push(warn(
            "path:trailing-slash",
            "trailing slash is usually not what you want — runtime matches exactly".into(),
            Some("config.path"),
        ));
    }
}

fn validate_auth(cfg: &Value, diags: &mut Vec<Diagnostic>) {
    let Some(auth) = cfg.get("auth") else { return };
    let auth_type = auth.get("type").and_then(Value::as_str).unwrap_or("none");
    if !ALLOWED_AUTH_TYPES.contains(&auth_type) {
        diags.push(err(
            "auth:unsupported-type",
            format!("auth.type '{auth_type}' is not in {:?}", ALLOWED_AUTH_TYPES),
            Some("config.auth.type"),
        ));
        return;
    }
    if auth_type == "none" {
        return;
    }
    match auth.get("secret_ref").and_then(Value::as_str) {
        None => diags.push(err(
            "auth:missing-secret-ref",
            format!("auth.secret_ref is required when auth.type is '{auth_type}'"),
            Some("config.auth.secret_ref"),
        )),
        Some("") => diags.push(err(
            "auth:empty-secret-ref",
            "auth.secret_ref must not be empty".into(),
            Some("config.auth.secret_ref"),
        )),
        Some(s) if s.starts_with("secret:") => diags.push(warn(
            "auth:redundant-secret-prefix",
            "secret_ref should be the bare secret name (e.g. 'WEBHOOK_TOKEN'); the 'secret:' prefix is added by the runtime".into(),
            Some("config.auth.secret_ref"),
        )),
        _ => {}
    }
}

fn validate_signature(cfg: &Value, diags: &mut Vec<Diagnostic>) {
    let Some(sig) = cfg.get("signature_validation") else {
        return;
    };
    if sig.get("header").and_then(Value::as_str).is_none() {
        diags.push(err(
            "signature:missing-header",
            "signature_validation.header is required when the block is present".into(),
            Some("config.signature_validation.header"),
        ));
    }
    if sig.get("secret_ref").and_then(Value::as_str).is_none() {
        diags.push(err(
            "signature:missing-secret-ref",
            "signature_validation.secret_ref is required when the block is present".into(),
            Some("config.signature_validation.secret_ref"),
        ));
    }
    if let Some(algo) = sig.get("algorithm").and_then(Value::as_str)
        && !ALLOWED_SIG_ALGOS.contains(&algo)
    {
        diags.push(err(
            "signature:unsupported-algorithm",
            format!(
                "signature_validation.algorithm '{algo}' is not in {:?}",
                ALLOWED_SIG_ALGOS
            ),
            Some("config.signature_validation.algorithm"),
        ));
    }
}

fn validate_allowed_sources(cfg: &Value, diags: &mut Vec<Diagnostic>) {
    let Some(arr) = cfg.get("allowed_sources").and_then(Value::as_array) else {
        return;
    };
    for (i, item) in arr.iter().enumerate() {
        let Some(s) = item.as_str() else {
            diags.push(err(
                "allowed-sources:not-string",
                format!("allowed_sources[{i}] is not a string"),
                Some(&format!("config.allowed_sources[{i}]")),
            ));
            continue;
        };
        if !is_valid_cidr_or_ip(s) {
            diags.push(err(
                "allowed-sources:invalid",
                format!("allowed_sources[{i}] = '{s}' is not a valid IP or CIDR"),
                Some(&format!("config.allowed_sources[{i}]")),
            ));
        }
    }
}

/// Permissive IP/CIDR check — covers IPv4 (a.b.c.d, a.b.c.d/n) and IPv6 (`:` present, optional `/n`).
/// Designed to catch obvious typos, not to be a full RFC-correct validator (the runtime does that).
fn is_valid_cidr_or_ip(s: &str) -> bool {
    let (addr, prefix) = match s.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (s, None),
    };
    let is_ipv6 = addr.contains(':');
    if is_ipv6 {
        // accept anything with hex chars, ':' and at most one '::'
        let occurrences = addr.matches("::").count();
        if occurrences > 1 {
            return false;
        }
        if !addr
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        {
            return false;
        }
        if let Some(p) = prefix {
            return p.parse::<u8>().map(|n| n <= 128).unwrap_or(false);
        }
        return true;
    }
    let octets: Vec<&str> = addr.split('.').collect();
    if octets.len() != 4 {
        return false;
    }
    if !octets.iter().all(|o| o.parse::<u8>().is_ok()) {
        return false;
    }
    if let Some(p) = prefix {
        return p.parse::<u8>().map(|n| n <= 32).unwrap_or(false);
    }
    true
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
    fn valid_minimal_config_passes() {
        let node = json!({"config": {"method": "POST", "path": "/webhooks/orders"}});
        let (valid, diags) = validate_webhook_config(&node);
        assert!(valid, "diagnostics: {diags:?}");
    }

    #[test]
    fn missing_method_and_path_each_error() {
        let node = json!({"config": {}});
        let (valid, diags) = validate_webhook_config(&node);
        assert!(!valid);
        assert!(has_code(&diags, "method:missing"));
        assert!(has_code(&diags, "path:missing"));
    }

    #[test]
    fn rejects_unknown_method() {
        let node = json!({"config": {"method": "DELETE", "path": "/x"}});
        let (valid, diags) = validate_webhook_config(&node);
        assert!(!valid);
        assert!(has_code(&diags, "method:unsupported"));
    }

    #[test]
    fn rejects_path_without_leading_slash() {
        let node = json!({"config": {"method": "POST", "path": "webhook/x"}});
        let (_v, diags) = validate_webhook_config(&node);
        assert!(has_code(&diags, "path:no-leading-slash"));
    }

    #[test]
    fn rejects_path_with_query() {
        let node = json!({"config": {"method": "POST", "path": "/x?a=1"}});
        let (_v, diags) = validate_webhook_config(&node);
        assert!(has_code(&diags, "path:has-query-or-fragment"));
    }

    #[test]
    fn warns_on_trailing_slash() {
        let node = json!({"config": {"method": "POST", "path": "/webhooks/x/"}});
        let (_v, diags) = validate_webhook_config(&node);
        assert!(has_code(&diags, "path:trailing-slash"));
    }

    #[test]
    fn auth_bearer_requires_secret_ref() {
        let node = json!({"config": {"method": "POST", "path": "/x", "auth": {"type": "bearer"}}});
        let (valid, diags) = validate_webhook_config(&node);
        assert!(!valid);
        assert!(has_code(&diags, "auth:missing-secret-ref"));
    }

    #[test]
    fn auth_none_does_not_require_secret_ref() {
        let node = json!({"config": {"method": "POST", "path": "/x", "auth": {"type": "none"}}});
        let (valid, _diags) = validate_webhook_config(&node);
        assert!(valid);
    }

    #[test]
    fn warns_on_secret_prefix_in_secret_ref() {
        let node = json!({"config": {
            "method": "POST", "path": "/x",
            "auth": {"type": "bearer", "secret_ref": "secret:WEBHOOK_TOKEN"}
        }});
        let (_v, diags) = validate_webhook_config(&node);
        assert!(has_code(&diags, "auth:redundant-secret-prefix"));
    }

    #[test]
    fn signature_block_requires_header_and_secret() {
        let node = json!({"config": {
            "method": "POST", "path": "/x",
            "signature_validation": {"algorithm": "hmac-sha256"}
        }});
        let (valid, diags) = validate_webhook_config(&node);
        assert!(!valid);
        assert!(has_code(&diags, "signature:missing-header"));
        assert!(has_code(&diags, "signature:missing-secret-ref"));
    }

    #[test]
    fn signature_unknown_algorithm_errors() {
        let node = json!({"config": {
            "method": "POST", "path": "/x",
            "signature_validation": {
                "header": "X-Sig", "secret_ref": "S", "algorithm": "md5"
            }
        }});
        let (valid, diags) = validate_webhook_config(&node);
        assert!(!valid);
        assert!(has_code(&diags, "signature:unsupported-algorithm"));
    }

    #[test]
    fn allowed_sources_validates_cidr() {
        let node = json!({"config": {
            "method": "POST", "path": "/x",
            "allowed_sources": ["10.0.0.0/8", "203.0.113.42", "not-an-ip"]
        }});
        let (valid, diags) = validate_webhook_config(&node);
        assert!(!valid);
        assert!(has_code(&diags, "allowed-sources:invalid"));
    }

    #[test]
    fn ipv6_cidr_accepted() {
        let node = json!({"config": {
            "method": "POST", "path": "/x",
            "allowed_sources": ["2001:db8::/32", "::1"]
        }});
        let (valid, diags) = validate_webhook_config(&node);
        assert!(valid, "diagnostics: {diags:?}");
    }
}
