#![allow(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]
#![allow(clippy::collapsible_if)]

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
mod bindings {
    wit_bindgen::generate!({ path: "wit/http", world: "component-v0-v6-v0", generate_all });
}

#[cfg(target_arch = "wasm32")]
use bindings::greentic::component::http_client as client;
#[cfg(target_arch = "wasm32")]
use bindings::greentic::component::logger_api;
#[cfg(target_arch = "wasm32")]
use bindings::greentic::component::secrets_store;

const COMPONENT_ID: &str = "http";
const WORLD_ID: &str = "component-v0-v6-v0";
const DEFAULT_TIMEOUT_MS: u32 = 30000;
const DEFAULT_METHOD: &str = "POST";

const I18N_KEYS: &[&str] = &[
    "http.op.request.title",
    "http.op.request.description",
    "http.op.stream.title",
    "http.op.stream.description",
    "http.schema.input.title",
    "http.schema.input.description",
    "http.schema.input.url.title",
    "http.schema.input.url.description",
    "http.schema.input.method.title",
    "http.schema.input.method.description",
    "http.schema.input.body.title",
    "http.schema.input.body.description",
    "http.schema.input.headers.title",
    "http.schema.input.headers.description",
    "http.schema.output.title",
    "http.schema.output.description",
    "http.schema.output.status.title",
    "http.schema.output.status.description",
    "http.schema.output.body.title",
    "http.schema.output.body.description",
    "http.schema.config.title",
    "http.schema.config.description",
    "http.schema.config.base_url.title",
    "http.schema.config.base_url.description",
    "http.schema.config.auth_type.title",
    "http.schema.config.auth_type.description",
    "http.schema.config.auth_token.title",
    "http.schema.config.auth_token.description",
    "http.schema.config.timeout_ms.title",
    "http.schema.config.timeout_ms.description",
    "http.qa.default.title",
    "http.qa.setup.title",
    "http.qa.setup.base_url",
    "http.qa.setup.auth_type",
    "http.qa.setup.auth_token",
    "http.qa.setup.timeout_ms",
    "http.qa.setup.default_headers",
];

