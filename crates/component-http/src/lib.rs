#![allow(unsafe_op_in_unsafe_fn)]

use greentic_types::cbor::canonical;
use http_core::config::ComponentConfig;
use serde::Serialize;
use serde_json::Value;

pub use describe::{SchemaIr, config_schema, input_schema, output_schema};
pub use http::{
    HttpError, HttpRequest, HttpResponse, base64_encode, http_send, log_event, parse_ndjson,
    parse_sse_events, resolve_secret,
};
pub use qa::canonical_qa_spec;
pub use request::{build_http_request, handle_request};
pub use stream::handle_stream;

pub(crate) fn load_config(input: &Value) -> Result<http_core::config::ComponentConfig, String> {
    let candidate = input
        .get("config")
        .cloned()
        .unwrap_or_else(|| input.clone());

    serde_json::from_value(candidate).map_err(|err| format!("invalid config: {err}"))
}

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;

mod describe;
mod http;
mod qa;
mod request;
mod stream;

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_ID: &str = "http";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const WORLD_ID: &str = "component-v0-v6-v0";
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
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
    "http.qa.default.description",
    "http.qa.setup.title",
    "http.qa.setup.description",
    "http.qa.update.title",
    "http.qa.update.description",
    "http.qa.remove.title",
    "http.qa.remove.description",
    "http.qa.setup.base_url",
    "http.qa.setup.auth_type",
    "http.qa.setup.auth_token",
    "http.qa.setup.timeout_ms",
    "http.qa.setup.default_headers",
];

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ApplyAnswersResult {
    ok: bool,
    config: Option<ComponentConfig>,
    error: Option<String>,
}

// ============================================================================
// WASM Component Implementation
// ============================================================================

#[cfg(target_arch = "wasm32")]
#[used]
#[unsafe(link_section = ".greentic.wasi")]
static WASI_TARGET_MARKER: [u8; 13] = *b"wasm32-wasip2";

#[cfg(target_arch = "wasm32")]
mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "component-v0-v6-v0",
    });
}

#[cfg(target_arch = "wasm32")]
use bindings::exports::greentic::component::{
    component_descriptor, component_i18n,
    component_qa::{self, QaMode},
    component_runtime, component_schema,
};

#[cfg(target_arch = "wasm32")]
use greentic_types::i18n_text::I18nText as CanonicalI18nText;
#[cfg(target_arch = "wasm32")]
use greentic_types::schemas::common::schema_ir::{
    AdditionalProperties as CanonicalAdditionalProperties, SchemaIr as CanonicalSchemaIr,
};
#[cfg(target_arch = "wasm32")]
use greentic_types::schemas::component::v0_6_0::{
    ComponentDescribe, ComponentInfo, ComponentOperation, ComponentRunInput, ComponentRunOutput,
    schema_hash,
};
#[cfg(target_arch = "wasm32")]
use std::collections::BTreeMap;

#[cfg(target_arch = "wasm32")]
struct Component;

#[cfg(target_arch = "wasm32")]
struct NodeCompat;

#[cfg(target_arch = "wasm32")]
impl node::Guest for NodeCompat {
    fn describe() -> node::ComponentDescriptor {
        node::ComponentDescriptor {
            name: COMPONENT_ID.to_string(),
            version: COMPONENT_VERSION.to_string(),
            summary: Some("HTTP client component with blocking and streaming support".to_string()),
            capabilities: vec![
                "host:http".to_string(),
                "host:secrets".to_string(),
                "host:telemetry".to_string(),
            ],
            ops: vec![
                node::Op {
                    name: "request".to_string(),
                    summary: Some("Send a blocking HTTP request".to_string()),
                    input: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(
                            &input_schema(),
                        )),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    output: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(
                            &output_schema(),
                        )),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    examples: Vec::new(),
                },
                node::Op {
                    name: "stream".to_string(),
                    summary: Some("Send an HTTP request and parse streamed responses".to_string()),
                    input: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(
                            &input_schema(),
                        )),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    output: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(
                            &output_schema(),
                        )),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    examples: Vec::new(),
                },
            ],
            schemas: Vec::new(),
            setup: None,
        }
    }

    fn invoke(
        op: String,
        envelope: node::InvocationEnvelope,
    ) -> Result<node::InvocationResult, node::NodeError> {
        let input: Value = match decode_cbor(&envelope.payload_cbor) {
            Ok(value) => value,
            Err(err) => {
                return Ok(node::InvocationResult {
                    ok: true,
                    output_cbor: canonical_cbor_bytes(
                        &serde_json::json!({"ok": false, "error": format!("invalid input cbor: {err}")}),
                    ),
                    output_metadata_cbor: None,
                });
            }
        };

        let output = run_op(&op, &input);

        Ok(node::InvocationResult {
            ok: true,
            output_cbor: canonical_cbor_bytes(&output),
            output_metadata_cbor: None,
        })
    }
}

