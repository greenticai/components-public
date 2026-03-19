#![allow(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
mod bindings {
    wit_bindgen::generate!({ path: "wit/events2msg", world: "component-v0-v6-v0", generate_all });
}

#[cfg(target_arch = "wasm32")]
use bindings::greentic::telemetry::logger_api;

const COMPONENT_ID: &str = "events2msg";
const WORLD_ID: &str = "component-v0-v6-v0";

const I18N_KEYS: &[&str] = &[
    "events2msg.op.route.title",
    "events2msg.op.route.description",
    "events2msg.op.validate.title",
    "events2msg.op.validate.description",
    "events2msg.schema.input.title",
    "events2msg.schema.input.description",
    "events2msg.schema.input.event.title",
    "events2msg.schema.input.event.description",
    "events2msg.schema.input.target_provider.title",
    "events2msg.schema.input.target_provider.description",
    "events2msg.schema.input.channel_id.title",
    "events2msg.schema.input.channel_id.description",
    "events2msg.schema.input.message_template.title",
    "events2msg.schema.input.message_template.description",
    "events2msg.schema.output.title",
    "events2msg.schema.output.description",
    "events2msg.schema.output.provider.title",
    "events2msg.schema.output.provider.description",
    "events2msg.schema.output.payload.title",
    "events2msg.schema.output.payload.description",
    "events2msg.schema.config.title",
    "events2msg.schema.config.description",
    "events2msg.schema.config.default_provider.title",
    "events2msg.schema.config.default_provider.description",
    "events2msg.schema.config.default_channel.title",
    "events2msg.schema.config.default_channel.description",
    "events2msg.qa.default.title",
    "events2msg.qa.setup.title",
    "events2msg.qa.setup.default_provider",
    "events2msg.qa.setup.default_channel",
];

/// Component configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentConfig {
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub default_channel: Option<String>,
    #[serde(default)]
    pub payload_mappings: Option<BTreeMap<String, String>>,
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

/// Messaging payload output structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagingPayload {
    pub provider: String,
    pub channel_id: Option<String>,
    pub conversation_id: Option<String>,
    pub message: MessagingMessage,
    pub metadata: Option<Value>,
}

/// Messaging message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagingMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub text: Option<String>,
    pub attachments: Option<Vec<Value>>,
    pub card: Option<Value>,
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
            "route" => handle_route(&input),
            "validate" => handle_validate(&input),
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
                default_provider: get_str("default_provider"),
                default_channel: get_str("default_channel"),
                payload_mappings: None,
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
// Core Events2Msg Logic
// ============================================================================

/// Handle route operation - transforms event to messaging payload
pub fn handle_route(input: &Value) -> Value {
    let cfg = match load_config(input) {
        Ok(cfg) => cfg,
        Err(err) => return json!({"ok": false, "error": err}),
    };

    // Get target provider (from input or config default)
    let target_provider = input
        .get("target_provider")
        .and_then(Value::as_str)
        .map(String::from)
        .or(cfg.default_provider.clone())
        .unwrap_or_else(|| "webchat".to_string());

    // Get channel/conversation ID
    let channel_id = input
        .get("channel_id")
        .and_then(Value::as_str)
        .map(String::from)
        .or(cfg.default_channel.clone());

    let conversation_id = input
        .get("conversation_id")
        .and_then(Value::as_str)
        .map(String::from);

    // Get event data
    let event = input.get("event").cloned().unwrap_or_else(|| json!({}));

    // Build messaging payload
    let message = build_message_from_event(&event, input);

    let payload = MessagingPayload {
        provider: target_provider.clone(),
        channel_id,
        conversation_id,
        message,
        metadata: input.get("metadata").cloned(),
    };

    log_event("route_success");

    json!({
        "ok": true,
        "provider": target_provider,
        "payload": payload,
        "nats_subject": build_nats_subject(&target_provider, input),
    })
}