/// Component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default = "default_api_key_header")]
    pub api_key_header: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u32,
    #[serde(default)]
    pub default_headers: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApplyAnswersResult {
    ok: bool,
    config: Option<ComponentConfig>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct I18nText {
    key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SchemaField {
    required: bool,
    schema: SchemaIr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SchemaIr {
    String {
        title: I18nText,
        description: I18nText,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        secret: bool,
    },
    Number {
        title: I18nText,
        description: I18nText,
    },
    Bool {
        title: I18nText,
        description: I18nText,
    },
    Array {
        title: I18nText,
        description: I18nText,
        items: Box<SchemaIr>,
    },
    Object {
        title: I18nText,
        description: I18nText,
        fields: BTreeMap<String, SchemaField>,
        additional_properties: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OperationDescriptor {
    name: String,
    title: I18nText,
    description: I18nText,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RedactionRule {
    path: String,
    strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DescribePayload {
    provider: String,
    world: String,
    operations: Vec<OperationDescriptor>,
    input_schema: SchemaIr,
    output_schema: SchemaIr,
    config_schema: SchemaIr,
    redactions: Vec<RedactionRule>,
    schema_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QaQuestionSpec {
    id: String,
    label: I18nText,
    help: Option<I18nText>,
    error: Option<I18nText>,
    kind: String,
    required: bool,
    default: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QaSpec {
    mode: String,
    title: I18nText,
    description: Option<I18nText>,
    questions: Vec<QaQuestionSpec>,
    defaults: Value,
}

// ============================================================================
// WASM Component Implementation
// ============================================================================

#[cfg(target_arch = "wasm32")]
struct Component;

#[cfg(target_arch = "wasm32")]
impl bindings::exports::greentic::component::descriptor::Guest for Component {
    fn describe() -> Vec<u8> {
        canonical_cbor_bytes(&build_describe_payload())
    }
}

#[cfg(target_arch = "wasm32")]
impl bindings::exports::greentic::component::runtime::Guest for Component {
    fn invoke(op: String, input_cbor: Vec<u8>) -> Vec<u8> {
        let input: Value = match decode_cbor(&input_cbor) {
            Ok(value) => value,
            Err(err) => {
                return canonical_cbor_bytes(
                    &json!({"ok": false, "error": format!("invalid input cbor: {err}")}),
                );
            }
        };

        let output = match op.as_str() {
            "request" => handle_request(&input),
            "stream" => handle_stream(&input),
            other => json!({"ok": false, "error": format!("unsupported op: {other}")}),
        };

        canonical_cbor_bytes(&output)
    }
}

#[cfg(target_arch = "wasm32")]
impl bindings::exports::greentic::component::qa::Guest for Component {
    fn qa_spec(mode: bindings::exports::greentic::component::qa::Mode) -> Vec<u8> {
        canonical_cbor_bytes(&build_qa_spec_wasm(mode))
    }

    fn apply_answers(
        mode: bindings::exports::greentic::component::qa::Mode,
        answers_cbor: Vec<u8>,
    ) -> Vec<u8> {
        let answers: Value = match decode_cbor(&answers_cbor) {
            Ok(value) => value,
            Err(err) => {
                return canonical_cbor_bytes(&ApplyAnswersResult {
                    ok: false,
                    config: None,
                    error: Some(format!("invalid answers cbor: {err}")),
                });
            }
        };

        if mode == bindings::exports::greentic::component::qa::Mode::Setup {
            let get_str = |key: &str| -> Option<String> {
                answers
                    .get(key)
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            };

            let cfg = ComponentConfig {
                base_url: get_str("base_url"),
                auth_type: get_str("auth_type").unwrap_or_else(default_auth_type),
                auth_token: get_str("auth_token"),
                api_key_header: get_str("api_key_header").unwrap_or_else(default_api_key_header),
                timeout_ms: answers
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .map(|v| v as u32)
                    .unwrap_or(DEFAULT_TIMEOUT_MS),
                default_headers: get_str("default_headers"),
            };

            return canonical_cbor_bytes(&ApplyAnswersResult {
                ok: true,
                config: Some(cfg),
                error: None,
            });
        }

        canonical_cbor_bytes(&ApplyAnswersResult {
            ok: true,
            config: None,
            error: None,
        })
    }
}

#[cfg(target_arch = "wasm32")]
impl bindings::exports::greentic::component::component_i18n::Guest for Component {
    fn i18n_keys() -> Vec<String> {
        I18N_KEYS.iter().map(|k| (*k).to_string()).collect()
    }

    fn i18n_bundle(locale: String) -> Vec<u8> {
        let locale = if locale.trim().is_empty() {
            "en".to_string()
        } else {
            locale
        };
        let mut messages = serde_json::Map::new();
        for key in I18N_KEYS {
            messages.insert((*key).to_string(), Value::String((*key).to_string()));
        }
        canonical_cbor_bytes(&json!({"locale": locale, "messages": Value::Object(messages)}))
    }
}

#[cfg(target_arch = "wasm32")]
bindings::export!(Component with_types_in bindings);

// ============================================================================
// Core HTTP Logic (shared between WASM and native)
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

/// Handle blocking HTTP request
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

/// Handle streaming HTTP request (for SSE/chunked responses)
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

/// Build HTTP request from config and input
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

    let method = input
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_METHOD)
        .to_uppercase();

    let mut headers: Vec<(String, String)> = Vec::new();

    if let Some(ref default_headers_json) = cfg.default_headers {
        if let Ok(default_headers) =
            serde_json::from_str::<serde_json::Map<String, Value>>(default_headers_json)
        {
            for (k, v) in default_headers {
                if let Some(v_str) = v.as_str() {
                    headers.push((k, v_str.to_string()));
                }
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

        match cfg.auth_type.as_str() {
            "bearer" => {
                headers.push((
                    "Authorization".to_string(),
                    format!("Bearer {}", resolved_token),
                ));
            }
            "api_key" => {
                headers.push((cfg.api_key_header.clone(), resolved_token));
            }
            "basic" => {
                let encoded = base64_encode(resolved_token.as_bytes());
                headers.push(("Authorization".to_string(), format!("Basic {}", encoded)));
            }
            _ => {}
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
// Helper Functions
// ============================================================================

fn build_describe_payload() -> DescribePayload {
    let input_schema = input_schema();
    let output_schema = output_schema();
    let config_schema = config_schema();

    DescribePayload {
        provider: COMPONENT_ID.to_string(),
        world: WORLD_ID.to_string(),
        operations: vec![
            op(
                "request",
                "http.op.request.title",
                "http.op.request.description",
            ),
            op(
                "stream",
                "http.op.stream.title",
                "http.op.stream.description",
            ),
        ],
        input_schema: input_schema.clone(),
        output_schema: output_schema.clone(),
        config_schema: config_schema.clone(),
        redactions: vec![RedactionRule {
            path: "$.auth_token".to_string(),
            strategy: "replace".to_string(),
        }],
        schema_hash: "http-schema-v1".to_string(),
    }
}

#[cfg(target_arch = "wasm32")]
fn build_qa_spec_wasm(mode: bindings::exports::greentic::component::qa::Mode) -> QaSpec {
    use bindings::exports::greentic::component::qa::Mode;

    match mode {
        Mode::Default => QaSpec {
            mode: "default".to_string(),
            title: i18n("http.qa.default.title"),
            description: None,
            questions: Vec::new(),
            defaults: json!({}),
        },
        Mode::Setup => QaSpec {
            mode: "setup".to_string(),
            title: i18n("http.qa.setup.title"),
            description: None,
            questions: vec![
                qa_q("base_url", "http.qa.setup.base_url", false),
                qa_q("auth_type", "http.qa.setup.auth_type", false),
                qa_q("auth_token", "http.qa.setup.auth_token", false),
                qa_q("timeout_ms", "http.qa.setup.timeout_ms", false),
                qa_q("default_headers", "http.qa.setup.default_headers", false),
            ],
            defaults: json!({
                "auth_type": "bearer",
                "timeout_ms": DEFAULT_TIMEOUT_MS,
            }),
        },
        Mode::Upgrade | Mode::Remove => QaSpec {
            mode: if mode == Mode::Upgrade {
                "upgrade"
            } else {
                "remove"
            }
            .to_string(),
            title: i18n("http.qa.default.title"),
            description: None,
            questions: Vec::new(),
            defaults: json!({}),
        },
    }
}

fn input_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "url".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::String {
                title: i18n("http.schema.input.url.title"),
                description: i18n("http.schema.input.url.description"),
                format: Some("uri".to_string()),
                secret: false,
            },
        },
    );
    fields.insert(
        "method".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("http.schema.input.method.title"),
                description: i18n("http.schema.input.method.description"),
                format: None,
                secret: false,
            },
        },
    );
    fields.insert(
        "body".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("http.schema.input.body.title"),
                description: i18n("http.schema.input.body.description"),
                format: None,
                secret: false,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("http.schema.input.title"),
        description: i18n("http.schema.input.description"),
        fields,
        additional_properties: true,
    }
}

fn output_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "status".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Number {
                title: i18n("http.schema.output.status.title"),
                description: i18n("http.schema.output.status.description"),
            },
        },
    );
    fields.insert(
        "body".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::String {
                title: i18n("http.schema.output.body.title"),
                description: i18n("http.schema.output.body.description"),
                format: None,
                secret: false,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("http.schema.output.title"),
        description: i18n("http.schema.output.description"),
        fields,
        additional_properties: true,
    }
}

fn config_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "base_url".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("http.schema.config.base_url.title"),
                description: i18n("http.schema.config.base_url.description"),
                format: Some("uri".to_string()),
                secret: false,
            },
        },
    );
    fields.insert(
        "auth_type".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("http.schema.config.auth_type.title"),
                description: i18n("http.schema.config.auth_type.description"),
                format: None,
                secret: false,
            },
        },
    );
    fields.insert(
        "auth_token".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("http.schema.config.auth_token.title"),
                description: i18n("http.schema.config.auth_token.description"),
                format: None,
                secret: true,
            },
        },
    );
    fields.insert(
        "timeout_ms".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::Number {
                title: i18n("http.schema.config.timeout_ms.title"),
                description: i18n("http.schema.config.timeout_ms.description"),
            },
        },
    );

    SchemaIr::Object {
        title: i18n("http.schema.config.title"),
        description: i18n("http.schema.config.description"),
        fields,
        additional_properties: false,
    }
}

