#[cfg(target_arch = "wasm32")]
use std::collections::BTreeMap;

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;
#[cfg(target_arch = "wasm32")]
use greentic_types::cbor::canonical;
#[cfg(target_arch = "wasm32")]
use greentic_types::schemas::common::schema_ir::{AdditionalProperties, SchemaIr};
use serde_json::{Map, Value};

const COMPONENT_NAME: &str = "component-pack2flow";
const COMPONENT_ORG: &str = "ai.greentic";
#[cfg(target_arch = "wasm32")]
const COMPONENT_ID: &str = "ai.greentic.component-pack2flow";
const COMPONENT_VERSION: &str = "0.1.0";
const DEFAULT_MAX_REDIRECTS: u64 = 3;
#[cfg(target_arch = "wasm32")]
const DEFAULT_OPERATION: &str = "handle_message";

#[cfg(target_arch = "wasm32")]
#[used]
#[unsafe(link_section = ".greentic.wasi")]
static WASI_TARGET_MARKER: [u8; 13] = *b"wasm32-wasip2";

#[cfg(target_arch = "wasm32")]
struct Component;

#[cfg(target_arch = "wasm32")]
impl node::Guest for Component {
    fn describe() -> node::ComponentDescriptor {
        node::ComponentDescriptor {
            name: COMPONENT_ID.to_string(),
            version: COMPONENT_VERSION.to_string(),
            summary: Some(
                "Jump control component that routes payloads into target flows".to_string(),
            ),
            capabilities: Vec::new(),
            ops: vec![node::Op {
                name: DEFAULT_OPERATION.to_string(),
                summary: Some("Emit a jump control directive to another flow".to_string()),
                input: node::IoSchema {
                    schema: node::SchemaSource::InlineCbor(encode_cbor(&input_schema())),
                    content_type: "application/cbor".to_string(),
                    schema_version: None,
                },
                output: node::IoSchema {
                    schema: node::SchemaSource::InlineCbor(encode_cbor(&output_schema())),
                    content_type: "application/cbor".to_string(),
                    schema_version: None,
                },
                examples: Vec::new(),
            }],
            schemas: Vec::new(),
            setup: None,
        }
    }

