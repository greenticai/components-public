#[cfg(target_arch = "wasm32")]
use std::collections::BTreeMap;

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;
#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::http_client_v1_1 as client;
#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::secrets_store;
#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::telemetry_logger as logger_api;
#[cfg(target_arch = "wasm32")]
use greentic_types::cbor::canonical;
#[cfg(target_arch = "wasm32")]
use greentic_types::i18n_text::I18nText;
#[cfg(target_arch = "wasm32")]
use greentic_types::schemas::common::schema_ir::{AdditionalProperties, SchemaIr};
#[cfg(target_arch = "wasm32")]
use greentic_types::schemas::component::v0_6_0::{ComponentQaSpec, QaMode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const COMPONENT_ID: &str = "ai.greentic.component-sorx-business";
const COMPONENT_NAME: &str = "component-sorx-business";
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const DEFAULT_OPERATION: &str = "invoke_locked_action";
const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const HASH_PREFIX: &str = "sha256:";

const OPERATIONS: &[&str] = &[
    "list_business_actions",
    "get_business_action_schema",
    "dry_run_locked_action",
    "invoke_locked_action",
    "explain_business_action_mapping",
];

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
            summary: Some("Generic Sorx business action client".to_string()),
            capabilities: vec![
                "host:http".to_string(),
                "host:secrets".to_string(),
                "host:telemetry".to_string(),
            ],
            ops: OPERATIONS
                .iter()
                .map(|name| node::Op {
                    name: (*name).to_string(),
                    summary: Some(format!("Sorx {name} operation")),
                    input: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(encode_cbor(&schema_for_op(name))),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    output: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(encode_cbor(&output_schema())),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    examples: Vec::new(),
                })
                .collect(),
            schemas: Vec::new(),
            setup: None,
        }
    }

    fn invoke(
        op: String,
        envelope: node::InvocationEnvelope,
    ) -> Result<node::InvocationResult, node::NodeError> {
        let input: Value = match canonical::from_cbor(&envelope.payload_cbor) {
            Ok(value) => value,
            Err(error) => {
                return Ok(node::InvocationResult {
                    ok: true,
                    output_cbor: encode_cbor(&error_response(
                        "invalid_input",
                        format!("CBOR payload decode failed: {error}"),
                    )),
                    output_metadata_cbor: None,
                });
            }
        };

        let output = execute_operation_with_sender(
            if op.is_empty() {
                DEFAULT_OPERATION
            } else {
                op.as_str()
            },
            &input,
            send_http,
        );
        Ok(node::InvocationResult {
            ok: true,
            output_cbor: encode_cbor(&output),
            output_metadata_cbor: None,
        })
    }
}

