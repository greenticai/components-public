//! `validate_webhook_config` — diagnostics for a webhook trigger node config in
//! the shape of trigger contract v1 (greentic-designer
//! `docs/trigger-contract-v1.md` §4 rule 5, §6.3.2–§6.3.5).
//!
//! Input shape:
//!   { "node": { "config": { trigger_id?, enabled?, methods?, verify,
//!                           challenge?, idempotency?, max_body_bytes?,
//!                           allowed_sources?, session?, limits? } } }
//!
//! The designer re-validates on build and greentic-start again on load; this
//! tool exists so the designer LLM and the operator see a problem while
//! editing, in words that say what to change. Keep it in step with
//! `describe.json`'s `config_schema` and with the contract.

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

const ALLOWED_METHODS: &[&str] = &["POST", "PUT", "PATCH", "GET"];
const VERIFY_SCHEMES: &[&str] = &["none", "hmac-sha256", "hmac-sha1", "bearer"];
const CHALLENGE_SCHEMES: &[&str] = &["meta_hub"];
const MAX_BODY_BYTES: u64 = 5 * 1024 * 1024;
const MAX_IDEMPOTENCY_TTL_S: u64 = 604_800;

pub fn validate_webhook_config(node: &Value) -> (bool, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let cfg = node.get("config").cloned().unwrap_or(Value::Null);

    if is_pre_contract(&cfg) {
        diags.push(err(
            "config:pre-contract-shape",
            "this node uses the old shape (method / path / auth). Set `verify` \
             (scheme + secret_ref) instead: the route is /trigger/<trigger_id>, and the \
             runtime verifies the request itself"
                .into(),
            Some("config"),
        ));
    }
    validate_trigger_id(&cfg, &mut diags);
    let methods = validate_methods(&cfg, &mut diags);
    validate_verify(&cfg, &mut diags);
    validate_challenge(&cfg, &methods, &mut diags);
    validate_idempotency(&cfg, &mut diags);
    validate_max_body(&cfg, &mut diags);
    validate_allowed_sources(&cfg, &mut diags);

    let valid = !diags.iter().any(|d| d.severity == Severity::Error);
    (valid, diags)
}

fn is_pre_contract(cfg: &Value) -> bool {
    cfg.get("verify").is_none()
        && ["method", "path", "auth", "signature_validation"]
            .iter()
            .any(|k| cfg.get(*k).is_some())
}

fn validate_trigger_id(cfg: &Value, diags: &mut Vec<Diagnostic>) {
    let Some(id) = cfg.get("trigger_id") else {
        return;
    };
    let ok = id.as_str().is_some_and(is_valid_trigger_id);
    if !ok {
        diags.push(err(
            "trigger_id:invalid",
            "trigger_id must match [a-z0-9][a-z0-9_-]{0,62}; it becomes the URL \
             segment /trigger/<trigger_id>"
                .into(),
            Some("config.trigger_id"),
        ));
    }
}

fn validate_methods(cfg: &Value, diags: &mut Vec<Diagnostic>) -> Vec<String> {
    let Some(methods) = cfg.get("methods") else {
        return vec!["POST".into()];
    };
    let Some(list) = methods.as_array() else {
        diags.push(err(
            "methods:not-a-list",
            "methods must be a list".into(),
            Some("config.methods"),
        ));
        return Vec::new();
    };
    let mut out = Vec::new();
    for m in list {
        match m.as_str().map(str::to_ascii_uppercase) {
            Some(m) if ALLOWED_METHODS.contains(&m.as_str()) => out.push(m),
            other => diags.push(err(
                "methods:unsupported",
                format!("method {other:?} is not in {ALLOWED_METHODS:?}"),
                Some("config.methods"),
            )),
        }
    }
    out
}