    fn invoke(
        op: String,
        envelope: node::InvocationEnvelope,
    ) -> Result<node::InvocationResult, node::NodeError> {
        let operation = if op.is_empty() {
            DEFAULT_OPERATION
        } else {
            &op
        };
        let input: Value = match canonical::from_cbor(&envelope.payload_cbor) {
            Ok(value) => value,
            Err(error) => {
                return Ok(node::InvocationResult {
                    ok: true,
                    output_cbor: encode_cbor(&error_response(&JumpError::InvalidInput(format!(
                        "CBOR payload decode failed: {error}"
                    )))),
                    output_metadata_cbor: None,
                });
            }
        };

        let output = process_invocation(operation, &input);
        Ok(node::InvocationResult {
            ok: true,
            output_cbor: encode_cbor(&output),
            output_metadata_cbor: None,
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod qa_exports {
    wit_bindgen::generate!({
        inline: r#"
            package greentic:component@0.6.0;

            interface component-qa {
              enum qa-mode {
                default,
                setup,
                update,
                remove,
              }

              qa-spec: func(mode: qa-mode) -> list<u8>;
              apply-answers: func(mode: qa-mode, current-config: list<u8>, answers: list<u8>) -> list<u8>;
            }

            interface component-i18n {
              i18n-keys: func() -> list<string>;
            }

            world wizard-support {
              export component-qa;
              export component-i18n;
            }
        "#,
        world: "wizard-support",
    });

    pub struct WizardSupport;

    impl exports::greentic::component::component_qa::Guest for WizardSupport {
        fn qa_spec(mode: exports::greentic::component::component_qa::QaMode) -> Vec<u8> {
            crate::encode_cbor(&crate::qa_spec_json(match mode {
                exports::greentic::component::component_qa::QaMode::Default => "default",
                exports::greentic::component::component_qa::QaMode::Setup => "setup",
                exports::greentic::component::component_qa::QaMode::Update => "update",
                exports::greentic::component::component_qa::QaMode::Remove => "remove",
            }))
        }

        fn apply_answers(
            _mode: exports::greentic::component::component_qa::QaMode,
            current_config: Vec<u8>,
            answers: Vec<u8>,
        ) -> Vec<u8> {
            crate::apply_answers_cbor(current_config, answers)
        }
    }

    impl exports::greentic::component::component_i18n::Guest for WizardSupport {
        fn i18n_keys() -> Vec<String> {
            crate::i18n_keys()
        }
    }

    export!(WizardSupport with_types_in self);
}

#[cfg(target_arch = "wasm32")]
greentic_interfaces_guest::export_component_v060!(Component);

#[derive(Debug, Clone, PartialEq, Eq)]
enum JumpError {
    MissingFlow,
    InvalidFlow,
    EmptyNode,
    InvalidNode,
    InvalidInput(String),
    JumpFailed(String),
}

impl JumpError {
    fn code(&self) -> &'static str {
        match self {
            Self::MissingFlow => "missing_flow",
            Self::InvalidFlow => "invalid_input.flow_invalid",
            Self::EmptyNode => "invalid_input.node_empty",
            Self::InvalidNode => "invalid_input.node_invalid",
            Self::InvalidInput(_) => "invalid_input",
            Self::JumpFailed(_) => "jump_failed",
        }
    }

    fn text(&self) -> String {
        match self {
            Self::MissingFlow => "Missing required target.flow".to_string(),
            Self::InvalidFlow => {
                "target.flow contains invalid characters (allowed: [A-Za-z0-9._-])".to_string()
            }
            Self::EmptyNode => "target.node must not be empty or whitespace".to_string(),
            Self::InvalidNode => {
                "target.node contains invalid characters (allowed: [A-Za-z0-9._-])".to_string()
            }
            Self::InvalidInput(message) => message.clone(),
            Self::JumpFailed(message) => message.clone(),
        }
    }
}

pub fn describe_payload() -> String {
    serde_json::json!({
        "component": {
            "name": COMPONENT_NAME,
            "org": COMPONENT_ORG,
            "version": COMPONENT_VERSION,
            "world": "greentic:component/component-v0-v6-v0@0.6.0",
            "schemas": {
                "component": "schemas/component.schema.json",
                "input": "schemas/io/input.schema.json",
                "output": "schemas/io/output.schema.json"
            }
        }
    })
    .to_string()
}

pub fn handle_message(operation: &str, input: &str) -> String {
    let invocation = match serde_json::from_str::<Value>(input) {
        Ok(value) => value,
        Err(error) => {
            return error_response(&JumpError::InvalidInput(format!(
                "Input must be valid JSON object: {error}"
            )))
            .to_string();
        }
    };

    process_invocation(operation, &invocation).to_string()
}

type Cbor = Vec<u8>;

// Adapter for runtime jump/transfer primitive. In v1 we emit a control directive
// in component output and keep this adapter as the single integration point.
fn jump(flow: &str, node: Option<&str>, payload: Cbor, hints: Cbor) -> Result<(), JumpError> {
    if flow.trim().is_empty() {
        return Err(JumpError::JumpFailed("empty target flow".to_string()));
    }

    let _ = (node, payload, hints);
    Ok(())
}

fn process_invocation(operation: &str, input: &Value) -> Value {
    match execute_pack2flow(operation, input) {
        Ok(value) => value,
        Err(error) => error_response(&error),
    }
}

fn execute_pack2flow(operation: &str, input: &Value) -> Result<Value, JumpError> {
    let root = input
        .as_object()
        .ok_or_else(|| JumpError::InvalidInput("Input must be a JSON object".to_string()))?;

    let target = root
        .get("target")
        .and_then(Value::as_object)
        .ok_or(JumpError::MissingFlow)?;

    let flow = target
        .get("flow")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(JumpError::MissingFlow)?;
    if !is_valid_identifier(flow) {
        return Err(JumpError::InvalidFlow);
    }

    let node = resolve_target_node(target.get("node"))?;

    let current_payload = object_field(root, "payload");
    let param_defaults = object_field(root, "params");
    let merged_payload = shallow_merge(param_defaults, current_payload);

    let current_hints = root
        .get("routing_hints")
        .and_then(Value::as_object)
        .cloned()
        .or_else(|| {
            root.get("current_hints")
                .and_then(Value::as_object)
                .cloned()
        })
        .unwrap_or_default();
    let hint_defaults = object_field(root, "hints");
    let merged_hints = shallow_merge(hint_defaults, current_hints);

    let max_redirects = root
        .get("max_redirects")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_REDIRECTS);
    let payload_cbor = serde_json::to_vec(&Value::Object(merged_payload.clone()))
        .map_err(|error| JumpError::JumpFailed(format!("Payload encoding failed: {error}")))?;
    let hints_cbor = serde_json::to_vec(&Value::Object(merged_hints.clone()))
        .map_err(|error| JumpError::JumpFailed(format!("Hints encoding failed: {error}")))?;
    jump(flow, node.as_deref(), payload_cbor, hints_cbor)?;

    let reason = root
        .get("reason")
        .and_then(Value::as_str)
        .map(std::string::ToString::to_string);

    Ok(serde_json::json!({
        "greentic_control": {
            "v": 1,
            "action": "jump",
            "operation": operation,
            "target": {
                "flow": flow,
                "node": node,
            },
            "params": merged_payload,
            "hints": merged_hints,
            "max_redirects": max_redirects,
            "reason": reason
        }
    }))
}

fn object_field(root: &Map<String, Value>, key: &str) -> Map<String, Value> {
    root.get(key)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn shallow_merge(
    mut defaults: Map<String, Value>,
    current: Map<String, Value>,
) -> Map<String, Value> {
    defaults.extend(current);
    defaults
}

fn resolve_target_node(explicit_node: Option<&Value>) -> Result<Option<String>, JumpError> {
    let Some(raw) = explicit_node else {
        return Ok(None);
    };
    let node = raw
        .as_str()
        .ok_or_else(|| JumpError::InvalidInput("target.node must be a string".to_string()))?;
    let trimmed = node.trim();
    if trimmed.is_empty() {
        return Err(JumpError::EmptyNode);
    }
    if !is_valid_identifier(trimmed) {
        return Err(JumpError::InvalidNode);
    }
    Ok(Some(trimmed.to_string()))
}

fn is_valid_identifier(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn error_response(error: &JumpError) -> Value {
    serde_json::json!({
        "status": "error",
        "error": {
            "code": error.code(),
            "text": error.text(),
        }
    })
}

#[cfg(target_arch = "wasm32")]
fn encode_cbor<T: serde::Serialize>(value: &T) -> Vec<u8> {
    canonical::to_canonical_cbor_allow_floats(value).expect("encode cbor")
}

#[cfg(target_arch = "wasm32")]
fn input_schema() -> SchemaIr {
    SchemaIr::Object {
        properties: BTreeMap::from([(
            "target".to_string(),
            SchemaIr::Object {
                properties: BTreeMap::from([(
                    "flow".to_string(),
                    SchemaIr::String {
                        min_len: Some(1),
                        max_len: None,
                        regex: None,
                        format: None,
                    },
                )]),
                required: vec!["flow".to_string()],
                additional: AdditionalProperties::Allow,
            },
        )]),
        required: vec!["target".to_string()],
        additional: AdditionalProperties::Allow,
    }
}

#[cfg(target_arch = "wasm32")]
fn output_schema() -> SchemaIr {
    SchemaIr::Object {
        properties: BTreeMap::from([
            (
                "greentic_control".to_string(),
                SchemaIr::Object {
                    properties: BTreeMap::from([
                        (
                            "v".to_string(),
                            SchemaIr::Int {
                                min: Some(1),
                                max: Some(1),
                            },
                        ),
                        (
                            "action".to_string(),
                            SchemaIr::String {
                                min_len: Some(4),
                                max_len: None,
                                regex: None,
                                format: None,
                            },
                        ),
                    ]),
                    required: vec!["v".to_string(), "action".to_string()],
                    additional: AdditionalProperties::Allow,
                },
            ),
            (
                "error".to_string(),
                SchemaIr::Object {
                    properties: BTreeMap::from([(
                        "code".to_string(),
                        SchemaIr::String {
                            min_len: Some(1),
                            max_len: None,
                            regex: None,
                            format: None,
                        },
                    )]),
                    required: vec!["code".to_string()],
                    additional: AdditionalProperties::Allow,
                },
            ),
        ]),
        required: vec![],
        additional: AdditionalProperties::Allow,
    }
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
fn config_schema() -> SchemaIr {
    SchemaIr::Object {
        properties: BTreeMap::new(),
        required: Vec::new(),
        additional: AdditionalProperties::Forbid,
    }
}

#[cfg(target_arch = "wasm32")]
fn i18n_keys() -> Vec<String> {
    vec![
        "component.display_name".to_string(),
        "operation.handle_message".to_string(),
        "qa.default.title".to_string(),
        "qa.default.description".to_string(),
        "qa.setup.title".to_string(),
        "qa.setup.description".to_string(),
        "qa.update.title".to_string(),
        "qa.update.description".to_string(),
        "qa.remove.title".to_string(),
        "qa.remove.description".to_string(),
    ]
}

#[cfg(target_arch = "wasm32")]
fn qa_spec_json(mode: &str) -> Value {
    let (mode, title_key, title_fallback) = match mode {
        "default" => ("default", "qa.default.title", "Default configuration"),
        "setup" => ("setup", "qa.setup.title", "Setup configuration"),
        "update" => ("update", "qa.update.title", "Update configuration"),
        "remove" => ("remove", "qa.remove.title", "Remove configuration"),
        _ => ("default", "qa.default.title", "Default configuration"),
    };
    serde_json::json!({
        "mode": mode,
        "title": {
            "key": title_key,
            "fallback": title_fallback,
        },
        "description": {
            "key": match mode {
                "default" => "qa.default.description",
                "setup" => "qa.setup.description",
                "update" => "qa.update.description",
                "remove" => "qa.remove.description",
                _ => "qa.default.description",
            },
            "fallback": match mode {
                "default" => "Review the default configuration for this component.",
                "setup" => "Provide any initial configuration values for this component.",
                "update" => "Adjust the existing configuration for this component.",
                "remove" => "Confirm whether this component configuration should be removed.",
                _ => "Review the default configuration for this component.",
            }
        },
        "questions": []
        ,
        "defaults": {}
    })
}

#[cfg(target_arch = "wasm32")]
fn apply_answers_cbor(current_config: Vec<u8>, answers: Vec<u8>) -> Vec<u8> {
    if !current_config.is_empty() {
        return current_config;
    }
    if !answers.is_empty() {
        return answers;
    }
    encode_cbor(&serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_payload_is_json() {
        let payload = describe_payload();
        let json: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(json["component"]["name"], COMPONENT_NAME);
    }

    #[test]
    fn merges_defaults_with_caller_priority() {
        let input = serde_json::json!({
            "target": { "flow": "flow-b" },
            "params": { "x": "default", "z": "keep" },
            "payload": { "x": "caller", "y": 2 },
            "hints": { "route": "default" },
            "routing_hints": { "route": "caller" }
        });

        let output = process_invocation("handle_message", &input);
        assert_eq!(output["greentic_control"]["v"], 1);
        assert_eq!(output["greentic_control"]["action"], "jump");
        assert_eq!(output["greentic_control"]["target"]["flow"], "flow-b");
        assert_eq!(output["greentic_control"]["target"]["node"], Value::Null);
        assert_eq!(output["greentic_control"]["params"]["x"], "caller");
        assert_eq!(output["greentic_control"]["params"]["z"], "keep");
        assert_eq!(output["greentic_control"]["hints"]["route"], "caller");
    }

    #[test]
    fn errors_when_target_flow_missing() {
        let input = serde_json::json!({ "target": {} });

        let output = process_invocation("handle_message", &input);
        assert_eq!(output["status"], "error");
        assert_eq!(output["error"]["code"], "missing_flow");
    }

    #[test]
    fn accepts_unknown_flow_for_runner_validation() {
        let input = serde_json::json!({
            "target": { "flow": "flow-x" }
        });

        let output = process_invocation("handle_message", &input);
        assert_eq!(output["greentic_control"]["action"], "jump");
        assert_eq!(output["greentic_control"]["target"]["flow"], "flow-x");
    }

    #[test]
    fn passes_max_redirects_to_runner() {
        let input = serde_json::json!({
            "target": { "flow": "flow-b" },
            "max_redirects": 3,
            "trace": { "redirect_count": 3 }
        });

        let output = process_invocation("handle_message", &input);
        assert_eq!(output["greentic_control"]["action"], "jump");
        assert_eq!(output["greentic_control"]["max_redirects"], 3);
    }

    #[test]
    fn does_not_depend_on_meta_redirect_count() {
        let input = serde_json::json!({
            "target": { "flow": "flow-b" },
            "meta": { "redirect_count": 2 }
        });

        let output = process_invocation("handle_message", &input);
        assert_eq!(output["greentic_control"]["action"], "jump");
    }

    #[test]
    fn empty_node_is_invalid() {
        let input = serde_json::json!({
            "target": { "flow": "flow-b", "node": "   " }
        });
        let output = process_invocation("handle_message", &input);
        assert_eq!(output["status"], "error");
        assert_eq!(output["error"]["code"], "invalid_input.node_empty");
    }

    #[test]
    fn invalid_node_syntax_is_rejected() {
        let input = serde_json::json!({
            "target": { "flow": "flow-b", "node": "node/1" }
        });
        let output = process_invocation("handle_message", &input);
        assert_eq!(output["status"], "error");
        assert_eq!(output["error"]["code"], "invalid_input.node_invalid");
    }

    #[test]
    fn invalid_flow_syntax_is_rejected() {
        let input = serde_json::json!({
            "target": { "flow": "flow/1" }
        });
        let output = process_invocation("handle_message", &input);
        assert_eq!(output["status"], "error");
        assert_eq!(output["error"]["code"], "invalid_input.flow_invalid");
    }

    #[test]
    fn handle_message_returns_error_for_non_json_input() {
        let output = handle_message("handle_message", "ping");
        let json: Value = serde_json::from_str(&output).expect("valid json");
        assert_eq!(json["status"], "error");
        assert_eq!(json["error"]["code"], "invalid_input");
    }
}