#[cfg(target_arch = "wasm32")]
greentic_interfaces_guest::export_component_v060!(Component);

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
            crate::encode_cbor(&crate::qa_spec(match mode {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SorxConfig {
    pub sorx_base_url: String,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u32,
    #[serde(default = "default_strict_tls")]
    pub strict_tls: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub kind: AuthKind,
    pub secret_ref: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            kind: AuthKind::None,
            secret_ref: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    #[default]
    None,
    BearerSecretRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SorxRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout_ms: u32,
    pub strict_tls: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SorxResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SorxHttpError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SorxError {
    InvalidInput { code: &'static str, message: String },
    Http(SorxHttpError),
}

impl From<SorxHttpError> for SorxError {
    fn from(value: SorxHttpError) -> Self {
        Self::Http(value)
    }
}

impl SorxError {
    fn code(&self) -> String {
        match self {
            Self::InvalidInput { code, .. } => (*code).to_string(),
            Self::Http(error) => error.code.clone(),
        }
    }

    fn message(&self) -> String {
        match self {
            Self::InvalidInput { message, .. } => message.clone(),
            Self::Http(error) => error.message.clone(),
        }
    }
}

fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

fn default_strict_tls() -> bool {
    true
}

pub fn describe_payload() -> String {
    json!({
        "component": {
            "name": COMPONENT_NAME,
            "id": COMPONENT_ID,
            "version": COMPONENT_VERSION,
            "world": "greentic:component/component-v0-v6-v0@0.6.0",
            "operations": OPERATIONS,
        },
        "schemas": {
            "input": operation_schema_json("invoke_locked_action"),
            "output": output_schema_json(),
            "config": config_schema_json(),
        }
    })
    .to_string()
}

pub fn handle_message(operation: &str, input: &str) -> String {
    let input = match serde_json::from_str::<Value>(input) {
        Ok(value) => value,
        Err(error) => {
            return error_response(
                "invalid_input",
                format!("Input must be valid JSON: {error}"),
            )
            .to_string();
        }
    };
    execute_operation_with_sender(operation, &input, send_http).to_string()
}

pub fn execute_operation(operation: &str, input: &Value) -> Value {
    execute_operation_with_sender(operation, input, send_http)
}

pub fn execute_operation_with_sender<F>(operation: &str, input: &Value, mut sender: F) -> Value
where
    F: FnMut(&SorxRequest) -> Result<SorxResponse, SorxHttpError>,
{
    match execute_operation_result(operation, input, &mut sender) {
        Ok(value) => value,
        Err(error) => error_response(error.code(), error.message()),
    }
}

fn execute_operation_result<F>(
    operation: &str,
    input: &Value,
    sender: &mut F,
) -> Result<Value, SorxError>
where
    F: FnMut(&SorxRequest) -> Result<SorxResponse, SorxHttpError>,
{
    match operation {
        "list_business_actions" => list_agent_endpoint_actions(input, sender),
        "get_business_action_schema" => get_agent_endpoint_schema(input, sender),
        "dry_run_locked_action" => {
            validate_action_input(input, true)?;
            validate_action_metadata(input)?;
            dry_run_agent_endpoint_action(input, sender)
        }
        "invoke_locked_action" => {
            validate_action_input(input, true)?;
            validate_action_metadata(input)?;
            if option_bool(input, "dry_run_first") {
                let dry_run_output = dry_run_agent_endpoint_action(input, sender)?;
                if !dry_run_output
                    .get("ok")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    return Ok(dry_run_output);
                }
            }
            invoke_agent_endpoint_action(input, sender)
        }
        "explain_business_action_mapping" => {
            validate_action_input(input, false)?;
            dry_run_agent_endpoint_action(input, sender)
        }
        other => Err(SorxError::InvalidInput {
            code: "unsupported_operation",
            message: format!("Unsupported operation: {other}"),
        }),
    }
}

pub fn build_request(operation: &str, input: &Value) -> Result<SorxRequest, SorxError> {
    let config = load_config(input)?;
    let (method, path, body) = match operation {
        "list_business_actions" => ("GET", "/v1/sorx/tools".to_string(), None),
        "get_business_action_schema" => {
            let _ = action_id_from_input(input)?;
            let _ = action_version_from_input(input)?;
            ("GET", "/v1/sorx/tools".to_string(), None)
        }
        "dry_run_locked_action" | "explain_business_action_mapping" => {
            let _ = parse_action_ref(input)?;
            ("GET", "/v1/sorx/tools".to_string(), None)
        }
        "invoke_locked_action" => {
            let _ = parse_action_ref(input)?;
            ("GET", "/v1/sorx/routes".to_string(), None)
        }
        other => {
            return Err(SorxError::InvalidInput {
                code: "unsupported_operation",
                message: format!("Unsupported operation: {other}"),
            });
        }
    };

    request_from_config(&config, method, &path, body)
}

fn request_from_config(
    config: &SorxConfig,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<SorxRequest, SorxError> {
    let mut headers = Vec::new();
    if body.is_some() {
        headers.push(("Content-Type".to_string(), "application/json".to_string()));
    }
    if let Some(token) = resolve_auth_token(config)? {
        headers.push(("Authorization".to_string(), format!("Bearer {token}")));
    }

    let body = body
        .map(|value| {
            serde_json::to_vec(&value).map_err(|error| SorxError::InvalidInput {
                code: "invalid_input",
                message: format!("Request body encoding failed: {error}"),
            })
        })
        .transpose()?;

    Ok(SorxRequest {
        method: method.to_string(),
        url: format!("{}{}", config.sorx_base_url.trim_end_matches('/'), path),
        headers,
        body,
        timeout_ms: config.timeout_ms,
        strict_tls: config.strict_tls,
    })
}

fn list_agent_endpoint_actions<F>(input: &Value, sender: &mut F) -> Result<Value, SorxError>
where
    F: FnMut(&SorxRequest) -> Result<SorxResponse, SorxHttpError>,
{
    let tools = send_json_request(sender, build_request("list_business_actions", input)?)?;
    if tools.status >= 400 {
        return Ok(normalize_error_status(tools.status, tools.body));
    }
    Ok(json!({
        "ok": true,
        "result": {
            "schema": "greentic.sorx.agent-endpoint-actions.v1",
            "actions": tools_as_actions(&tools.body)
        },
        "sorx": {
            "status": tools.status,
            "source": "/v1/sorx/tools"
        }
    }))
}

fn get_agent_endpoint_schema<F>(input: &Value, sender: &mut F) -> Result<Value, SorxError>
where
    F: FnMut(&SorxRequest) -> Result<SorxResponse, SorxHttpError>,
{
    let id = action_id_from_input(input)?;
    let tools = send_json_request(sender, build_request("get_business_action_schema", input)?)?;
    if tools.status >= 400 {
        return Ok(normalize_error_status(tools.status, tools.body));
    }
    let Some(tool) = find_tool(&tools.body, &id) else {
        return Ok(error_response(
            "unknown_action",
            format!("Sorx tool not found: {id}"),
        ));
    };
    Ok(json!({
        "ok": true,
        "result": {
            "schema": "greentic.sorx.agent-endpoint-action-schema.v1",
            "id": tool_id(tool).unwrap_or(id.as_str()),
            "version": action_version_from_input(input).unwrap_or_else(|_| "0.1.0".to_string()),
            "input_schema": tool.get("input_schema").cloned().unwrap_or_else(|| json!({ "type": "object" })),
            "output_schema": tool.get("output_schema").cloned().unwrap_or_else(|| json!({ "type": "object" }))
        },
        "sorx": {
            "status": tools.status,
            "source": "/v1/sorx/tools"
        }
    }))
}

fn dry_run_agent_endpoint_action<F>(input: &Value, sender: &mut F) -> Result<Value, SorxError>
where
    F: FnMut(&SorxRequest) -> Result<SorxResponse, SorxHttpError>,
{
    let parsed_ref = parse_action_ref(input)?;
    let tools = send_json_request(sender, build_request("dry_run_locked_action", input)?)?;
    if tools.status >= 400 {
        return Ok(normalize_error_status(tools.status, tools.body));
    }
    let Some(tool) = find_tool(&tools.body, &parsed_ref.id) else {
        return Ok(error_response(
            "unknown_action",
            format!("Sorx tool not found: {}", parsed_ref.id),
        ));
    };
    Ok(json!({
        "ok": true,
        "action_ref": action_ref(input).unwrap_or_else(|| json!({})),
        "result": {
            "valid": true,
            "canonical_payload": input.get("values").cloned().unwrap_or_else(|| json!({})),
            "execution_target": {
                "endpoint_id": tool.get("endpoint_id").cloned().unwrap_or_else(|| json!(parsed_ref.id)),
                "operation_id": tool.get("operation_id").cloned().unwrap_or_else(|| json!(parsed_ref.id)),
                "tool_name": tool.get("name").cloned().unwrap_or_else(|| json!(parsed_ref.id))
            }
        },
        "explain": {
            "source": "/v1/sorx/tools",
            "tool": tool
        },
        "sorx": {
            "status": tools.status,
            "source": "/v1/sorx/tools"
        }
    }))
}

fn invoke_agent_endpoint_action<F>(input: &Value, sender: &mut F) -> Result<Value, SorxError>
where
    F: FnMut(&SorxRequest) -> Result<SorxResponse, SorxHttpError>,
{
    let parsed_ref = parse_action_ref(input)?;
    let routes = send_json_request(sender, build_request("invoke_locked_action", input)?)?;
    if routes.status >= 400 {
        return Ok(normalize_error_status(routes.status, routes.body));
    }
    let Some(route) = find_route(&routes.body, &parsed_ref.id) else {
        return Ok(error_response(
            "unknown_action",
            format!("Sorx route not found for action: {}", parsed_ref.id),
        ));
    };
    let method = route
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("POST")
        .to_string();
    let path = route
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_code("invalid_route", "Sorx route is missing path"))?;
    if path.contains('{') {
        return Ok(error_response(
            "route_parameters_required",
            format!(
                "Sorx route path contains parameters and cannot be invoked generically: {path}"
            ),
        ));
    }
    let config = load_config(input)?;
    let body = if method == "GET" {
        None
    } else {
        Some(input.get("values").cloned().unwrap_or_else(|| json!({})))
    };
    let invoke = send_json_request(sender, request_from_config(&config, &method, path, body)?)?;
    normalize_response(
        SorxResponse {
            status: invoke.status,
            headers: Vec::new(),
            body: serde_json::to_vec(&invoke.body).unwrap_or_default(),
        },
        action_ref(input),
    )
}

struct JsonResponse {
    status: u16,
    body: Value,
}

fn send_json_request<F>(sender: &mut F, request: SorxRequest) -> Result<JsonResponse, SorxError>
where
    F: FnMut(&SorxRequest) -> Result<SorxResponse, SorxHttpError>,
{
    let response = sender(&request)?;
    Ok(JsonResponse {
        status: response.status,
        body: parse_body(&response.body),
    })
}

fn tools_as_actions(body: &Value) -> Value {
    let actions = body
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    let id = tool_id(tool)?;
                    Some(json!({
                        "id": id,
                        "version": "0.1.0",
                        "versions": ["0.1.0"],
                        "label": tool.get("name").cloned().unwrap_or_else(|| json!(id)),
                        "description": tool.get("description").cloned().unwrap_or(Value::Null),
                        "risk": tool.get("risk").cloned().unwrap_or(Value::Null),
                        "input_schema": tool.get("input_schema").cloned().unwrap_or_else(|| json!({ "type": "object" })),
                        "source": "sorx_tool"
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Value::Array(actions)
}

fn find_tool<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    body.get("tools")
        .and_then(Value::as_array)?
        .iter()
        .find(|tool| {
            tool.get("name").and_then(Value::as_str) == Some(id)
                || tool.get("endpoint_id").and_then(Value::as_str) == Some(id)
                || tool.get("operation_id").and_then(Value::as_str) == Some(id)
        })
}

fn find_route<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    body.get("routes")
        .and_then(Value::as_array)?
        .iter()
        .find(|route| {
            route.get("endpoint_id").and_then(Value::as_str) == Some(id)
                || route.get("operation_id").and_then(Value::as_str) == Some(id)
        })
}

fn tool_id(tool: &Value) -> Option<&str> {
    tool.get("endpoint_id")
        .and_then(Value::as_str)
        .or_else(|| tool.get("name").and_then(Value::as_str))
        .or_else(|| tool.get("operation_id").and_then(Value::as_str))
}

fn normalize_response(
    response: SorxResponse,
    action_ref: Option<Value>,
) -> Result<Value, SorxError> {
    let body = parse_body(&response.body);
    if response.status >= 400 {
        return Ok(normalize_error_status(response.status, body));
    }
    if is_drift_body(&body) {
        return Ok(contract_drift_response(None, Some(body)));
    }

    let mut output = Map::new();
    output.insert("ok".to_string(), Value::Bool(true));
    if let Some(action_ref) = action_ref {
        output.insert("action_ref".to_string(), action_ref);
    }
    output.insert(
        "result".to_string(),
        body.get("result").cloned().unwrap_or_else(|| body.clone()),
    );
    let mut sorx = Map::new();
    sorx.insert("status".to_string(), Value::from(response.status));
    for key in ["audit_event_id", "policy_decision", "approval_required"] {
        if let Some(value) = body.get(key) {
            sorx.insert(key.to_string(), value.clone());
        }
    }
    output.insert("sorx".to_string(), Value::Object(sorx));
    if let Some(explain) = body.get("explain").cloned() {
        output.insert("explain".to_string(), explain);
    }
    Ok(Value::Object(output))
}

fn normalize_error_status(status: u16, body: Value) -> Value {
    if is_drift_body(&body) {
        return contract_drift_response(None, Some(body));
    }
    let error_value = body.get("error").unwrap_or(&body);
    let code = error_value
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("sorx_error");
    let message = error_value
        .get("message")
        .or_else(|| error_value.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("Sorx request failed");
    json!({
        "ok": false,
        "error": {
            "code": code,
            "message": message,
        },
        "sorx": {
            "status": status,
            "details": body,
        }
    })
}

fn parse_body(body: &[u8]) -> Value {
    if body.is_empty() {
        return json!({});
    }
    serde_json::from_slice(body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(body).to_string()))
}

fn load_config(input: &Value) -> Result<SorxConfig, SorxError> {
    let Some(config) = input.get("config") else {
        return Err(SorxError::InvalidInput {
            code: "invalid_config",
            message: "missing config.sorx_base_url".to_string(),
        });
    };
    let mut config: SorxConfig =
        serde_json::from_value(config.clone()).map_err(|error| SorxError::InvalidInput {
            code: "invalid_config",
            message: format!("invalid config: {error}"),
        })?;
    config.sorx_base_url = config.sorx_base_url.trim_end_matches('/').to_string();
    if !config.sorx_base_url.starts_with("http://") && !config.sorx_base_url.starts_with("https://")
    {
        return Err(SorxError::InvalidInput {
            code: "invalid_config",
            message: "config.sorx_base_url must start with http:// or https://".to_string(),
        });
    }
    Ok(config)
}

fn resolve_auth_token(config: &SorxConfig) -> Result<Option<String>, SorxError> {
    match config.auth.kind {
        AuthKind::None => Ok(None),
        AuthKind::BearerSecretRef => {
            let secret_ref =
                config
                    .auth
                    .secret_ref
                    .as_deref()
                    .ok_or_else(|| SorxError::InvalidInput {
                        code: "invalid_config",
                        message: "auth.secret_ref is required for bearer_secret_ref".to_string(),
                    })?;
            resolve_secret(secret_ref).map(Some)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn resolve_secret(secret_ref: &str) -> Result<String, SorxError> {
    match secrets_store::get(secret_ref) {
        Ok(Some(bytes)) => String::from_utf8(bytes).map_err(|_| SorxError::InvalidInput {
            code: "invalid_config",
            message: format!("secret {secret_ref} is not valid utf-8"),
        }),
        Ok(None) => Err(SorxError::InvalidInput {
            code: "invalid_config",
            message: format!("secret not found: {secret_ref}"),
        }),
        Err(_) => Err(SorxError::InvalidInput {
            code: "invalid_config",
            message: format!("failed to read secret: {secret_ref}"),
        }),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_secret(secret_ref: &str) -> Result<String, SorxError> {
    std::env::var(secret_ref).map_err(|_| SorxError::InvalidInput {
        code: "invalid_config",
        message: format!("secret not found: {secret_ref}"),
    })
}

#[cfg(target_arch = "wasm32")]
fn send_http(request: &SorxRequest) -> Result<SorxResponse, SorxHttpError> {
    log_event("sorx_request");
    let req = client::Request {
        method: request.method.clone(),
        url: request.url.clone(),
        headers: request.headers.clone(),
        body: request.body.clone(),
    };
    let options = client::RequestOptions {
        timeout_ms: Some(request.timeout_ms),
        allow_insecure: Some(!request.strict_tls),
        follow_redirects: Some(true),
    };
    client::send(&req, Some(options), None)
        .map(|response| SorxResponse {
            status: response.status,
            headers: response.headers,
            body: response.body.unwrap_or_default(),
        })
        .map_err(|error| SorxHttpError {
            code: error.code,
            message: error.message,
        })
}

#[cfg(not(target_arch = "wasm32"))]
fn send_http(_request: &SorxRequest) -> Result<SorxResponse, SorxHttpError> {
    Err(SorxHttpError {
        code: "not_implemented".to_string(),
        message: "HTTP is not available in native builds".to_string(),
    })
}

#[cfg(target_arch = "wasm32")]
fn log_event(event: &str) {
    let span = logger_api::SpanContext {
        tenant: "tenant".into(),
        session_id: None,
        flow_id: "component-sorx-business".into(),
        node_id: None,
        provider: "sorx".into(),
        start_ms: None,
        end_ms: None,
    };
    let fields = [("event".to_string(), event.to_string())];
    let _ = logger_api::log(&span, &fields, None);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ActionRef {
    id: String,
    version: String,
    contract_hash: String,
}

fn validate_action_input(input: &Value, validate_options: bool) -> Result<(), SorxError> {
    let root = input
        .as_object()
        .ok_or_else(|| invalid("Input must be an object"))?;
    parse_action_ref(input)?;
    if !root.get("values").is_some_and(Value::is_object) {
        return Err(invalid_code("missing_values", "values must be an object"));
    }
    if validate_options {
        validate_options_shape(input)?;
    }
    Ok(())
}

fn parse_action_ref(input: &Value) -> Result<ActionRef, SorxError> {
    let action_ref = input
        .get("action_ref")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_code("missing_action_ref", "action_ref must be an object"))?;
    let id = required_non_empty_string(action_ref, "id", "missing_action_id")?;
    let version = required_non_empty_string(action_ref, "version", "missing_action_version")?;
    let contract_hash =
        required_non_empty_string(action_ref, "contract_hash", "missing_contract_hash")?;
    if !valid_contract_hash(&contract_hash) {
        return Err(invalid_code(
            "invalid_contract_hash",
            "action_ref.contract_hash must match sha256:<64 lowercase hex chars>",
        ));
    }
    if !valid_path_segment(&id) {
        return Err(invalid_code(
            "invalid_action_id",
            "action_ref.id must not contain path separators or unsafe URL characters",
        ));
    }
    if !valid_path_segment(&version) {
        return Err(invalid_code(
            "invalid_action_version",
            "action_ref.version must not contain path separators or unsafe URL characters",
        ));
    }
    Ok(ActionRef {
        id,
        version,
        contract_hash,
    })
}

fn action_ref(input: &Value) -> Option<Value> {
    input.get("action_ref").cloned()
}

fn action_id_from_input(input: &Value) -> Result<String, SorxError> {
    let id = input
        .get("action_id")
        .and_then(Value::as_str)
        .or_else(|| input.pointer("/action_ref/id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_code("missing_action_id", "action_id is required"))?;
    if !valid_path_segment(id) {
        return Err(invalid_code(
            "invalid_action_id",
            "action_id must not contain path separators or unsafe URL characters",
        ));
    }
    Ok(id.to_string())
}

fn action_version_from_input(input: &Value) -> Result<String, SorxError> {
    let version = input
        .get("action_version")
        .and_then(Value::as_str)
        .or_else(|| input.get("version").and_then(Value::as_str))
        .or_else(|| input.pointer("/action_ref/version").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_code("missing_action_version", "action_version is required"))?;
    if !valid_path_segment(version) {
        return Err(invalid_code(
            "invalid_action_version",
            "action_version must not contain path separators or unsafe URL characters",
        ));
    }
    Ok(version.to_string())
}

fn required_non_empty_string(
    object: &Map<String, Value>,
    key: &str,
    code: &'static str,
) -> Result<String, SorxError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| invalid_code(code, format!("action_ref.{key} is required")))
}

fn valid_contract_hash(value: &str) -> bool {
    let Some(rest) = value.strip_prefix(HASH_PREFIX) else {
        return false;
    };
    rest.len() == 64
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_path_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn validate_options_shape(input: &Value) -> Result<(), SorxError> {
    let Some(options) = input.get("options") else {
        return Ok(());
    };
    let options = options
        .as_object()
        .ok_or_else(|| invalid_code("invalid_options", "options must be an object"))?;
    for key in options.keys() {
        if !matches!(
            key.as_str(),
            "idempotency_key" | "require_explanation" | "dry_run_first" | "fail_on_warning"
        ) {
            return Err(invalid_code(
                "invalid_options",
                format!("unknown options field: {key}"),
            ));
        }
    }
    Ok(())
}

fn option_bool(input: &Value, key: &str) -> bool {
    input
        .pointer(&format!("/options/{key}"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn validate_action_metadata(input: &Value) -> Result<(), SorxError> {
    let Some(metadata) = input
        .get("action_metadata")
        .or_else(|| input.get("_action_metadata"))
    else {
        return Ok(());
    };
    let action_ref = parse_action_ref(input)?;
    let metadata_hash = metadata
        .get("contract_hash")
        .or_else(|| metadata.get("hash"))
        .and_then(Value::as_str);
    if let Some(metadata_hash) = metadata_hash
        && metadata_hash != action_ref.contract_hash
    {
        return Err(SorxError::InvalidInput {
            code: "action_contract_drift",
            message: "The action contract changed. Revalidate the flow node.".to_string(),
        });
    }
    if metadata
        .get("idempotency_required")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && input
            .pointer("/options/idempotency_key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(invalid_code(
            "idempotency_required",
            "options.idempotency_key is required for this action",
        ));
    }
    if let Some(schema) = metadata
        .get("input_schema")
        .or_else(|| metadata.get("values_schema"))
    {
        validate_json_schema_subset(
            input.get("values").unwrap_or(&Value::Null),
            schema,
            "values",
        )?;
    }
    Ok(())
}

fn validate_json_schema_subset(value: &Value, schema: &Value, path: &str) -> Result<(), SorxError> {
    if let Some(type_name) = schema.get("type").and_then(Value::as_str) {
        let valid = match type_name {
            "object" => value.is_object(),
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            _ => true,
        };
        if !valid {
            return Err(invalid_code(
                "schema_validation_failed",
                format!("{path} must be {type_name}"),
            ));
        }
    }

    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        let object = value.as_object().ok_or_else(|| {
            invalid_code("schema_validation_failed", format!("{path} must be object"))
        })?;
        for field in required {
            let Some(field) = field.as_str() else {
                continue;
            };
            if !object.contains_key(field) {
                return Err(invalid_code(
                    "schema_validation_failed",
                    format!("{path}.{field} is required"),
                ));
            }
        }
    }

    if schema
        .get("additionalProperties")
        .and_then(Value::as_bool)
        .is_some_and(|allow| !allow)
        && let Some(object) = value.as_object()
    {
        let allowed = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for key in object.keys() {
            if !allowed.contains_key(key) {
                return Err(invalid_code(
                    "schema_validation_failed",
                    format!("{path}.{key} is not allowed"),
                ));
            }
        }
    }

    if let (Some(properties), Some(object)) = (
        schema.get("properties").and_then(Value::as_object),
        value.as_object(),
    ) {
        for (key, subschema) in properties {
            if let Some(subvalue) = object.get(key) {
                validate_json_schema_subset(subvalue, subschema, &format!("{path}.{key}"))?;
            }
        }
    }

    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, item) in array.iter().enumerate() {
            validate_json_schema_subset(item, items, &format!("{path}[{index}]"))?;
        }
    }
    Ok(())
}

fn is_drift_body(body: &Value) -> bool {
    let code = body
        .pointer("/error/code")
        .or_else(|| body.get("code"))
        .and_then(Value::as_str);
    matches!(
        code,
        Some("contract_hash_mismatch" | "version_mismatch" | "action_contract_drift")
    )
}

fn contract_drift_response(expected: Option<Value>, actual: Option<Value>) -> Value {
    json!({
        "ok": false,
        "error": {
            "code": "action_contract_drift",
            "message": "The action contract changed. Revalidate the flow node."
        },
        "expected": expected.unwrap_or_else(|| json!({})),
        "actual": actual.unwrap_or_else(|| json!({})),
    })
}

fn error_response(code: impl Into<String>, message: impl Into<String>) -> Value {
    let code = code.into();
    if code == "action_contract_drift" {
        return contract_drift_response(None, None);
    }
    json!({
        "ok": false,
        "error": {
            "code": code,
            "message": message.into(),
        }
    })
}

fn invalid(message: impl Into<String>) -> SorxError {
    invalid_code("invalid_input", message)
}

fn invalid_code(code: &'static str, message: impl Into<String>) -> SorxError {
    SorxError::InvalidInput {
        code,
        message: message.into(),
    }
}

pub fn operation_schema_json(operation: &str) -> Value {
    match operation {
        "list_business_actions" => json!({"type": "object", "additionalProperties": true}),
        "get_business_action_schema" => json!({
            "type": "object",
            "required": ["action_id", "action_version"],
            "properties": {
                "action_id": { "type": "string", "minLength": 1 },
                "action_version": { "type": "string", "minLength": 1 },
                "config": { "type": "object", "additionalProperties": true }
            },
            "additionalProperties": true
        }),
        _ => action_input_schema_json(),
    }
}

pub fn action_input_schema_json() -> Value {
    json!({
        "type": "object",
        "required": ["action_ref", "values"],
        "properties": {
            "action_ref": {
                "type": "object",
                "required": ["id", "version", "contract_hash"],
                "properties": {
                    "id": { "type": "string", "minLength": 1 },
                    "version": { "type": "string", "minLength": 1 },
                    "contract_hash": {
                        "type": "string",
                        "pattern": "^sha256:[a-f0-9]{64}$"
                    }
                },
                "additionalProperties": false
            },
            "values": { "type": "object", "additionalProperties": true },
            "options": {
                "type": "object",
                "properties": {
                    "idempotency_key": { "type": "string" },
                    "require_explanation": { "type": "boolean", "default": true },
                    "dry_run_first": { "type": "boolean", "default": false },
                    "fail_on_warning": { "type": "boolean", "default": false }
                },
                "additionalProperties": false
            },
            "action_metadata": { "type": "object", "additionalProperties": true },
            "config": { "type": "object", "additionalProperties": true }
        },
        "additionalProperties": false
    })
}

pub fn output_schema_json() -> Value {
    json!({
        "type": "object",
        "required": ["ok"],
        "properties": {
            "ok": { "type": "boolean" },
            "action_ref": { "type": "object", "additionalProperties": true },
            "result": {},
            "sorx": { "type": "object", "additionalProperties": true },
            "explain": { "type": "object", "additionalProperties": true },
            "error": {
                "type": "object",
                "properties": {
                    "code": { "type": "string" },
                    "message": { "type": "string" }
                },
                "additionalProperties": true
            }
        },
        "additionalProperties": true
    })
}

pub fn config_schema_json() -> Value {
    json!({
        "type": "object",
        "required": ["sorx_base_url"],
        "properties": {
            "sorx_base_url": { "type": "string", "format": "uri", "title": "Sorx backend URL" },
            "auth": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["none", "bearer_secret_ref"] },
                    "secret_ref": { "type": "string" }
                },
                "additionalProperties": false
            },
            "timeout_ms": { "type": "integer", "minimum": 1, "default": 30000 },
            "strict_tls": { "type": "boolean", "default": true }
        },
        "additionalProperties": false
    })
}

#[cfg(target_arch = "wasm32")]
fn schema_for_op(operation: &str) -> SchemaIr {
    match operation {
        "list_business_actions" => SchemaIr::Object {
            properties: BTreeMap::new(),
            required: Vec::new(),
            additional: AdditionalProperties::Allow,
        },
        "get_business_action_schema" => SchemaIr::Object {
            properties: BTreeMap::from([
                ("action_id".to_string(), string_schema()),
                ("action_version".to_string(), string_schema()),
            ]),
            required: vec!["action_id".to_string(), "action_version".to_string()],
            additional: AdditionalProperties::Allow,
        },
        _ => action_schema_ir(),
    }
}

#[cfg(target_arch = "wasm32")]
fn action_schema_ir() -> SchemaIr {
    SchemaIr::Object {
        properties: BTreeMap::from([
            (
                "action_ref".to_string(),
                SchemaIr::Object {
                    properties: BTreeMap::from([
                        ("id".to_string(), string_schema()),
                        ("version".to_string(), string_schema()),
                        ("contract_hash".to_string(), string_schema()),
                    ]),
                    required: vec![
                        "id".to_string(),
                        "version".to_string(),
                        "contract_hash".to_string(),
                    ],
                    additional: AdditionalProperties::Forbid,
                },
            ),
            (
                "values".to_string(),
                SchemaIr::Object {
                    properties: BTreeMap::new(),
                    required: Vec::new(),
                    additional: AdditionalProperties::Allow,
                },
            ),
        ]),
        required: vec!["action_ref".to_string(), "values".to_string()],
        additional: AdditionalProperties::Forbid,
    }
}

#[cfg(target_arch = "wasm32")]
fn output_schema() -> SchemaIr {
    SchemaIr::Object {
        properties: BTreeMap::from([
            ("ok".to_string(), SchemaIr::Bool),
            (
                "error".to_string(),
                SchemaIr::Object {
                    properties: BTreeMap::from([("code".to_string(), string_schema())]),
                    required: Vec::new(),
                    additional: AdditionalProperties::Allow,
                },
            ),
        ]),
        required: vec!["ok".to_string()],
        additional: AdditionalProperties::Allow,
    }
}

#[cfg(target_arch = "wasm32")]
fn string_schema() -> SchemaIr {
    SchemaIr::String {
        min_len: Some(1),
        max_len: None,
        regex: None,
        format: None,
    }
}

#[cfg(target_arch = "wasm32")]
fn encode_cbor<T: serde::Serialize>(value: &T) -> Vec<u8> {
    canonical::to_canonical_cbor_allow_floats(value).expect("encode cbor")
}

#[cfg(target_arch = "wasm32")]
fn i18n_keys() -> Vec<String> {
    vec![
        "component.display_name".to_string(),
        "operation.list_business_actions".to_string(),
        "operation.get_business_action_schema".to_string(),
        "operation.dry_run_locked_action".to_string(),
        "operation.invoke_locked_action".to_string(),
        "operation.explain_business_action_mapping".to_string(),
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
fn qa_spec(mode: &str) -> ComponentQaSpec {
    let (mode, title_key, title_fallback) = match mode {
        "setup" => (QaMode::Setup, "qa.setup.title", "Setup configuration"),
        "update" => (QaMode::Update, "qa.update.title", "Update configuration"),
        "remove" => (QaMode::Remove, "qa.remove.title", "Remove configuration"),
        _ => (QaMode::Default, "qa.default.title", "Default configuration"),
    };
    let description_key = match mode {
        QaMode::Default => "qa.default.description",
        QaMode::Setup => "qa.setup.description",
        QaMode::Update => "qa.update.description",
        QaMode::Remove => "qa.remove.description",
    };
    let description_fallback = match mode {
        QaMode::Default => "Review the default configuration for this component.",
        QaMode::Setup => "Provide the Sorx endpoint and authentication settings.",
        QaMode::Update => "Adjust Sorx endpoint or authentication settings.",
        QaMode::Remove => "Confirm whether this component configuration should be removed.",
    };

    ComponentQaSpec {
        mode,
        title: I18nText::new(title_key, Some(title_fallback.to_string())),
        description: Some(I18nText::new(
            description_key,
            Some(description_fallback.to_string()),
        )),
        questions: Vec::new(),
        defaults: BTreeMap::new(),
    }
}

#[cfg(target_arch = "wasm32")]
fn apply_answers_cbor(current_config: Vec<u8>, answers: Vec<u8>) -> Vec<u8> {
    if !current_config.is_empty() {
        return current_config;
    }
    if !answers.is_empty() {
        return answers;
    }
    encode_cbor(&json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const HASH: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn config() -> Value {
        json!({
            "sorx_base_url": "https://sorx.example.test",
            "auth": { "kind": "none" },
            "timeout_ms": 1234,
            "strict_tls": true
        })
    }

    fn action_input() -> Value {
        json!({
            "config": config(),
            "action_ref": {
                "id": "record.payment",
                "version": "0.1.0",
                "contract_hash": HASH
            },
            "values": {},
            "options": { "require_explanation": true }
        })
    }

    fn response(body: Value) -> SorxResponse {
        SorxResponse {
            status: 200,
            headers: Vec::new(),
            body: serde_json::to_vec(&body).expect("body"),
        }
    }

    fn tools_response() -> SorxResponse {
        response(json!({
            "schema": "greentic.sorx.tools.v1",
            "tools": [{
                "name": "record.payment",
                "endpoint_id": "record.payment",
                "operation_id": "record.payment",
                "description": "Record a payment",
                "risk": "low",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "amount": { "type": "integer" }
                    },
                    "required": ["amount"]
                }
            }]
        }))
    }

    fn routes_response() -> SorxResponse {
        response(json!({
            "schema": "greentic.sorx.routes.v1",
            "routes": [{
                "endpoint_id": "record.payment",
                "operation_id": "record.payment",
                "method": "POST",
                "path": "/v1/agent/payments/create"
            }]
        }))
    }

    #[test]
    fn manifest_includes_all_operations() {
        let manifest: Value = serde_json::from_str(include_str!("../component.manifest.json"))
            .expect("manifest json");
        let ops = manifest["operations"].as_array().expect("operations");
        for expected in OPERATIONS {
            assert!(ops.iter().any(|op| op["name"] == *expected), "{expected}");
        }
    }

    #[test]
    fn describe_payload_includes_all_operations() {
        let payload: Value = serde_json::from_str(&describe_payload()).expect("describe json");
        for expected in OPERATIONS {
            assert!(
                payload["component"]["operations"]
                    .as_array()
                    .expect("operations")
                    .iter()
                    .any(|op| op == expected),
                "{expected}"
            );
        }
    }

    #[test]
    fn action_schema_has_action_ref_values_options() {
        let schema = action_input_schema_json();
        let properties = schema["properties"].as_object().expect("properties");
        assert!(properties.contains_key("action_ref"));
        assert!(properties.contains_key("values"));
        assert!(properties.contains_key("options"));
        assert_eq!(
            properties["action_ref"]["properties"]["contract_hash"]["pattern"],
            "^sha256:[a-f0-9]{64}$"
        );
    }

    #[test]
    fn list_maps_to_expected_endpoint() {
        let request = build_request("list_business_actions", &json!({ "config": config() }))
            .expect("request");
        assert_eq!(request.method, "GET");
        assert_eq!(request.url, "https://sorx.example.test/v1/sorx/tools");
    }

    #[test]
    fn list_normalizes_sorx_tools_as_actions() {
        let output = execute_operation_with_sender(
            "list_business_actions",
            &json!({ "config": config() }),
            |_| Ok(tools_response()),
        );
        assert_eq!(output["ok"], true);
        assert_eq!(output["result"]["actions"][0]["id"], "record.payment");
        assert_eq!(output["sorx"]["source"], "/v1/sorx/tools");
    }

    #[test]
    fn get_schema_maps_to_expected_endpoint() {
        let request = build_request(
            "get_business_action_schema",
            &json!({
                "config": config(),
                "action_id": "record.payment",
                "action_version": "0.1.0"
            }),
        )
        .expect("request");
        assert_eq!(request.method, "GET");
        assert_eq!(request.url, "https://sorx.example.test/v1/sorx/tools");
    }

    #[test]
    fn unsafe_action_id_is_rejected() {
        let output = build_request(
            "get_business_action_schema",
            &json!({
                "config": config(),
                "action_id": "../bad",
                "action_version": "0.1.0"
            }),
        )
        .expect_err("unsafe id");
        assert_eq!(output.code(), "invalid_action_id");
    }

    #[test]
    fn unsafe_action_version_is_rejected() {
        let output = build_request(
            "get_business_action_schema",
            &json!({
                "config": config(),
                "action_id": "record.payment",
                "action_version": "../bad"
            }),
        )
        .expect_err("unsafe version");
        assert_eq!(output.code(), "invalid_action_version");
    }

    #[test]
    fn dry_run_and_invoke_map_to_expected_endpoints() {
        let input = action_input();
        let dry_run = build_request("dry_run_locked_action", &input).expect("dry run request");
        assert_eq!(dry_run.method, "GET");
        assert!(dry_run.url.ends_with("/v1/sorx/tools"));
        let invoke = build_request("invoke_locked_action", &input).expect("invoke request");
        assert_eq!(invoke.method, "GET");
        assert!(invoke.url.ends_with("/v1/sorx/routes"));
    }

    #[test]
    fn explain_maps_to_dry_run_endpoint() {
        let explain =
            build_request("explain_business_action_mapping", &action_input()).expect("explain");
        assert!(explain.url.ends_with("/v1/sorx/tools"));
        assert_eq!(explain.method, "GET");
    }

    #[test]
    fn bearer_auth_header_can_be_added_from_env_secret() {
        unsafe {
            std::env::set_var("SORX_TEST_TOKEN", "abc");
        }
        let request = build_request(
            "list_business_actions",
            &json!({
                "config": {
                    "sorx_base_url": "https://sorx.example.test",
                    "auth": { "kind": "bearer_secret_ref", "secret_ref": "SORX_TEST_TOKEN" }
                }
            }),
        )
        .expect("request");
        assert!(
            request
                .headers
                .iter()
                .any(|(name, value)| { name == "Authorization" && value == "Bearer abc" })
        );
        unsafe {
            std::env::remove_var("SORX_TEST_TOKEN");
        }
    }

    #[test]
    fn invalid_hash_rejected_before_request() {
        let mut input = action_input();
        input["action_ref"]["contract_hash"] = Value::String("sha256:not-a-hash".to_string());
        let output = execute_operation_with_sender("invoke_locked_action", &input, |_| {
            panic!("request should not be sent")
        });
        assert_eq!(output["ok"], false);
        assert_eq!(output["error"]["code"], "invalid_contract_hash");
    }

    #[test]
    fn missing_action_ref_fields_are_rejected() {
        for (field, code) in [
            ("id", "missing_action_id"),
            ("version", "missing_action_version"),
            ("contract_hash", "missing_contract_hash"),
        ] {
            let mut input = action_input();
            input["action_ref"].as_object_mut().unwrap().remove(field);
            let output = execute_operation_with_sender("invoke_locked_action", &input, |_| {
                panic!("request should not be sent")
            });
            assert_eq!(output["error"]["code"], code);
        }
    }

    #[test]
    fn local_metadata_hash_mismatch_is_contract_drift() {
        let mut input = action_input();
        input["action_metadata"] = json!({
            "contract_hash": "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        });
        let output = execute_operation_with_sender("invoke_locked_action", &input, |_| {
            panic!("request should not be sent")
        });
        assert_eq!(output["error"]["code"], "action_contract_drift");
    }

    #[test]
    fn schema_validation_rejects_missing_and_unknown_fields() {
        let mut input = action_input();
        input["action_metadata"] = json!({
            "contract_hash": HASH,
            "input_schema": {
                "type": "object",
                "required": ["amount"],
                "properties": {
                    "amount": { "type": "integer" }
                },
                "additionalProperties": false
            }
        });
        let output = execute_operation_with_sender("invoke_locked_action", &input, |_| {
            panic!("request should not be sent")
        });
        assert_eq!(output["error"]["code"], "schema_validation_failed");

        input["values"] = json!({ "amount": 1, "extra": true });
        let output = execute_operation_with_sender("invoke_locked_action", &input, |_| {
            panic!("request should not be sent")
        });
        assert_eq!(output["error"]["code"], "schema_validation_failed");
    }

    #[test]
    fn idempotency_required_is_enforced() {
        let mut input = action_input();
        input["action_metadata"] = json!({
            "contract_hash": HASH,
            "idempotency_required": true
        });
        let output = execute_operation_with_sender("invoke_locked_action", &input, |_| {
            panic!("request should not be sent")
        });
        assert_eq!(output["error"]["code"], "idempotency_required");
    }

    #[test]
    fn dry_run_first_failure_prevents_invoke() {
        let mut input = action_input();
        input["options"]["dry_run_first"] = Value::Bool(true);
        let calls = RefCell::new(Vec::new());
        let output = execute_operation_with_sender("invoke_locked_action", &input, |request| {
            calls.borrow_mut().push(request.url.clone());
            Ok(response(
                json!({ "schema": "greentic.sorx.tools.v1", "tools": [] }),
            ))
        });
        assert_eq!(output["ok"], false);
        assert_eq!(output["error"]["code"], "unknown_action");
        assert_eq!(calls.borrow().len(), 1);
        assert!(calls.borrow()[0].ends_with("/v1/sorx/tools"));
    }

    #[test]
    fn sorx_success_and_error_are_normalized() {
        let calls = RefCell::new(0);
        let ok =
            execute_operation_with_sender("invoke_locked_action", &action_input(), |request| {
                *calls.borrow_mut() += 1;
                if request.url.ends_with("/v1/sorx/routes") {
                    return Ok(routes_response());
                }
                assert_eq!(
                    request.url,
                    "https://sorx.example.test/v1/agent/payments/create"
                );
                Ok(response(json!({
                    "ok": true,
                    "result": { "id": "result_1" },
                })))
            });
        assert_eq!(ok["ok"], true);
        assert_eq!(ok["result"]["id"], "result_1");
        assert_eq!(*calls.borrow(), 2);

        let err =
            execute_operation_with_sender("invoke_locked_action", &action_input(), |request| {
                if request.url.ends_with("/v1/sorx/routes") {
                    return Ok(routes_response());
                }
                Ok(SorxResponse {
                    status: 403,
                    headers: Vec::new(),
                    body: br#"{"error":{"code":"policy_denied","message":"no"}}"#.to_vec(),
                })
            });
        assert_eq!(err["ok"], false);
        assert_eq!(err["error"]["code"], "policy_denied");
        assert_eq!(err["sorx"]["status"], 403);
    }

    #[test]
    fn sorx_hash_mismatch_is_normalized_as_drift() {
        let output =
            execute_operation_with_sender("invoke_locked_action", &action_input(), |request| {
                if request.url.ends_with("/v1/sorx/routes") {
                    return Ok(routes_response());
                }
                Ok(SorxResponse {
                    status: 409,
                    headers: Vec::new(),
                    body: br#"{"error":{"code":"contract_hash_mismatch","message":"changed"}}"#
                        .to_vec(),
                })
            });
        assert_eq!(output["error"]["code"], "action_contract_drift");
    }

    #[test]
    fn component_schemas_have_no_domain_specific_fields() {
        let all = json!({
            "action": action_input_schema_json(),
            "config": config_schema_json(),
        })
        .to_string();
        for banned in [
            "tenant", "flat", "landlord", "claim", "order", "invoice", "plumber",
        ] {
            assert!(!all.contains(banned), "{banned}");
        }
    }
}
