use http_core::{ComponentConfig, apply_answers};
use serde_json::json;

#[test]
fn default_config_has_post_method_and_30s_timeout() {
    let cfg = ComponentConfig::default();
    assert_eq!(cfg.auth_type, "none");
    assert_eq!(cfg.api_key_header, "X-API-Key");
    assert_eq!(cfg.timeout_ms, 30000);
    assert!(cfg.base_url.is_none());
    assert!(cfg.auth_token.is_none());
    assert!(cfg.default_headers.is_none());
}

#[test]
fn apply_answers_merges_base_url_auth_and_timeout() {
    let answers = json!({
        "base_url": "https://api.example.com",
        "auth_type": "bearer",
        "auth_token": "secret:HTTP_TOKEN",
        "timeout_ms": 15000,
        "default_headers": {"X-Team": "qa"}
    });
    let cfg = apply_answers(ComponentConfig::default(), &answers).expect("apply succeeds");
    assert_eq!(cfg.base_url.as_deref(), Some("https://api.example.com"));
    assert_eq!(cfg.auth_type, "bearer");
    assert_eq!(cfg.auth_token.as_deref(), Some("secret:HTTP_TOKEN"));
    assert_eq!(cfg.timeout_ms, 15000);
    assert!(cfg.default_headers.is_some());
}

#[test]
fn apply_answers_rejects_non_positive_timeout() {
    let answers = json!({ "timeout_ms": 0 });
    let err = apply_answers(ComponentConfig::default(), &answers).expect_err("must fail");
    assert!(format!("{err}").contains("timeout_ms"));
}
