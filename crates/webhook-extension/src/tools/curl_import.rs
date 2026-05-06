//! `infer_auth_from_curl` — derive inbound webhook auth shape from a curl
//! the *upstream* system would send to our webhook endpoint.
//!
//! Note: webhook is ingress, so the curl represents what an external service
//! sends INTO the runtime. We infer the auth/signature config the runtime
//! needs to validate that request — not the egress auth shape `http-extension`
//! produces from the same input.

use serde_json::{Value, json};

pub fn infer_auth_from_curl(args: &Value) -> Result<String, String> {
    let cmd = args
        .get("curl_cmd")
        .and_then(Value::as_str)
        .ok_or("missing required field: curl_cmd")?;

    let headers = parse_headers(cmd);
    let method = parse_method(cmd);
    let path = parse_path_from_url(cmd);

    let mut auth: Option<Value> = None;
    let mut signature_validation: Option<Value> = None;
    let mut rationale_lines = Vec::<String>::new();

    for (k, v) in &headers {
        let kl = k.to_ascii_lowercase();
        if kl == "authorization" {
            if let Some(_tok) = v.strip_prefix("Bearer ") {
                auth = Some(json!({"type": "bearer", "secret_ref": "WEBHOOK_BEARER"}));
                rationale_lines.push("Authorization: Bearer → bearer auth".into());
            } else if v.starts_with("Basic ") {
                auth = Some(json!({"type": "basic", "secret_ref": "WEBHOOK_BASIC"}));
                rationale_lines.push("Authorization: Basic → basic auth".into());
            } else {
                rationale_lines.push(format!(
                    "Authorization scheme '{v}' not recognised — left auth unset"
                ));
            }
        } else if is_signature_header(&kl) {
            let algo = if kl.contains("sha256") || v.contains("sha256=") {
                "hmac-sha256"
            } else if kl.contains("sha1") || v.contains("sha1=") {
                "hmac-sha1"
            } else {
                "hmac-sha256"
            };
            signature_validation = Some(json!({
                "header": k,
                "algorithm": algo,
                "secret_ref": "WEBHOOK_SIGNING_KEY",
            }));
            // Pair with hmac auth type when no explicit Authorization header is present
            if auth.is_none() {
                auth = Some(json!({"type": "hmac", "secret_ref": "WEBHOOK_SIGNING_KEY"}));
            }
            rationale_lines.push(format!(
                "Signature header '{k}' → signature_validation ({algo})"
            ));
        }
    }

    if auth.is_none() && signature_validation.is_none() {
        rationale_lines
            .push("No auth or signature headers detected — defaulting to auth.type = none".into());
        auth = Some(json!({"type": "none"}));
    }

    let mut suggested = serde_json::Map::new();
    suggested.insert("method".into(), json!(method));
    if let Some(p) = path {
        suggested.insert("path".into(), json!(p));
    }
    if let Some(a) = auth {
        suggested.insert("auth".into(), a);
    }
    if let Some(sv) = signature_validation {
        suggested.insert("signature_validation".into(), sv);
    }

    Ok(json!({
        "suggested_config": Value::Object(suggested),
        "rationale": rationale_lines.join("; "),
    })
    .to_string())
}

fn parse_method(cmd: &str) -> String {
    let lower_chunks: Vec<&str> = cmd.split_ascii_whitespace().collect();
    for window in lower_chunks.windows(2) {
        let flag = window[0];
        if flag == "-X" || flag == "--request" {
            return window[1]
                .trim_matches(|c: char| c == '\'' || c == '"')
                .to_uppercase();
        }
    }
    if cmd.contains("--data") || cmd.contains(" -d ") || cmd.contains("--data-raw") {
        return "POST".into();
    }
    "POST".into()
}

fn parse_path_from_url(cmd: &str) -> Option<String> {
    let url = cmd
        .split_ascii_whitespace()
        .find(|t| t.starts_with("http://") || t.starts_with("https://"))?
        .trim_matches(|c: char| c == '\'' || c == '"');
    let after_scheme = url.split_once("://")?.1;
    let path_start = after_scheme.find('/')?;
    let mut path = &after_scheme[path_start..];
    if let Some(idx) = path.find(['?', '#']) {
        path = &path[..idx];
    }
    Some(path.to_string())
}

