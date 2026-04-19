use http_core::auth::{AuthType, build_auth_header};

#[test]
fn none_returns_no_header() {
    assert!(build_auth_header(AuthType::None, None, None).is_none());
}

#[test]
fn bearer_builds_authorization_header() {
    let h = build_auth_header(AuthType::Bearer, Some("abc123"), None).expect("header");
    assert_eq!(h.name, "Authorization");
    assert_eq!(h.value, "Bearer abc123");
}

#[test]
fn bearer_missing_token_returns_none() {
    assert!(build_auth_header(AuthType::Bearer, None, None).is_none());
}

#[test]
fn api_key_uses_custom_header_name() {
    let h = build_auth_header(AuthType::ApiKey, Some("k"), Some("X-My-Key")).expect("header");
    assert_eq!(h.name, "X-My-Key");
    assert_eq!(h.value, "k");
}

#[test]
fn basic_base64_encodes_user_pass() {
    let h = build_auth_header(AuthType::Basic, Some("alice:s3cret"), None).expect("header");
    assert_eq!(h.name, "Authorization");
    assert_eq!(h.value, "Basic YWxpY2U6czNjcmV0");
}

#[test]
fn auth_type_parses_string() {
    assert_eq!(AuthType::from_str("bearer"), Some(AuthType::Bearer));
    assert_eq!(AuthType::from_str("api_key"), Some(AuthType::ApiKey));
    assert_eq!(AuthType::from_str("nope"), None);
}