fn validate_verify(cfg: &Value, diags: &mut Vec<Diagnostic>) {
    let Some(verify) = cfg.get("verify") else {
        diags.push(err(
            "verify:missing",
            "verify is required; write { \"scheme\": \"none\" } to accept unverified \
             calls deliberately"
                .into(),
            Some("config.verify"),
        ));
        return;
    };
    let scheme = verify.get("scheme").and_then(Value::as_str).unwrap_or("");
    if !VERIFY_SCHEMES.contains(&scheme) {
        diags.push(err(
            "verify:unsupported-scheme",
            format!("verify.scheme '{scheme}' is not in {VERIFY_SCHEMES:?}"),
            Some("config.verify.scheme"),
        ));
        return;
    }
    if scheme == "none" {
        diags.push(warn(
            "verify:none",
            "anyone who knows this trigger's URL can start the flow".into(),
            Some("config.verify.scheme"),
        ));
        return;
    }
    if scheme.starts_with("hmac") && verify.get("header").and_then(Value::as_str).is_none() {
        diags.push(err(
            "verify:missing-header",
            "verify.header is required for an HMAC scheme (e.g. X-Hub-Signature-256)".into(),
            Some("config.verify.header"),
        ));
    }
    if let Some(enc) = verify.get("encoding").and_then(Value::as_str)
        && enc != "hex"
        && enc != "base64"
    {
        diags.push(err(
            "verify:bad-encoding",
            format!("verify.encoding '{enc}' must be hex or base64"),
            Some("config.verify.encoding"),
        ));
    }
    check_ref(verify.get("secret_ref"), "config.verify.secret_ref", diags);
}

fn validate_challenge(cfg: &Value, methods: &[String], diags: &mut Vec<Diagnostic>) {
    let Some(challenge) = cfg.get("challenge") else {
        return;
    };
    let scheme = challenge
        .get("scheme")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !CHALLENGE_SCHEMES.contains(&scheme) {
        diags.push(err(
            "challenge:unsupported-scheme",
            format!("challenge.scheme '{scheme}' is not in {CHALLENGE_SCHEMES:?}"),
            Some("config.challenge.scheme"),
        ));
    }
    check_ref(
        challenge.get("verify_token_ref"),
        "config.challenge.verify_token_ref",
        diags,
    );
    if methods.iter().any(|m| m == "GET") {
        diags.push(err(
            "challenge:get-fires",
            "GET is reserved for the subscription handshake when a challenge is set; \
             remove it from methods"
                .into(),
            Some("config.methods"),
        ));
    }
}

fn validate_idempotency(cfg: &Value, diags: &mut Vec<Diagnostic>) {
    let Some(idem) = cfg.get("idempotency") else {
        return;
    };
    let key = idem.get("key").and_then(Value::as_str).unwrap_or("");
    let ok = key.starts_with("header:") || key.starts_with("query:") || key.starts_with("body.");
    if !ok || key.ends_with(':') || key == "body." {
        diags.push(err(
            "idempotency:bad-key",
            "idempotency.key must be header:<name>, query:<name> or body.<path>".into(),
            Some("config.idempotency.key"),
        ));
    }
    if let Some(ttl) = idem.get("ttl_s")
        && !ttl
            .as_u64()
            .is_some_and(|n| (1..=MAX_IDEMPOTENCY_TTL_S).contains(&n))
    {
        diags.push(err(
            "idempotency:bad-ttl",
            format!("idempotency.ttl_s must be 1..={MAX_IDEMPOTENCY_TTL_S}"),
            Some("config.idempotency.ttl_s"),
        ));
    }
}

fn validate_max_body(cfg: &Value, diags: &mut Vec<Diagnostic>) {
    if let Some(v) = cfg.get("max_body_bytes")
        && !v
            .as_u64()
            .is_some_and(|n| (1..=MAX_BODY_BYTES).contains(&n))
    {
        diags.push(err(
            "max_body_bytes:out-of-range",
            format!("max_body_bytes must be 1..={MAX_BODY_BYTES}"),
            Some("config.max_body_bytes"),
        ));
    }
}

fn validate_allowed_sources(cfg: &Value, diags: &mut Vec<Diagnostic>) {
    let Some(list) = cfg.get("allowed_sources").and_then(Value::as_array) else {
        return;
    };
    if !list.is_empty() {
        diags.push(warn(
            "allowed_sources:not-enforced",
            "the current runtime does not enforce allowed_sources yet and will refuse to \
             serve this trigger (503) rather than skip the check; leave it empty and \
             rely on verify"
                .into(),
            Some("config.allowed_sources"),
        ));
    }
}