fn parse_headers(cmd: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // look for -H or --header
        let rest = &cmd[i..];
        let (flag_len, _) = if rest.starts_with("-H ") {
            (3, true)
        } else if rest.starts_with("--header ") {
            (9, true)
        } else {
            i += 1;
            continue;
        };
        i += flag_len;
        let after = &cmd[i..];
        let (raw, consumed) = read_quoted_or_word(after);
        i += consumed;
        if let Some((k, v)) = raw.split_once(':') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

fn read_quoted_or_word(s: &str) -> (String, usize) {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return (String::new(), 0);
    }
    let first = bytes[0] as char;
    if first == '\'' || first == '"' {
        if let Some(end) = s[1..].find(first) {
            return (s[1..1 + end].to_string(), 1 + end + 1);
        }
        return (s[1..].to_string(), s.len());
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    (s[..end].to_string(), end)
}

fn is_signature_header(kl: &str) -> bool {
    kl.contains("signature") || kl == "x-hub-signature" || kl == "x-hub-signature-256"
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    fn run(curl: &str) -> Value {
        let raw = infer_auth_from_curl(&json!({"curl_cmd": curl})).expect("ok");
        serde_json::from_str(&raw).unwrap()
    }

    #[test]
    fn detects_bearer_auth() {
        let v =
            run(r#"curl -X POST -H 'Authorization: Bearer xyz123' https://example.com/api/hooks"#);
        let cfg = &v["suggested_config"];
        assert_eq!(cfg["auth"]["type"], "bearer");
        assert_eq!(cfg["auth"]["secret_ref"], "WEBHOOK_BEARER");
        assert_eq!(cfg["method"], "POST");
        assert_eq!(cfg["path"], "/api/hooks");
    }

    #[test]
    fn detects_basic_auth() {
        let v = run(r#"curl -H "Authorization: Basic dXNlcjpwYXNz" https://x/intake"#);
        assert_eq!(v["suggested_config"]["auth"]["type"], "basic");
    }

    #[test]
    fn detects_github_signature_header() {
        let v = run(
            r#"curl -X POST -H 'X-Hub-Signature-256: sha256=abc' -H 'Content-Type: application/json' https://example.com/webhook"#,
        );
        let cfg = &v["suggested_config"];
        assert_eq!(cfg["auth"]["type"], "hmac");
        assert_eq!(cfg["signature_validation"]["header"], "X-Hub-Signature-256");
        assert_eq!(cfg["signature_validation"]["algorithm"], "hmac-sha256");
    }

    #[test]
    fn detects_slack_signature_with_sha256() {
        let v = run(r#"curl -H 'X-Slack-Signature: v0=...' https://x/slack"#);
        assert_eq!(
            v["suggested_config"]["signature_validation"]["header"],
            "X-Slack-Signature"
        );
    }

    #[test]
    fn defaults_to_none_when_no_auth_headers() {
        let v = run("curl https://example.com/ping");
        assert_eq!(v["suggested_config"]["auth"]["type"], "none");
        assert_eq!(v["suggested_config"]["method"], "POST");
    }

    #[test]
    fn picks_up_request_flag_method() {
        let v = run("curl --request PUT https://x/webhook");
        assert_eq!(v["suggested_config"]["method"], "PUT");
    }

    #[test]
    fn missing_curl_cmd_errors() {
        let err = infer_auth_from_curl(&json!({})).expect_err("must fail");
        assert!(err.contains("curl_cmd"));
    }

    #[test]
    fn auth_takes_precedence_over_signature_for_auth_block() {
        // If both Authorization: Bearer AND signature header present, auth.type stays bearer
        let v = run(
            r#"curl -H 'Authorization: Bearer T' -H 'X-Hub-Signature-256: sha256=abc' https://x/y"#,
        );
        assert_eq!(v["suggested_config"]["auth"]["type"], "bearer");
        // signature_validation still emitted
        assert!(
            v["suggested_config"]
                .as_object()
                .unwrap()
                .contains_key("signature_validation")
        );
    }
}