/// Handle validate operation - validates event payload before routing
pub fn handle_validate(input: &Value) -> Value {
    let mut errors: Vec<String> = Vec::new();

    // Check required fields
    if input.get("event").is_none() && input.get("message").is_none() {
        errors.push("missing 'event' or 'message' field".to_string());
    }

    // Validate target_provider if specified
    if let Some(provider) = input.get("target_provider").and_then(Value::as_str) {
        let valid_providers = ["telegram", "slack", "teams", "webchat", "whatsapp", "webex"];
        if !valid_providers.contains(&provider) {
            errors.push(format!("invalid target_provider: {provider}"));
        }
    }

    if errors.is_empty() {
        log_event("validate_success");
        json!({
            "ok": true,
            "valid": true,
        })
    } else {
        json!({
            "ok": true,
            "valid": false,
            "errors": errors,
        })
    }
}

/// Build messaging message from event data
fn build_message_from_event(event: &Value, input: &Value) -> MessagingMessage {
    // Try to get message text from various sources
    let text = input
        .get("message_template")
        .and_then(Value::as_str)
        .map(|template| interpolate_template(template, event))
        .or_else(|| {
            input
                .get("message")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .or_else(|| event.get("text").and_then(Value::as_str).map(String::from))
        .or_else(|| {
            event
                .get("message")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .or_else(|| {
            event
                .get("content")
                .and_then(Value::as_str)
                .map(String::from)
        });

    // Get message type
    let message_type = input
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or("message")
        .to_string();

    // Get attachments if any
    let attachments = input.get("attachments").and_then(Value::as_array).cloned();

    // Get card if any (for adaptive cards)
    let card = input.get("card").cloned();

    MessagingMessage {
        message_type,
        text,
        attachments,
        card,
    }
}

/// Simple template interpolation ({{field}} syntax)
fn interpolate_template(template: &str, data: &Value) -> String {
    let mut result = template.to_string();

    if let Value::Object(obj) = data {
        for (key, value) in obj {
            let placeholder = format!("{{{{{}}}}}", key);
            let replacement = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => value.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }

    result
}

/// Build NATS subject for messaging
fn build_nats_subject(provider: &str, input: &Value) -> String {
    let env = input
        .get("env")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let tenant = input
        .get("tenant")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let team = input
        .get("team")
        .and_then(Value::as_str)
        .unwrap_or("default");

    format!("greentic.messaging.egress.{env}.{tenant}.{team}.{provider}")
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
                "route",
                "events2msg.op.route.title",
                "events2msg.op.route.description",
            ),
            op(
                "validate",
                "events2msg.op.validate.title",
                "events2msg.op.validate.description",
            ),
        ],
        input_schema: input_schema.clone(),
        output_schema: output_schema.clone(),
        config_schema: config_schema.clone(),
        redactions: vec![],
        schema_hash: "events2msg-schema-v1".to_string(),
    }
}

#[cfg(target_arch = "wasm32")]
fn build_qa_spec_wasm(mode: bindings::exports::greentic::component::qa::Mode) -> QaSpec {
    use bindings::exports::greentic::component::qa::Mode;

    match mode {
        Mode::Default => QaSpec {
            mode: "default".to_string(),
            title: i18n("events2msg.qa.default.title"),
            description: None,
            questions: Vec::new(),
            defaults: json!({}),
        },
        Mode::Setup => QaSpec {
            mode: "setup".to_string(),
            title: i18n("events2msg.qa.setup.title"),
            description: None,
            questions: vec![
                qa_q(
                    "default_provider",
                    "events2msg.qa.setup.default_provider",
                    false,
                ),
                qa_q(
                    "default_channel",
                    "events2msg.qa.setup.default_channel",
                    false,
                ),
            ],
            defaults: json!({
                "default_provider": "webchat",
            }),
        },
        Mode::Upgrade | Mode::Remove => QaSpec {
            mode: if mode == Mode::Upgrade {
                "upgrade"
            } else {
                "remove"
            }
            .to_string(),
            title: i18n("events2msg.qa.default.title"),
            description: None,
            questions: Vec::new(),
            defaults: json!({}),
        },
    }
}

fn input_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "event".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::Object {
                title: i18n("events2msg.schema.input.event.title"),
                description: i18n("events2msg.schema.input.event.description"),
                fields: BTreeMap::new(),
                additional_properties: true,
            },
        },
    );
    fields.insert(
        "target_provider".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("events2msg.schema.input.target_provider.title"),
                description: i18n("events2msg.schema.input.target_provider.description"),
                format: None,
                secret: false,
            },
        },
    );
    fields.insert(
        "channel_id".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("events2msg.schema.input.channel_id.title"),
                description: i18n("events2msg.schema.input.channel_id.description"),
                format: None,
                secret: false,
            },
        },
    );
    fields.insert(
        "message_template".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("events2msg.schema.input.message_template.title"),
                description: i18n("events2msg.schema.input.message_template.description"),
                format: None,
                secret: false,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("events2msg.schema.input.title"),
        description: i18n("events2msg.schema.input.description"),
        fields,
        additional_properties: true,
    }
}