#[cfg(target_arch = "wasm32")]
fn run_op(op: &str, input: &Value) -> Value {
    match op {
        "request" => handle_request(input),
        "stream" => handle_stream(input),
        other => serde_json::json!({"ok": false, "error": format!("unsupported op: {other}")}),
    }
}

#[cfg(target_arch = "wasm32")]
fn empty_canonical_schema() -> CanonicalSchemaIr {
    CanonicalSchemaIr::Object {
        properties: BTreeMap::new(),
        required: Vec::new(),
        additional: CanonicalAdditionalProperties::Allow,
    }
}

#[cfg(target_arch = "wasm32")]
fn component_info() -> ComponentInfo {
    ComponentInfo {
        id: COMPONENT_ID.to_string(),
        version: COMPONENT_VERSION.to_string(),
        role: "tool".to_string(),
        display_name: Some(CanonicalI18nText::new("http.component.display_name", None)),
    }
}

#[cfg(target_arch = "wasm32")]
fn component_info_cbor() -> Vec<u8> {
    canonical::to_canonical_cbor_allow_floats(&component_info()).unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn component_describe() -> ComponentDescribe {
    // The component-http describe surface is currently expressed via the local
    // SchemaIr in `describe.rs` (different shape from the canonical
    // greentic_types SchemaIr). For the canonical describe payload we use an
    // empty canonical Object schema as a structural placeholder; the
    // authoritative schemas are returned by component-schema's
    // input/output/config-schema functions (CBOR of the local SchemaIr).
    let stub = empty_canonical_schema();
    let op_schema_hash = schema_hash(&stub, &stub, &stub).unwrap_or_default();

    let request_op = ComponentOperation {
        id: "request".to_string(),
        display_name: Some(CanonicalI18nText::new("http.op.request.title", None)),
        input: ComponentRunInput {
            schema: stub.clone(),
        },
        output: ComponentRunOutput {
            schema: stub.clone(),
        },
        defaults: BTreeMap::new(),
        redactions: Vec::new(),
        constraints: BTreeMap::new(),
        schema_hash: op_schema_hash.clone(),
    };
    let stream_op = ComponentOperation {
        id: "stream".to_string(),
        display_name: Some(CanonicalI18nText::new("http.op.stream.title", None)),
        input: ComponentRunInput {
            schema: stub.clone(),
        },
        output: ComponentRunOutput {
            schema: stub.clone(),
        },
        defaults: BTreeMap::new(),
        redactions: Vec::new(),
        constraints: BTreeMap::new(),
        schema_hash: op_schema_hash,
    };

    ComponentDescribe {
        info: component_info(),
        provided_capabilities: Vec::new(),
        required_capabilities: vec![
            "host:http".to_string(),
            "host:secrets".to_string(),
            "host:telemetry".to_string(),
        ],
        metadata: BTreeMap::new(),
        operations: vec![request_op, stream_op],
        config_schema: stub,
    }
}

#[cfg(target_arch = "wasm32")]
fn component_describe_cbor() -> Vec<u8> {
    canonical::to_canonical_cbor_allow_floats(&component_describe()).unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
impl component_descriptor::Guest for Component {
    fn get_component_info() -> Vec<u8> {
        component_info_cbor()
    }

    fn describe() -> Vec<u8> {
        component_describe_cbor()
    }
}

#[cfg(target_arch = "wasm32")]
impl component_schema::Guest for Component {
    fn input_schema() -> Vec<u8> {
        canonical::to_canonical_cbor_allow_floats(&input_schema()).unwrap_or_default()
    }

    fn output_schema() -> Vec<u8> {
        canonical::to_canonical_cbor_allow_floats(&output_schema()).unwrap_or_default()
    }

    fn config_schema() -> Vec<u8> {
        canonical::to_canonical_cbor_allow_floats(&config_schema()).unwrap_or_default()
    }
}

#[cfg(target_arch = "wasm32")]
impl component_runtime::Guest for Component {
    fn run(input: Vec<u8>, state: Vec<u8>) -> component_runtime::RunResult {
        let input_json: Value = match decode_cbor(&input) {
            Ok(value) => value,
            Err(err) => {
                let err_payload = serde_json::json!({
                    "ok": false,
                    "error": format!("invalid input cbor: {err}"),
                });
                return component_runtime::RunResult {
                    output: canonical_cbor_bytes(&err_payload),
                    new_state: state,
                };
            }
        };

        let op = input_json
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("request");
        let output = run_op(op, &input_json);

        component_runtime::RunResult {
            output: canonical_cbor_bytes(&output),
            new_state: state,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl component_qa::Guest for Component {
    fn qa_spec(mode: QaMode) -> Vec<u8> {
        let mode_key = match mode {
            QaMode::Default => "default",
            QaMode::Setup => "setup",
            QaMode::Update => "update",
            QaMode::Remove => "remove",
        };
        canonical_cbor_bytes(&qa::canonical_qa_spec(mode_key))
    }

    fn apply_answers(mode: QaMode, current_config: Vec<u8>, answers: Vec<u8>) -> Vec<u8> {
        let _ = mode; // all modes route through apply_answers; no-op removal is handled by empty answers
        let answers: Value = match decode_cbor(&answers) {
            Ok(value) => value,
            Err(err) => {
                return canonical_cbor_bytes(&ApplyAnswersResult {
                    ok: false,
                    config: None,
                    error: Some(format!("invalid answers cbor: {err}")),
                });
            }
        };

        let base_cfg: http_core::config::ComponentConfig = if current_config.is_empty() {
            http_core::config::ComponentConfig::default()
        } else {
            match decode_cbor(&current_config) {
                Ok(v) => serde_json::from_value(v).unwrap_or_default(),
                Err(_) => http_core::config::ComponentConfig::default(),
            }
        };

        match http_core::config::apply_answers(base_cfg, &answers) {
            Ok(new_cfg) => canonical_cbor_bytes(&ApplyAnswersResult {
                ok: true,
                config: Some(new_cfg),
                error: None,
            }),
            Err(e) => canonical_cbor_bytes(&ApplyAnswersResult {
                ok: false,
                config: None,
                error: Some(e.to_string()),
            }),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl component_i18n::Guest for Component {
    fn i18n_keys() -> Vec<String> {
        I18N_KEYS.iter().map(|k| (*k).to_string()).collect()
    }
}

#[cfg(target_arch = "wasm32")]
bindings::export!(Component with_types_in bindings);

#[cfg(target_arch = "wasm32")]
greentic_interfaces_guest::export_component_v060!(NodeCompat);

// ============================================================================
// CBOR helpers
// ============================================================================

pub fn canonical_cbor_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    canonical::to_canonical_cbor(value).unwrap_or_default()
}

pub fn decode_cbor(bytes: &[u8]) -> Result<Value, String> {
    canonical::from_cbor(bytes).map_err(|e| e.to_string())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use greentic_types::schemas::component::v0_6_0::QuestionKind as CanonicalQuestionKind;
    use serde_json::json;

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

    #[test]
    fn test_setup_qa_spec_uses_canonical_number_question_for_timeout() {
        let spec = canonical_qa_spec("setup");
        let timeout = spec
            .questions
            .iter()
            .find(|question| question.id == "timeout_ms")
            .expect("timeout question");

        assert!(matches!(timeout.kind, CanonicalQuestionKind::Number));
        let timeout_default = spec.defaults.get("timeout_ms").expect("timeout default");
        assert_eq!(serde_json::to_value(timeout_default).unwrap(), json!(30000));
    }
}
