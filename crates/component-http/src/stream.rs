use serde_json::{Value, json};

use crate::request::build_http_request;
use crate::{http_send, log_event, parse_ndjson, parse_sse_events};

/// Handle streaming HTTP request (for SSE/chunked responses).
pub fn handle_stream(input: &Value) -> Value {
    let cfg = match load_config(input) {
        Ok(cfg) => cfg,
        Err(err) => return json!({"ok": false, "error": err}),
    };

    let mut request = match build_http_request(&cfg, input) {
        Ok(req) => req,
        Err(err) => return json!({"ok": false, "error": err}),
    };

    let has_accept = request
        .headers
        .iter()
        .any(|(k, _)| k.to_lowercase() == "accept");
    if !has_accept {
        request
            .headers
            .push(("Accept".to_string(), "text/event-stream".to_string()));
    }

    match http_send(&request, cfg.timeout_ms) {
        Ok(resp) => {
            let body_str = resp
                .body
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();

            let is_sse = resp.headers.iter().any(|(k, v)| {
                k.to_lowercase() == "content-type" && v.contains("text/event-stream")
            });

            let events = if is_sse {
                parse_sse_events(&body_str)
            } else {
                parse_ndjson(&body_str)
            };

            let full_content = events
                .iter()
                .filter_map(|e| e.get("data").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");

            log_event("stream_success");

            json!({
                "ok": true,
                "status": resp.status,
                "events": events,
                "content": full_content,
                "body_raw": body_str,
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

fn load_config(input: &Value) -> Result<http_core::config::ComponentConfig, String> {
    crate::load_config(input)
}