fn op(name: &str, title: &str, description: &str) -> OperationDescriptor {
    OperationDescriptor {
        name: name.to_string(),
        title: i18n(title),
        description: i18n(description),
    }
}

fn qa_q(key: &str, text: &str, required: bool) -> QaQuestionSpec {
    QaQuestionSpec {
        id: key.to_string(),
        label: i18n(text),
        help: None,
        error: None,
        kind: "text".to_string(),
        required,
        default: None,
    }
}

fn i18n(key: &str) -> I18nText {
    I18nText {
        key: key.to_string(),
    }
}

fn load_config(input: &Value) -> Result<ComponentConfig, String> {
    let candidate = input
        .get("config")
        .cloned()
        .unwrap_or_else(|| input.clone());

    serde_json::from_value(candidate).map_err(|err| format!("invalid config: {err}"))
}

fn default_auth_type() -> String {
    "none".to_string()
}

fn default_api_key_header() -> String {
    "X-API-Key".to_string()
}

fn default_timeout() -> u32 {
    DEFAULT_TIMEOUT_MS
}

fn canonical_cbor_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

fn decode_cbor(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

// ============================================================================
// Platform-specific implementations
// ============================================================================

#[cfg(target_arch = "wasm32")]
fn log_event(event: &str) {
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
fn log_event(_event: &str) {
    // No-op for native builds
}

#[cfg(target_arch = "wasm32")]
fn resolve_secret(token: &str) -> Result<String, String> {
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
fn resolve_secret(token: &str) -> Result<String, String> {
    if let Some(secret_name) = token.strip_prefix("secret:") {
        // For testing, check environment variable
        std::env::var(secret_name).map_err(|_| format!("secret not found: {}", secret_name))
    } else {
        Ok(token.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
fn http_send(req: &HttpRequest, timeout_ms: u32) -> Result<HttpResponse, HttpError> {
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
fn http_send(_req: &HttpRequest, _timeout_ms: u32) -> Result<HttpResponse, HttpError> {
    // Stub for native builds - tests should mock this
    Err(HttpError {
        code: "not_implemented".to_string(),
        message: "HTTP not available in native builds".to_string(),
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_events() {
        let body = "event: message\ndata: hello\n\nevent: done\ndata: world\n\n";
        let events = parse_sse_events(body);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event"], "message");
        assert_eq!(events[0]["data"], "hello");
        assert_eq!(events[1]["event"], "done");
        assert_eq!(events[1]["data"], "world");
    }

    #[test]
    fn test_parse_ndjson() {
        let body = "{\"text\": \"hello\"}\n{\"text\": \"world\"}\n";
        let events = parse_ndjson(body);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
    }

    #[test]
    fn test_handle_request_missing_url() {
        let input = json!({});
        let output = handle_request(&input);
        assert_eq!(output["ok"], Value::Bool(false));
        assert!(output["error"].as_str().unwrap().contains("missing url"));
    }

    #[test]
    fn test_build_http_request_with_base_url() {
        let cfg = ComponentConfig {
            base_url: Some("https://api.example.com".to_string()),
            auth_type: "none".to_string(),
            auth_token: None,
            api_key_header: "X-API-Key".to_string(),
            timeout_ms: 30000,
            default_headers: None,
        };
        let input = json!({"url": "/v1/test"});
        let req = build_http_request(&cfg, &input).unwrap();
        assert_eq!(req.url, "https://api.example.com/v1/test");
    }

    #[test]
    fn test_build_http_request_with_bearer_auth() {
        let cfg = ComponentConfig {
            base_url: None,
            auth_type: "bearer".to_string(),
            auth_token: Some("test-token".to_string()),
            api_key_header: "X-API-Key".to_string(),
            timeout_ms: 30000,
            default_headers: None,
        };
        let input = json!({"url": "https://api.example.com/test"});
        let req = build_http_request(&cfg, &input).unwrap();
        let auth_header = req.headers.iter().find(|(k, _)| k == "Authorization");
        assert!(auth_header.is_some());
        assert_eq!(auth_header.unwrap().1, "Bearer test-token");
    }
}
