//! Pure input checks. A request's text and `context` are third-party data and
//! the `context` echoes through events, so credentials must not ride in it
//! (spec 7.5).

use serde_json::Value;

pub const MAX_CONTEXT_BYTES: usize = 4096;
pub const MAX_TEXT_CHARS: usize = 8000;

const SECRET_KEY_WORDS: &[&str] = &[
    "secret",
    "token",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "authorization",
    "private_key",
];

const SECRET_VALUE_PREFIXES: &[&str] = &["bearer ", "sk-", "gts_", "gti_", "gtm_", "akia", "-----begin"];

/// Why a `context` is refused. Never carries the offending value.
pub fn context_problem(context: &Value) -> Option<String> {
    if !context.is_object() {
        return Some("context must be a JSON object".to_string());
    }
    let size = serde_json::to_vec(context).map(|b| b.len()).unwrap_or(usize::MAX);
    if size > MAX_CONTEXT_BYTES {
        return Some(format!("context is {size} bytes; the limit is {MAX_CONTEXT_BYTES}"));
    }
    credential_path(context, "context")
        .map(|path| format!("{path} looks like a credential; context is echoed in events, do not put secrets in it"))
}

fn credential_path(v: &Value, path: &str) -> Option<String> {
    match v {
        Value::Object(map) => map.iter().find_map(|(k, child)| {
            let lower = k.to_ascii_lowercase();
            let here = format!("{path}.{k}");
            if SECRET_KEY_WORDS.iter().any(|w| lower.contains(w)) {
                return Some(here);
            }
            credential_path(child, &here)
        }),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(i, child)| credential_path(child, &format!("{path}[{i}]"))),
        Value::String(s) => {
            let lower = s.trim_start().to_ascii_lowercase();
            SECRET_VALUE_PREFIXES
                .iter()
                .any(|p| lower.starts_with(p))
                .then(|| path.to_string())
        }
        _ => None,
    }
}

/// Normalise the Core's base URL: HTTPS only, no trailing slash, no query.
pub fn base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let rest = trimmed
        .strip_prefix("https://")
        .ok_or_else(|| "base_url must start with https://".to_string())?;
    if rest.is_empty() || rest.contains(['?', '#', ' ']) || rest.starts_with('/') {
        return Err("base_url must be an https origin, optionally with a path prefix".to_string());
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plain_context_passes() {
        assert!(context_problem(&json!({"order": "A-17", "lines": [1, 2]})).is_none());
    }

    #[test]
    fn credential_keys_and_values_are_refused_without_echoing_them() {
        let p = context_problem(&json!({"auth": {"api_key": "x"}})).unwrap();
        assert!(p.contains("context.auth.api_key"));
        let p = context_problem(&json!({"note": "Bearer abc123"})).unwrap();
        assert!(p.contains("context.note") && !p.contains("abc123"));
    }

    #[test]
    fn oversize_and_non_object_are_refused() {
        let big = json!({"a": "x".repeat(MAX_CONTEXT_BYTES)});
        assert!(context_problem(&big).unwrap().contains("limit"));
        assert!(context_problem(&json!("s")).is_some());
    }

    #[test]
    fn base_url_is_https_only_and_trimmed() {
        assert_eq!(base_url(" https://core.example.com/ ").unwrap(), "https://core.example.com");
        assert_eq!(base_url("https://h/p/").unwrap(), "https://h/p");
        assert!(base_url("http://core.example.com").is_err());
        assert!(base_url("https://").is_err());
        assert!(base_url("https://h/?x=1").is_err());
    }
}
