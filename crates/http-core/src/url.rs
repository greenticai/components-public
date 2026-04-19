//! URL validation + `${placeholder}` extraction helpers.

#[derive(Debug, thiserror::Error)]
pub enum UrlError {
    #[error("empty url")]
    Empty,
    #[error("unsupported scheme: {0} (expected http:// or https://)")]
    UnsupportedScheme(String),
}

pub fn validate_url(url: &str) -> Result<(), UrlError> {
    if url.is_empty() {
        return Err(UrlError::Empty);
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        let scheme = url.split("://").next().unwrap_or(url);
        Err(UrlError::UnsupportedScheme(scheme.to_string()))
    }
}

/// Extract placeholder names (between `${` and `}`) in first-occurrence order.
/// Duplicates are removed. Unclosed or malformed placeholders are ignored.
pub fn extract_placeholders(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'$' && bytes[i + 1] == b'{' {
            if let Some(end_rel) = s[i + 2..].find('}') {
                let name = &s[i + 2..i + 2 + end_rel];
                if !name.is_empty() && !out.iter().any(|n| n == name) {
                    out.push(name.to_string());
                }
                i += 2 + end_rel + 1;
                continue;
            } else {
                break;
            }
        }
        i += 1;
    }
    out
}
