use http_core::auth::{AuthType, build_auth_header};
use http_core::config::ComponentConfig;
use serde_json::{Value, json};

use crate::{HttpRequest, http_send, log_event, resolve_secret};

const DEFAULT_METHOD: &str = "POST";

/// Handle blocking HTTP request.
pub fn handle_request(input: &Value) -> Value {
    let cfg = match load_config(input) {
        Ok(cfg) => cfg,
        Err(err) => return json!({"ok": false, "error": err}),
    };

    let request = match build_http_request(&cfg, input) {
        Ok(req) => req,
        Err(err) => return json!({"ok": false, "error": err}),
    };

    match http_send(&request, cfg.timeout_ms) {
        Ok(resp) => {
            let body_str = resp
                .body
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();

            let body_json: Value =
                serde_json::from_str(&body_str).unwrap_or_else(|_| Value::String(body_str.clone()));

            log_event("request_success");

            let headers_map: serde_json::Map<String, Value> = resp
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();

            json!({
                "ok": true,
                "status": resp.status,
                "body": body_json,
                "body_raw": body_str,
                "headers": headers_map,
            })
        }
        Err(err) => {
            json!({
                "ok": false,
                "error": format!("HTTP error: {} ({})", err.message, err.code),
            })
        }
    }
}

/// Build HTTP request from config and input.
pub fn build_http_request(cfg: &ComponentConfig, input: &Value) -> Result<HttpRequest, String> {
    let url = input
        .get("url")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .or_else(|| {
            input
                .get("endpoint")
                .and_then(Value::as_str)
                .map(|s| s.to_string())
        })
        .ok_or("missing url")?;

    let full_url = if url.starts_with("http://") || url.starts_with("https://") {
        url
    } else if let Some(ref base) = cfg.base_url {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            url.trim_start_matches('/')
        )
    } else {
        url
    };

    http_core::url::validate_url(&full_url).map_err(|e| format!("invalid url: {e}"))?;

    let method = input
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_METHOD)
        .to_uppercase();

    let mut headers: Vec<(String, String)> = Vec::new();

    // default_headers is now Option<Value> (JSON object) in http-core
    if let Some(ref dh) = cfg.default_headers
        && let Some(map) = dh.as_object()
    {
        for (k, v) in map {
            if let Some(v_str) = v.as_str() {
                headers.push((k.clone(), v_str.to_string()));
            }
        }
    }

    if let Some(input_headers) = input.get("headers").and_then(Value::as_object) {
        for (k, v) in input_headers {
            if let Some(v_str) = v.as_str() {
                headers.push((k.clone(), v_str.to_string()));
            }
        }
    }

    if let Some(ref token) = cfg.auth_token {
        let resolved_token = resolve_secret(token)?;
        let auth_type = AuthType::from_str(&cfg.auth_type).unwrap_or(AuthType::None);
        if let Some(h) =
            build_auth_header(auth_type, Some(&resolved_token), Some(&cfg.api_key_header))
        {
            headers.push((h.name, h.value));
        }
    }

    let has_content_type = headers
        .iter()
        .any(|(k, _)| k.to_lowercase() == "content-type");
    if !has_content_type && input.get("body").is_some() {
        headers.push(("Content-Type".to_string(), "application/json".to_string()));
    }

    let body = input.get("body").map(|b| {
        if b.is_string() {
            b.as_str().unwrap().as_bytes().to_vec()
        } else {
            serde_json::to_vec(b).unwrap_or_default()
        }
    });

    Ok(HttpRequest {
        method,
        url: full_url,
        headers,
        body,
    })
}

fn load_config(input: &Value) -> Result<ComponentConfig, String> {
    crate::load_config(input)
}
