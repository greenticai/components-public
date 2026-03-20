#![allow(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]
#![allow(clippy::collapsible_if)]

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
mod bindings {
    wit_bindgen::generate!({ path: "wit/msg2events", world: "component-v0-v6-v0", generate_all });
}

#[cfg(target_arch = "wasm32")]
use bindings::greentic::component::logger_api;

const COMPONENT_ID: &str = "msg2events";
const WORLD_ID: &str = "component-v0-v6-v0";

const I18N_KEYS: &[&str] = &[
    "msg2events.op.route.title",
    "msg2events.op.route.description",
    "msg2events.op.extract.title",
    "msg2events.op.extract.description",
    "msg2events.op.validate.title",
    "msg2events.op.validate.description",
    "msg2events.schema.input.title",
    "msg2events.schema.input.description",
    "msg2events.schema.input.message.title",
    "msg2events.schema.input.message.description",
    "msg2events.schema.input.target_flow.title",
    "msg2events.schema.input.target_flow.description",
    "msg2events.schema.input.event_type.title",
    "msg2events.schema.input.event_type.description",
    "msg2events.schema.output.title",
    "msg2events.schema.output.description",
    "msg2events.schema.output.event.title",
    "msg2events.schema.output.event.description",
    "msg2events.schema.config.title",
    "msg2events.schema.config.description",
    "msg2events.schema.config.default_flow.title",
    "msg2events.schema.config.default_flow.description",
    "msg2events.schema.config.default_event_type.title",
    "msg2events.schema.config.default_event_type.description",
    "msg2events.qa.default.title",
    "msg2events.qa.setup.title",
    "msg2events.qa.setup.default_flow",
    "msg2events.qa.setup.default_event_type",
];

/// Component configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentConfig {
    #[serde(default)]
    pub default_flow: Option<String>,
    #[serde(default)]
    pub default_event_type: Option<String>,
    #[serde(default)]
    pub extract_mappings: Option<BTreeMap<String, String>>,
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

/// Event payload output structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventPayload {
    pub event_type: String,
    pub source: EventSource,
    pub data: Value,
    pub metadata: Option<Value>,
    pub timestamp: Option<String>,
}

/// Event source information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSource {
    pub provider: String,
    pub channel_id: Option<String>,
    pub conversation_id: Option<String>,
    pub user_id: Option<String>,
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
            "extract" => handle_extract(&input),
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
                default_flow: get_str("default_flow"),
                default_event_type: get_str("default_event_type"),
                extract_mappings: None,
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
// Core Msg2Events Logic
// ============================================================================

/// Handle route operation - transforms message to event and routes to events flow
pub fn handle_route(input: &Value) -> Value {
    let cfg = match load_config(input) {
        Ok(cfg) => cfg,
        Err(err) => return json!({"ok": false, "error": err}),
    };

    // Extract event from message
    let event_payload = extract_event_from_message(input, &cfg);

    // Get target flow
    let target_flow = input
        .get("target_flow")
        .and_then(Value::as_str)
        .map(String::from)
        .or(cfg.default_flow.clone());

    log_event("route_success");

    json!({
        "ok": true,
        "event": event_payload,
        "target_flow": target_flow,
        "nats_subject": build_nats_subject(input),
    })
}

/// Handle extract operation - extracts event data from message without routing
pub fn handle_extract(input: &Value) -> Value {
    let cfg = match load_config(input) {
        Ok(cfg) => cfg,
        Err(err) => return json!({"ok": false, "error": err}),
    };

    let event_payload = extract_event_from_message(input, &cfg);

    log_event("extract_success");

    json!({
        "ok": true,
        "event": event_payload,
    })
}