fn output_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "provider".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::String {
                title: i18n("events2msg.schema.output.provider.title"),
                description: i18n("events2msg.schema.output.provider.description"),
                format: None,
                secret: false,
            },
        },
    );
    fields.insert(
        "payload".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Object {
                title: i18n("events2msg.schema.output.payload.title"),
                description: i18n("events2msg.schema.output.payload.description"),
                fields: BTreeMap::new(),
                additional_properties: true,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("events2msg.schema.output.title"),
        description: i18n("events2msg.schema.output.description"),
        fields,
        additional_properties: true,
    }
}

fn config_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "default_provider".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("events2msg.schema.config.default_provider.title"),
                description: i18n("events2msg.schema.config.default_provider.description"),
                format: None,
                secret: false,
            },
        },
    );
    fields.insert(
        "default_channel".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("events2msg.schema.config.default_channel.title"),
                description: i18n("events2msg.schema.config.default_channel.description"),
                format: None,
                secret: false,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("events2msg.schema.config.title"),
        description: i18n("events2msg.schema.config.description"),
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
        flow_id: "events2msg-component".into(),
        node_id: None,
        provider: "events2msg".into(),
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_route_basic() {
        let input = json!({
            "event": {
                "text": "Hello from event"
            },
            "target_provider": "telegram"
        });
        let output = handle_route(&input);
        assert_eq!(output["ok"], Value::Bool(true));
        assert_eq!(output["provider"], "telegram");
    }

    #[test]
    fn test_handle_route_with_template() {
        let input = json!({
            "event": {
                "name": "John",
                "action": "signed up"
            },
            "message_template": "User {{name}} has {{action}}",
            "target_provider": "slack"
        });
        let output = handle_route(&input);
        assert_eq!(output["ok"], Value::Bool(true));
        let payload = &output["payload"];
        assert_eq!(payload["message"]["text"], "User John has signed up");
    }

    #[test]
    fn test_handle_validate_success() {
        let input = json!({
            "event": { "text": "test" },
            "target_provider": "telegram"
        });
        let output = handle_validate(&input);
        assert_eq!(output["ok"], Value::Bool(true));
        assert_eq!(output["valid"], Value::Bool(true));
    }

    #[test]
    fn test_handle_validate_invalid_provider() {
        let input = json!({
            "event": { "text": "test" },
            "target_provider": "invalid_provider"
        });
        let output = handle_validate(&input);
        assert_eq!(output["ok"], Value::Bool(true));
        assert_eq!(output["valid"], Value::Bool(false));
    }

    #[test]
    fn test_handle_validate_missing_event() {
        let input = json!({});
        let output = handle_validate(&input);
        assert_eq!(output["valid"], Value::Bool(false));
    }

    #[test]
    fn test_interpolate_template() {
        let template = "Hello {{name}}, your order #{{order_id}} is ready";
        let data = json!({
            "name": "Alice",
            "order_id": 12345
        });
        let result = interpolate_template(template, &data);
        assert_eq!(result, "Hello Alice, your order #12345 is ready");
    }

    #[test]
    fn test_build_nats_subject() {
        let input = json!({
            "env": "prod",
            "tenant": "acme",
            "team": "support"
        });
        let subject = build_nats_subject("telegram", &input);
        assert_eq!(
            subject,
            "greentic.messaging.egress.prod.acme.support.telegram"
        );
    }

    #[test]
    fn test_build_nats_subject_defaults() {
        let input = json!({});
        let subject = build_nats_subject("webchat", &input);
        assert_eq!(
            subject,
            "greentic.messaging.egress.default.default.default.webchat"
        );
    }
}
