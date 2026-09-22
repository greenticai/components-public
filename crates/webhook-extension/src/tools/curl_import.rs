//! `infer_auth_from_curl` — derive a webhook trigger's `verify` block from a
//! curl the *upstream* system would send to our trigger endpoint.
//!
//! Webhook is ingress: the curl is what an external service sends INTO the
//! runtime, and the output is the trigger contract v1 `verify` block the
//! runtime needs to authenticate it (greentic-designer
//! `docs/trigger-contract-v1.md` §6.3.3). Secret references are NAMES
//! (`webhook/signing_key`), never values — the token in the sample curl is
//! deliberately not copied anywhere.

use serde_json::{Value, json};

pub fn infer_auth_from_curl(args: &Value) -> Result<String, String> {
    let cmd = args
        .get("curl_cmd")
        .and_then(Value::as_str)
        .ok_or("missing required field: curl_cmd")?;

    let headers = parse_headers(cmd);
    let method = parse_method(cmd);

    let mut verify: Option<Value> = None;
    let mut rationale_lines = Vec::<String>::new();

    for (k, v) in &headers {
        let kl = k.to_ascii_lowercase();
        if kl == "authorization" && verify.is_none() {
            if v.starts_with("Bearer ") {
                verify = Some(json!({
                    "scheme": "bearer",
                    "header": "Authorization",
                    "secret_ref": "webhook/bearer_token",
                }));
                rationale_lines.push("Authorization: Bearer → verify.scheme = bearer".into());
            } else {
                rationale_lines.push(format!(
                    "Authorization scheme '{}' is not verifiable by the runtime (bearer only)",
                    v.split_whitespace().next().unwrap_or("")
                ));
            }
        } else if is_signature_header(&kl) {
            let (scheme, prefix) = if kl.contains("sha1") || v.starts_with("sha1=") {
                ("hmac-sha1", "sha1=")
            } else {
                ("hmac-sha256", "sha256=")
            };
            if kl == "x-slack-signature" || kl == "stripe-signature" {
                rationale_lines.push(format!(
                    "'{k}' signs a timestamp with the body; trigger contract v1 has no \
                     timestamped scheme yet, so it cannot be verified as-is"
                ));
                continue;
            }
            let prefix = if v.starts_with(prefix) { prefix } else { "" };
            verify = Some(json!({
                "scheme": scheme,
                "header": k,
                "prefix": prefix,
                "encoding": "hex",
                "secret_ref": "webhook/signing_key",
            }));
            rationale_lines.push(format!("Signature header '{k}' → verify.scheme = {scheme}"));
        }
    }

    let verify = verify.unwrap_or_else(|| {
        rationale_lines.push(
            "No verifiable auth header detected — verify.scheme = none: anyone who knows the \
             URL can start the flow"
                .into(),
        );
        json!({ "scheme": "none" })
    });

    let methods = if method == "GET" {
        json!(["POST"])
    } else {
        json!([method])
    };
    Ok(json!({
        "suggested_config": {
            "methods": methods,
            "verify": verify,
        },
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
    fn a_bearer_header_becomes_bearer_verification_without_copying_the_token() {
        let out = run(r#"curl -X POST https://x/hook -H "Authorization: Bearer s3cr3t-token""#);
        let verify = &out["suggested_config"]["verify"];
        assert_eq!(verify["scheme"], "bearer");
        assert_eq!(verify["secret_ref"], "webhook/bearer_token");
        assert!(!out.to_string().contains("s3cr3t-token"));
    }

    #[test]
    fn a_github_or_meta_signature_becomes_hmac_sha256_with_its_prefix() {
        let out = run(r#"curl https://x/hook -H "X-Hub-Signature-256: sha256=abc" -d '{}'"#);
        let verify = &out["suggested_config"]["verify"];
        assert_eq!(verify["scheme"], "hmac-sha256");
        assert_eq!(verify["header"], "X-Hub-Signature-256");
        assert_eq!(verify["prefix"], "sha256=");
    }

    #[test]
    fn a_sha1_signature_becomes_hmac_sha1() {
        let out = run(r#"curl https://x/hook -H "X-Hub-Signature: sha1=abc" -d '{}'"#);
        assert_eq!(out["suggested_config"]["verify"]["scheme"], "hmac-sha1");
    }

    #[test]
    fn a_timestamped_signature_is_named_as_unsupported_not_guessed() {
        let out = run(r#"curl https://x/hook -H "X-Slack-Signature: v0=abc" -d '{}'"#);
        assert_eq!(out["suggested_config"]["verify"]["scheme"], "none");
        assert!(out["rationale"].as_str().unwrap().contains("timestamp"));
    }

    #[test]
    fn no_auth_headers_suggests_none_and_says_what_that_means() {
        let out = run("curl -X POST https://x/hook -d '{}'");
        assert_eq!(out["suggested_config"]["verify"]["scheme"], "none");
        assert!(out["rationale"].as_str().unwrap().contains("anyone"));
    }

    #[test]
    fn picks_up_request_flag_method() {
        let out = run("curl -X PUT https://x/hook");
        assert_eq!(out["suggested_config"]["methods"], json!(["PUT"]));
    }

    #[test]
    fn missing_curl_cmd_errors() {
        let err = infer_auth_from_curl(&json!({})).expect_err("must fail");
        assert!(err.contains("curl_cmd"));
    }
}
