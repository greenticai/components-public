//! HTTP types, parse helpers, and platform-specific (WASM vs native) implementations.

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::http_client_v1_1 as client;
#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::secrets_store;
#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::telemetry_logger as logger_api;

use serde_json::Value;

// ============================================================================
// HTTP types shared between WASM and native
// ============================================================================

/// HTTP request structure for testing
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// HTTP response structure for testing
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// HTTP error structure
#[derive(Debug, Clone)]
pub struct HttpError {
    pub code: String,
    pub message: String,
}

// ============================================================================
// Parse helpers (SSE / NDJSON)
// ============================================================================

/// Parse Server-Sent Events (SSE) format
pub fn parse_sse_events(body: &str) -> Vec<Value> {
    let mut events = Vec::new();
    let mut current_event = serde_json::Map::new();

    for line in body.lines() {
        if line.is_empty() {
            if !current_event.is_empty() {
                events.push(Value::Object(current_event.clone()));
                current_event.clear();
            }
        } else if let Some(data) = line.strip_prefix("data: ") {
            let existing = current_event
                .get("data")
                .and_then(Value::as_str)
                .unwrap_or("");
            let new_data = if existing.is_empty() {
                data.to_string()
            } else {
                format!("{}\n{}", existing, data)
            };
            current_event.insert("data".to_string(), Value::String(new_data));
        } else if let Some(event_type) = line.strip_prefix("event: ") {
            current_event.insert("event".to_string(), Value::String(event_type.to_string()));
        } else if let Some(id) = line.strip_prefix("id: ") {
            current_event.insert("id".to_string(), Value::String(id.to_string()));
        }
    }

    if !current_event.is_empty() {
        events.push(Value::Object(current_event));
    }

    events
}

/// Parse newline-delimited JSON (NDJSON)
pub fn parse_ndjson(body: &str) -> Vec<Value> {
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            serde_json::from_str::<Value>(line).ok().map(|v| {
                let mut event = serde_json::Map::new();
                event.insert("data".to_string(), v);
                Value::Object(event)
            })
        })
        .collect()
}

/// Simple base64 encoding
pub fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::new();
    let mut i = 0;

    while i < data.len() {
        let b0 = data[i] as usize;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as usize
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as usize
        } else {
            0
        };

        result.push(ALPHABET[b0 >> 2] as char);
        result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

        if i + 1 < data.len() {
            result.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        } else {
            result.push('=');
        }

        if i + 2 < data.len() {
            result.push(ALPHABET[b2 & 0x3f] as char);
        } else {
            result.push('=');
        }

        i += 3;
    }

    result
}

// ============================================================================
// Platform-specific implementations
// ============================================================================

#[cfg(target_arch = "wasm32")]
pub fn log_event(event: &str) {
    let span = logger_api::SpanContext {
        tenant: "tenant".into(),
        session_id: None,
        flow_id: "http-component".into(),
        node_id: None,
        provider: "http".into(),
        start_ms: None,
        end_ms: None,
    };
    let fields = [("event".to_string(), event.to_string())];
    let _ = logger_api::log(&span, &fields, None);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn log_event(_event: &str) {
    // No-op for native builds
}

#[cfg(target_arch = "wasm32")]
pub fn resolve_secret(token: &str) -> Result<String, String> {
    if let Some(secret_name) = token.strip_prefix("secret:") {
        match secrets_store::get(secret_name) {
            Ok(Some(bytes)) => {
                String::from_utf8(bytes).map_err(|_| "secret not valid utf-8".to_string())
            }
            Ok(None) => Err(format!("secret not found: {}", secret_name)),
            Err(_) => Err(format!("failed to get secret: {}", secret_name)),
        }
    } else {
        Ok(token.to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn resolve_secret(token: &str) -> Result<String, String> {
    if let Some(secret_name) = token.strip_prefix("secret:") {
        // For testing, check environment variable
        std::env::var(secret_name).map_err(|_| format!("secret not found: {}", secret_name))
    } else {
        Ok(token.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
pub fn http_send(req: &HttpRequest, timeout_ms: u32) -> Result<HttpResponse, HttpError> {
    let wasm_req = client::Request {
        method: req.method.clone(),
        url: req.url.clone(),
        headers: req.headers.clone(),
        body: req.body.clone(),
    };

    let options = client::RequestOptions {
        timeout_ms: Some(timeout_ms),
        allow_insecure: Some(false),
        follow_redirects: Some(true),
    };

    match client::send(&wasm_req, Some(options), None) {
        Ok(resp) => Ok(HttpResponse {
            status: resp.status,
            headers: resp.headers,
            body: resp.body,
        }),
        Err(err) => Err(HttpError {
            code: err.code,
            message: err.message,
        }),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn http_send(_req: &HttpRequest, _timeout_ms: u32) -> Result<HttpResponse, HttpError> {
    // Stub for native builds - tests should mock this
    Err(HttpError {
        code: "not_implemented".to_string(),
        message: "HTTP not available in native builds".to_string(),
    })
}