/// Handle validate operation - validates message payload before routing
pub fn handle_validate(input: &Value) -> Value {
    let mut errors: Vec<String> = Vec::new();

    // Check for message content
    if input.get("message").is_none()
        && input.get("activity").is_none()
        && input.get("text").is_none()
    {
        errors.push("missing message content (message, activity, or text field)".to_string());
    }

    // Validate source provider if specified
    if let Some(provider) = input.get("source_provider").and_then(Value::as_str) {
        let valid_providers = [
            "telegram", "slack", "teams", "webchat", "whatsapp", "webex", "email", "sms",
        ];
        if !valid_providers.contains(&provider) {
            errors.push(format!("invalid source_provider: {provider}"));
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

/// Extract event payload from messaging input
fn extract_event_from_message(input: &Value, cfg: &ComponentConfig) -> EventPayload {
    // Determine event type
    let event_type = input
        .get("event_type")
        .and_then(Value::as_str)
        .map(String::from)
        .or(cfg.default_event_type.clone())
        .unwrap_or_else(|| determine_event_type(input));

    // Extract source information
    let source = EventSource {
        provider: input
            .get("source_provider")
            .or_else(|| input.get("provider"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        channel_id: input
            .get("channel_id")
            .and_then(Value::as_str)
            .map(String::from),
        conversation_id: input
            .get("conversation_id")
            .and_then(Value::as_str)
            .map(String::from),
        user_id: input
            .get("user_id")
            .or_else(|| input.get("from").and_then(|f| f.get("id")))
            .and_then(Value::as_str)
            .map(String::from),
    };

    // Extract message data
    let data = extract_message_data(input);

    // Get metadata
    let metadata = input.get("metadata").cloned();

    // Get timestamp
    let timestamp = input
        .get("timestamp")
        .and_then(Value::as_str)
        .map(String::from);

    EventPayload {
        event_type,
        source,
        data,
        metadata,
        timestamp,
    }
}

/// Determine event type from message content
fn determine_event_type(input: &Value) -> String {
    // Check for specific message types
    if input.get("activity").is_some() {
        if let Some(activity_type) = input
            .get("activity")
            .and_then(|a| a.get("type"))
            .and_then(Value::as_str)
        {
            return format!("message.{}", activity_type.to_lowercase());
        }
    }

    // Check for attachments
    if input.get("attachments").is_some() {
        return "message.attachment".to_string();
    }

    // Check for card/adaptive card
    if input.get("card").is_some() || input.get("adaptive_card").is_some() {
        return "message.card".to_string();
    }

    // Check for command
    if let Some(text) = input.get("text").and_then(Value::as_str) {
        if text.starts_with('/') {
            return "message.command".to_string();
        }
    }

    // Default
    "message.text".to_string()
}

/// Extract message data from various message formats
fn extract_message_data(input: &Value) -> Value {
    let mut data = serde_json::Map::new();

    // Extract text content
    if let Some(text) = input.get("text").and_then(Value::as_str) {
        data.insert("text".to_string(), Value::String(text.to_string()));
    } else if let Some(message) = input.get("message") {
        if let Some(text) = message.get("text").and_then(Value::as_str) {
            data.insert("text".to_string(), Value::String(text.to_string()));
        } else if message.is_string() {
            data.insert("text".to_string(), message.clone());
        }
    }

    // Extract activity data (BotFramework format)
    if let Some(activity) = input.get("activity") {
        if let Some(text) = activity.get("text").and_then(Value::as_str) {
            data.insert("text".to_string(), Value::String(text.to_string()));
        }
        if let Some(from) = activity.get("from") {
            data.insert("from".to_string(), from.clone());
        }
        if let Some(attachments) = activity.get("attachments") {
            data.insert("attachments".to_string(), attachments.clone());
        }
    }

    // Extract attachments
    if let Some(attachments) = input.get("attachments") {
        data.insert("attachments".to_string(), attachments.clone());
    }

    // Extract entities (mentions, hashtags, etc.)
    if let Some(entities) = input.get("entities") {
        data.insert("entities".to_string(), entities.clone());
    }

    // Extract reply context
    if let Some(reply_to) = input
        .get("reply_to_message_id")
        .or_else(|| input.get("reply_to"))
    {
        data.insert("reply_to".to_string(), reply_to.clone());
    }

    // Include raw message if minimal data extracted
    if data.is_empty() {
        if let Some(raw) = input.get("message").or_else(|| input.get("activity")) {
            data.insert("raw".to_string(), raw.clone());
        }
    }

    Value::Object(data)
}

/// Build NATS subject for events
fn build_nats_subject(input: &Value) -> String {
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

    format!("greentic.events.ingress.{env}.{tenant}.{team}")
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
                "msg2events.op.route.title",
                "msg2events.op.route.description",
            ),
            op(
                "extract",
                "msg2events.op.extract.title",
                "msg2events.op.extract.description",
            ),
            op(
                "validate",
                "msg2events.op.validate.title",
                "msg2events.op.validate.description",
            ),
        ],
        input_schema: input_schema.clone(),
        output_schema: output_schema.clone(),
        config_schema: config_schema.clone(),
        redactions: vec![],
        schema_hash: "msg2events-schema-v1".to_string(),
    }
}

#[cfg(target_arch = "wasm32")]
fn build_qa_spec_wasm(mode: bindings::exports::greentic::component::qa::Mode) -> QaSpec {
    use bindings::exports::greentic::component::qa::Mode;

    match mode {
        Mode::Default => QaSpec {
            mode: "default".to_string(),
            title: i18n("msg2events.qa.default.title"),
            description: None,
            questions: Vec::new(),
            defaults: json!({}),
        },
        Mode::Setup => QaSpec {
            mode: "setup".to_string(),
            title: i18n("msg2events.qa.setup.title"),
            description: None,
            questions: vec![
                qa_q("default_flow", "msg2events.qa.setup.default_flow", false),
                qa_q(
                    "default_event_type",
                    "msg2events.qa.setup.default_event_type",
                    false,
                ),
            ],
            defaults: json!({
                "default_event_type": "message.text",
            }),
        },
        Mode::Upgrade | Mode::Remove => QaSpec {
            mode: if mode == Mode::Upgrade {
                "upgrade"
            } else {
                "remove"
            }
            .to_string(),
            title: i18n("msg2events.qa.default.title"),
            description: None,
            questions: Vec::new(),
            defaults: json!({}),
        },
    }
}

fn input_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "message".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::Object {
                title: i18n("msg2events.schema.input.message.title"),
                description: i18n("msg2events.schema.input.message.description"),
                fields: BTreeMap::new(),
                additional_properties: true,
            },
        },
    );
    fields.insert(
        "target_flow".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("msg2events.schema.input.target_flow.title"),
                description: i18n("msg2events.schema.input.target_flow.description"),
                format: None,
                secret: false,
            },
        },
    );
    fields.insert(
        "event_type".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("msg2events.schema.input.event_type.title"),
                description: i18n("msg2events.schema.input.event_type.description"),
                format: None,
                secret: false,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("msg2events.schema.input.title"),
        description: i18n("msg2events.schema.input.description"),
        fields,
        additional_properties: true,
    }
}

