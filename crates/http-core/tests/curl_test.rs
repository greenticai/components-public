use http_core::parse_curl;

#[test]
fn parses_post_with_headers_and_body() {
    let cmd = r#"curl -X POST https://api.example.com/users \
        -H 'Content-Type: application/json' \
        -H 'Authorization: Bearer mytoken' \
        -d '{"name":"alice"}' "#;
    let p = parse_curl(cmd).unwrap();
    assert_eq!(p.method.as_deref(), Some("POST"));
    assert_eq!(p.url.as_deref(), Some("https://api.example.com/users"));
    assert_eq!(
        p.headers.get("Content-Type").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        p.headers.get("Authorization").map(String::as_str),
        Some("Bearer mytoken")
    );
    assert_eq!(p.body.as_deref(), Some(r#"{"name":"alice"}"#));
    assert!(p.unsupported_flags.is_empty());
}

#[test]
fn defaults_to_get_when_no_method_and_no_body() {
    let p = parse_curl("curl https://api.example.com/users").unwrap();
    assert_eq!(p.method.as_deref(), Some("GET"));
}

#[test]
fn defaults_to_post_when_body_present_and_no_method() {
    let p = parse_curl(r#"curl https://api.example.com -d 'x=y'"#).unwrap();
    assert_eq!(p.method.as_deref(), Some("POST"));
}

#[test]
fn reports_unsupported_flags() {
    let p = parse_curl(r#"curl -F 'file=@a.txt' https://api.example.com/upload"#).unwrap();
    assert!(p.unsupported_flags.contains(&"-F".to_string()));
}