/// A secret REFERENCE: `<provider>/<key>`, never the secret itself. The message
/// never echoes the value — the likeliest cause is a pasted credential.
fn check_ref(value: Option<&Value>, path: &str, diags: &mut Vec<Diagnostic>) {
    match value.and_then(Value::as_str) {
        None => diags.push(err(
            "secret_ref:missing",
            "a secret reference is required here, e.g. threads/app_secret".into(),
            Some(path),
        )),
        Some(v) if !is_valid_secret_ref(v) => diags.push(err(
            "secret_ref:not-a-reference",
            "this must NAME a secret as <provider>/<key> (e.g. threads/app_secret), not \
             contain its value"
                .into(),
            Some(path),
        )),
        _ => {}
    }
}

pub(crate) fn is_valid_trigger_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 63
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

pub(crate) fn is_valid_secret_ref(value: &str) -> bool {
    fn segment_ok(s: &str) -> bool {
        let b = s.as_bytes();
        !b.is_empty()
            && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
            && b.iter().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'-')
            })
    }
    match value.split_once('/') {
        Some((provider, key)) => !key.contains('/') && segment_ok(provider) && segment_ok(key),
        None => false,
    }
}

fn err(code: &str, message: String, path: Option<&str>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        code: code.into(),
        message,
        path: path.map(str::to_string),
    }
}

fn warn(code: &str, message: String, path: Option<&str>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        code: code.into(),
        message,
        path: path.map(str::to_string),
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use serde_json::json;

    fn codes(cfg: Value) -> (bool, Vec<String>) {
        let (valid, diags) = validate_webhook_config(&json!({ "config": cfg }));
        (valid, diags.into_iter().map(|d| d.code).collect())
    }

    #[test]
    fn a_meta_threads_config_is_valid() {
        let (valid, codes) = codes(json!({
            "trigger_id": "threads_replies",
            "methods": ["POST"],
            "verify": {"scheme": "hmac-sha256", "header": "X-Hub-Signature-256",
                       "prefix": "sha256=", "encoding": "hex", "secret_ref": "threads/app_secret"},
            "challenge": {"scheme": "meta_hub", "verify_token_ref": "threads/verify_token"},
        }));
        assert!(valid, "{codes:?}");
        assert!(codes.is_empty(), "{codes:?}");
    }

    #[test]
    fn verify_is_required_and_none_is_a_warning_not_an_error() {
        assert!(!codes(json!({})).0);
        let (valid, codes) = codes(json!({"verify": {"scheme": "none"}}));
        assert!(valid);
        assert_eq!(codes, vec!["verify:none".to_string()]);
    }

    #[test]
    fn a_pasted_credential_is_refused_without_echoing_it() {
        let (valid, diags) = validate_webhook_config(&json!({"config": {
            "verify": {"scheme": "bearer", "secret_ref": "sk_live_51H8xyz"}
        }}));
        assert!(!valid);
        let text = serde_json::to_string(&diags).unwrap();
        assert!(text.contains("secret_ref:not-a-reference"));
        assert!(!text.contains("sk_live_51H8xyz"));
    }

    #[test]
    fn the_old_shape_is_named_with_the_fix() {
        let (valid, codes) = codes(json!({"method": "POST", "path": "/webhook/orders"}));
        assert!(!valid);
        assert!(codes.contains(&"config:pre-contract-shape".to_string()));
    }

    #[test]
    fn get_cannot_fire_when_a_challenge_is_declared() {
        let (valid, codes) = codes(json!({
            "methods": ["POST", "GET"],
            "verify": {"scheme": "none"},
            "challenge": {"scheme": "meta_hub", "verify_token_ref": "threads/verify_token"},
        }));
        assert!(!valid);
        assert!(codes.contains(&"challenge:get-fires".to_string()));
    }

    #[test]
    fn allowed_sources_warns_that_the_runtime_will_not_serve_it_yet() {
        let (_, codes) =
            codes(json!({"verify": {"scheme": "none"}, "allowed_sources": ["10.0.0.0/8"]}));
        assert!(codes.contains(&"allowed_sources:not-enforced".to_string()));
    }

    #[test]
    fn a_bad_idempotency_key_and_trigger_id_are_errors() {
        let (valid, codes) = codes(json!({
            "trigger_id": "Bad Id",
            "verify": {"scheme": "none"},
            "idempotency": {"key": "x-request-id"},
        }));
        assert!(!valid);
        assert!(codes.contains(&"trigger_id:invalid".to_string()));
        assert!(codes.contains(&"idempotency:bad-key".to_string()));
    }
}
