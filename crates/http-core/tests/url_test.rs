use http_core::url::{UrlError, extract_placeholders, validate_url};

#[test]
fn accepts_http_and_https_schemes() {
    validate_url("http://example.com").unwrap();
    validate_url("https://api.example.com/v1").unwrap();
}

#[test]
fn rejects_file_scheme() {
    let err = validate_url("file:///etc/passwd").expect_err("must fail");
    assert!(matches!(err, UrlError::UnsupportedScheme(_)));
}

#[test]
fn rejects_empty_or_relative_url() {
    assert!(validate_url("").is_err());
    assert!(validate_url("/api/users").is_err());
    assert!(validate_url("example.com").is_err());
}

#[test]
fn extract_placeholders_finds_template_vars() {
    let names = extract_placeholders("/users/${user.id}/posts/${post_id}");
    assert_eq!(names, vec!["user.id".to_string(), "post_id".to_string()]);
}

#[test]
fn extract_placeholders_deduplicates() {
    let names = extract_placeholders("${a}/${b}/${a}");
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn extract_placeholders_ignores_broken_syntax() {
    assert!(extract_placeholders("no placeholders here").is_empty());
    assert!(extract_placeholders("${unclosed").is_empty());
    assert!(extract_placeholders("}${").is_empty());
}