fn output_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "event".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Object {
                title: i18n("msg2events.schema.output.event.title"),
                description: i18n("msg2events.schema.output.event.description"),
                fields: BTreeMap::new(),
                additional_properties: true,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("msg2events.schema.output.title"),
        description: i18n("msg2events.schema.output.description"),
        fields,
        additional_properties: true,
    }
}

fn config_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "default_flow".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("msg2events.schema.config.default_flow.title"),
                description: i18n("msg2events.schema.config.default_flow.description"),
                format: None,
                secret: false,
            },
        },
    );
    fields.insert(
        "default_event_type".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("msg2events.schema.config.default_event_type.title"),
                description: i18n("msg2events.schema.config.default_event_type.description"),
                format: None,
                secret: false,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("msg2events.schema.config.title"),
        description: i18n("msg2events.schema.config.description"),
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
        flow_id: "msg2events-component".into(),
        node_id: None,
        provider: "msg2events".into(),
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
            "text": "Hello from messaging",
            "source_provider": "telegram"
        });
        let output = handle_route(&input);
        assert_eq!(output["ok"], Value::Bool(true));
        assert!(output["event"].is_object());
    }

    #[test]
    fn test_handle_extract_with_activity() {
        let input = json!({
            "activity": {
                "type": "message",
                "text": "Hello from BotFramework",
                "from": {
                    "id": "user123",
                    "name": "John"
                }
            },
            "source_provider": "webchat"
        });
        let output = handle_extract(&input);
        assert_eq!(output["ok"], Value::Bool(true));
        let event = &output["event"];
        assert_eq!(event["source"]["provider"], "webchat");
    }

    #[test]
    fn test_handle_validate_success() {
        let input = json!({
            "text": "test message",
            "source_provider": "telegram"
        });
        let output = handle_validate(&input);
        assert_eq!(output["ok"], Value::Bool(true));
        assert_eq!(output["valid"], Value::Bool(true));
    }

    #[test]
    fn test_handle_validate_invalid_provider() {
        let input = json!({
            "text": "test",
            "source_provider": "invalid_provider"
        });
        let output = handle_validate(&input);
        assert_eq!(output["ok"], Value::Bool(true));
        assert_eq!(output["valid"], Value::Bool(false));
    }

    #[test]
    fn test_handle_validate_missing_message() {
        let input = json!({});
        let output = handle_validate(&input);
        assert_eq!(output["valid"], Value::Bool(false));
    }

    #[test]
    fn test_determine_event_type_command() {
        let input = json!({"text": "/start"});
        let event_type = determine_event_type(&input);
        assert_eq!(event_type, "message.command");
    }

    #[test]
    fn test_determine_event_type_attachment() {
        let input = json!({"attachments": [{"type": "image"}]});
        let event_type = determine_event_type(&input);
        assert_eq!(event_type, "message.attachment");
    }

    #[test]
    fn test_determine_event_type_default() {
        let input = json!({"text": "hello"});
        let event_type = determine_event_type(&input);
        assert_eq!(event_type, "message.text");
    }

    #[test]
    fn test_build_nats_subject() {
        let input = json!({
            "env": "prod",
            "tenant": "acme",
            "team": "support"
        });
        let subject = build_nats_subject(&input);
        assert_eq!(subject, "greentic.events.ingress.prod.acme.support");
    }

    #[test]
    fn test_extract_message_data() {
        let input = json!({
            "text": "Hello world",
            "attachments": [{"type": "file", "url": "https://example.com/file.pdf"}],
            "entities": [{"type": "mention", "text": "@user"}]
        });
        let data = extract_message_data(&input);
        assert_eq!(data["text"], "Hello world");
        assert!(data["attachments"].is_array());
        assert!(data["entities"].is_array());
    }
}
