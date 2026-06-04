//! `curl_to_node` — convert a curl command into a YGTc HTTP node.

use super::runtime_component_ref;
use http_core::{ComponentConfig, NodeBuilder, parse_curl};
use serde_json::{Value, json};

pub fn curl_to_node(args: &Value) -> Result<String, String> {
    let cmd = args
        .get("curl_cmd")
        .and_then(Value::as_str)
        .ok_or("missing required field: curl_cmd")?;
    let node_id = args
        .get("node_id")
        .and_then(Value::as_str)
        .unwrap_or("http_call")
        .to_string();

    let parsed = parse_curl(cmd).map_err(|e| format!("parse error: {e}"))?;
    let url = parsed
        .url
        .clone()
        .ok_or("curl command did not contain a URL")?;

    let (base_url, path) = split_url(&url);

    let mut auth_type = "none";
    let mut auth_token: Option<String> = None;
    let mut remaining_headers = serde_json::Map::new();
    let mut bare_token_rewritten = false;
    for (k, v) in &parsed.headers {
        if k.eq_ignore_ascii_case("Authorization") {
            if let Some(_tok) = v.strip_prefix("Bearer ") {
                auth_type = "bearer";
                auth_token = Some("secret:HTTP_TOKEN".into());
                bare_token_rewritten = true;
            } else if v.starts_with("Basic ") {
                auth_type = "basic";
                auth_token = Some("secret:HTTP_BASIC".into());
                bare_token_rewritten = true;
            }
        } else {
            remaining_headers.insert(k.clone(), json!(v));
        }
    }

    let cfg = ComponentConfig {
        base_url: Some(base_url),
        auth_type: auth_type.into(),
        auth_token,
        default_headers: if remaining_headers.is_empty() {
            None
        } else {
            Some(Value::Object(remaining_headers))
        },
        ..Default::default()
    };

    let method = parsed.method.clone().unwrap_or_else(|| "GET".into());

    let mut builder = NodeBuilder::new(node_id, runtime_component_ref())
        .with_config(cfg)
        .with_input("method", method)
        .with_input("path", path);
    if let Some(body) = parsed.body.as_ref() {
        builder = builder.with_input("body_template", body.clone());
    }
    let node = builder
        .with_rationale(format!(
            "Imported from curl. {} header(s), {} unsupported flag(s).",
            parsed.headers.len(),
            parsed.unsupported_flags.len()
        ))
        .build();

    let mut out = serde_json::to_value(&node).map_err(|e| e.to_string())?;

    // Attach diagnostics array for unsupported flags + bare-token note
    let mut diags = Vec::new();
    if bare_token_rewritten {
        diags.push(json!({
            "severity": "warning",
            "code": "auth:bare-token-rewritten",
            "message": "raw bearer/basic token replaced with secret: reference — create this secret in the Operator"
        }));
    }
    for flag in &parsed.unsupported_flags {
        diags.push(json!({
            "severity": "info",
            "code": "curl:unsupported-flag",
            "message": format!("flag {flag} not mapped to HTTP node config")
        }));
    }
    if !diags.is_empty() {
        out["diagnostics"] = Value::Array(diags);
    }

    serde_json::to_string(&out).map_err(|e| e.to_string())
}

fn split_url(url: &str) -> (String, String) {
    if let Some(scheme_end) = url.find("://")
        && let Some(path_start) = url[scheme_end + 3..].find('/')
    {
        let base = url[..scheme_end + 3 + path_start].to_string();
        let path = url[scheme_end + 3 + path_start..].to_string();
        return (base, path);
    }
    (url.to_string(), "/".to_string())
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn converts_bearer_curl_into_node() {
        let args = json!({
            "curl_cmd": "curl -X POST https://api.example.com/users -H 'Authorization: Bearer xxx' -H 'X-Team: qa' -d '{\"name\":\"alice\"}'",
            "node_id": "create_user"
        });
        let out = curl_to_node(&args).expect("ok");
        let j: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(j["node_id"], "create_user");
        assert_eq!(j["config"]["auth_type"], "bearer");
        assert_eq!(j["config"]["auth_token"], "secret:HTTP_TOKEN");
        assert_eq!(j["config"]["base_url"], "https://api.example.com");
        assert_eq!(j["inputs"]["method"], "POST");
        assert_eq!(j["inputs"]["path"], "/users");
        assert_eq!(j["config"]["default_headers"]["X-Team"], "qa");
    }

    #[test]
    fn notes_unsupported_flags() {
        let args = json!({ "curl_cmd": "curl -F 'f=@a.txt' https://api.example.com/upload", "node_id": "up" });
        let out = curl_to_node(&args).expect("ok");
        let j: Value = serde_json::from_str(&out).unwrap();
        let note = j["diagnostics"].as_array().unwrap();
        assert!(note.iter().any(|d| d["code"] == "curl:unsupported-flag"));
    }
}
