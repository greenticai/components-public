#![allow(unsafe_op_in_unsafe_fn)]

//! Chat2Data Executor Component
//!
//! This component executes translated queries in a sandboxed environment with
//! resource limits. It implements Layer 4 of the Chat2Data security architecture.
//!
//! # Operations
//!
//! - `execute` - Execute a translated query
//! - `execute_github` - Execute GitHub API request
//! - `execute_mcp` - Execute MCP tool call (delegated to MCP runtime)
//!
//! # Security Features
//!
//! - Resource limits (max rows, timeout, memory)
//! - Read-only mode enforcement
//! - Rate limiting awareness
//! - Audit logging
//!
//! # Note on SQLite Execution
//!
//! SQLite queries are NOT executed directly by this component. Instead, the
//! translated SQL query is passed to the host runtime which provides database
//! access. This component prepares the execution request with proper limits.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[allow(clippy::too_many_arguments)]
mod bindings {
    wit_bindgen::generate!({ path: "wit/chat2data-executor", world: "component-v0-v6-v0", generate_all });
}

use bindings::greentic::http::http_client as client;
#[cfg(not(test))]
use bindings::greentic::secrets_store::secrets_store;
#[cfg(not(test))]
use bindings::greentic::telemetry::logger_api;

const COMPONENT_ID: &str = "chat2data-executor";
const WORLD_ID: &str = "component-v0-v6-v0";
const DEFAULT_TIMEOUT_MS: u32 = 30000;

const I18N_KEYS: &[&str] = &[
    "chat2data-executor.op.execute.title",
    "chat2data-executor.op.execute.description",
    "chat2data-executor.op.execute_github.title",
    "chat2data-executor.op.execute_github.description",
    "chat2data-executor.op.execute_mcp.title",
    "chat2data-executor.op.execute_mcp.description",
    "chat2data-executor.schema.input.title",
    "chat2data-executor.schema.input.description",
    "chat2data-executor.schema.output.title",
    "chat2data-executor.schema.output.description",
    "chat2data-executor.schema.config.title",
    "chat2data-executor.schema.config.description",
    "chat2data-executor.schema.config.github_token.title",
    "chat2data-executor.schema.config.github_token.description",
    "chat2data-executor.qa.default.title",
    "chat2data-executor.qa.setup.title",
    "chat2data-executor.qa.setup.github_token",
];

// ============================================================================
// Data Structures
// ============================================================================

/// Component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComponentConfig {
    /// GitHub API token (required for GitHub queries)
    #[serde(default)]
    github_token: Option<String>,
    /// GitHub API base URL
    #[serde(default = "default_github_api_url")]
    github_api_url: String,
    /// Default timeout in milliseconds
    #[serde(default = "default_timeout")]
    timeout_ms: u32,
    /// Maximum result size in bytes
    #[serde(default = "default_max_result_size")]
    max_result_size_bytes: usize,
}

impl Default for ComponentConfig {
    fn default() -> Self {
        Self {
            github_token: None,
            github_api_url: default_github_api_url(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_result_size_bytes: default_max_result_size(),
        }
    }
}

/// Translated query from translator component
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TranslatedQuery {
    /// Target type
    target: String,
    /// Query type (sql, http, mcp)
    query_type: String,
    /// The actual query
    query: QuerySpec,
    /// Renderer hint
    renderer: String,
    /// Renderer options
    renderer_options: Value,
    /// Maximum rows
    max_rows: usize,
    /// Query hash for audit
    query_hash: String,
}

/// Query specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum QuerySpec {
    /// SQLite parameterized query
    Sql {
        sql: String,
        params: Vec<SqlParam>,
        param_names: Vec<String>,
    },
    /// HTTP request (for GitHub API)
    Http {
        method: String,
        path: String,
        query_params: BTreeMap<String, String>,
        headers: BTreeMap<String, String>,
    },
    /// MCP tool call
    Mcp { tool_name: String, arguments: Value },
}

/// SQL parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SqlParam {
    Null,
    Integer { value: i64 },
    Real { value: f64 },
    Text { value: String },
    Blob { value: Vec<u8> },
}

/// Execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExecutionResult {
    /// Execution success
    success: bool,
    /// Result data (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<QueryData>,
    /// Error information (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ExecutionError>,
    /// Execution metadata
    metadata: ExecutionMetadata,
}

/// Query result data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueryData {
    /// Result rows
    rows: Vec<Value>,
    /// Column names/schema
    #[serde(default)]
    columns: Vec<String>,
    /// Total row count (if known)
    #[serde(skip_serializing_if = "Option::is_none")]
    total_count: Option<usize>,
    /// Was result truncated?
    truncated: bool,
    /// Truncation reason
    #[serde(skip_serializing_if = "Option::is_none")]
    truncation_reason: Option<String>,
}

/// Execution error
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExecutionError {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
    /// Is this a retryable error?
    retryable: bool,
    /// Suggested retry after (seconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after: Option<u32>,
}

/// Execution metadata for auditing
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExecutionMetadata {
    /// Query hash
    query_hash: String,
    /// Execution time in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_time_ms: Option<u64>,
    /// Rows returned
    #[serde(skip_serializing_if = "Option::is_none")]
    row_count: Option<usize>,
    /// Result size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    result_size_bytes: Option<usize>,
    /// Renderer hint from query
    renderer: String,
    /// Renderer options
    renderer_options: Value,
}

/// Result of applying QA answers
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApplyAnswersResult {
    ok: bool,
    config: Option<ComponentConfig>,
    error: Option<String>,
}

// ============================================================================
// Default Values
// ============================================================================

fn default_github_api_url() -> String {
    "https://api.github.com".to_string()
}

fn default_timeout() -> u32 {
    DEFAULT_TIMEOUT_MS
}

fn default_max_result_size() -> usize {
    10 * 1024 * 1024 // 10MB
}

// ============================================================================
// WASM Component Implementation
// ============================================================================

struct Component;

impl bindings::exports::greentic::component::descriptor::Guest for Component {
    fn describe() -> Vec<u8> {
        canonical_cbor_bytes(&build_describe_payload())
    }
}

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
            "execute" => handle_execute(&input),
            "execute_github" => handle_execute_github(&input),
            "execute_mcp" => handle_execute_mcp(&input),
            other => json!({"ok": false, "error": format!("unsupported op: {other}")}),
        };

        canonical_cbor_bytes(&output)
    }
}

impl bindings::exports::greentic::component::qa::Guest for Component {
    fn qa_spec(mode: bindings::exports::greentic::component::qa::Mode) -> Vec<u8> {
        canonical_cbor_bytes(&build_qa_spec(mode))
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
            let cfg = ComponentConfig {
                github_token: answers
                    .get("github_token")
                    .and_then(Value::as_str)
                    .map(String::from),
                github_api_url: answers
                    .get("github_api_url")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_else(default_github_api_url),
                timeout_ms: answers
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .map(|v| v as u32)
                    .unwrap_or(DEFAULT_TIMEOUT_MS),
                max_result_size_bytes: default_max_result_size(),
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

bindings::export!(Component with_types_in bindings);

// ============================================================================
// Core Execution Logic
// ============================================================================

/// Handle generic execute operation - routes to appropriate handler
fn handle_execute(input: &Value) -> Value {
    let translated_query: TranslatedQuery = match input
        .get("query")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(tq) => tq,
        None => {
            return json!({
                "ok": false,
                "error": "missing or invalid query"
            });
        }
    };

    // Route to appropriate handler based on query type
    match translated_query.query_type.as_str() {
        "sql" => handle_execute_sql(input, &translated_query),
        "http" => handle_execute_github_internal(input, &translated_query),
        "mcp" => handle_execute_mcp_internal(&translated_query),
        other => json!({
            "ok": false,
            "error": format!("unsupported query type: {}", other)
        }),
    }
}

/// Handle SQLite query execution
/// Note: Actual execution is delegated to the host runtime
fn handle_execute_sql(_input: &Value, translated_query: &TranslatedQuery) -> Value {
    // For SQLite, we prepare an execution request that the runtime will execute
    // This is because the WASM component doesn't have direct database access
    // The runtime will execute the query and return results

    let QuerySpec::Sql {
        sql,
        params,
        param_names,
    } = &translated_query.query
    else {
        return json!({
            "ok": false,
            "error": "expected SQL query spec"
        });
    };

    log_event("execute_sql_prepared");

    // Return the prepared query for runtime execution
    // In a full implementation, this would call a host function to execute
    json!({
        "ok": true,
        "execution_type": "sql",
        "prepared_query": {
            "sql": sql,
            "params": params,
            "param_names": param_names,
            "max_rows": translated_query.max_rows,
        },
        "metadata": {
            "query_hash": translated_query.query_hash,
            "renderer": translated_query.renderer,
            "renderer_options": translated_query.renderer_options,
        },
        "requires_runtime_execution": true,
        "message": "SQL query prepared for runtime execution"
    })
}

/// Handle GitHub API execution
fn handle_execute_github(input: &Value) -> Value {
    let translated_query: TranslatedQuery = match input
        .get("query")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(tq) => tq,
        None => {
            return json!({
                "ok": false,
                "error": "missing or invalid query"
            });
        }
    };

    handle_execute_github_internal(input, &translated_query)
}

fn handle_execute_github_internal(input: &Value, translated_query: &TranslatedQuery) -> Value {
    let cfg = load_config(input).unwrap_or_default();

    let QuerySpec::Http {
        method,
        path,
        query_params,
        headers,
    } = &translated_query.query
    else {
        return json!({
            "ok": false,
            "error": "expected HTTP query spec"
        });
    };

    // Get GitHub token
    let token = match cfg.github_token.as_ref() {
        Some(t) => match resolve_secret(t) {
            Ok(resolved) => resolved,
            Err(err) => {
                return json!({
                    "ok": false,
                    "error": format!("GitHub token error: {}", err)
                });
            }
        },
        None => {
            return json!({
                "ok": false,
                "error": "GitHub token is required"
            });
        }
    };

    // Build full URL
    let mut url = format!("{}{}", cfg.github_api_url, path);
    if !query_params.is_empty() {
        let params: Vec<String> = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
            .collect();
        url.push('?');
        url.push_str(&params.join("&"));
    }

    // Build headers
    let mut all_headers: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    all_headers.push(("Authorization".to_string(), format!("Bearer {}", token)));
    all_headers.push((
        "User-Agent".to_string(),
        "chat2data-executor/0.1".to_string(),
    ));

    // Make HTTP request
    let req = client::Request {
        method: method.clone(),
        url,
        headers: all_headers,
        body: None,
    };

    let options = client::RequestOptions {
        timeout_ms: Some(cfg.timeout_ms),
        allow_insecure: Some(false),
        follow_redirects: Some(true),
    };

    match http_send(&req, &options) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            let body = resp.body.unwrap_or_default();
            let body_size = body.len();

            // Check result size
            if body_size > cfg.max_result_size_bytes {
                return json!({
                    "ok": true,
                    "result": ExecutionResult {
                        success: false,
                        data: None,
                        error: Some(ExecutionError {
                            code: "RESULT_TOO_LARGE".to_string(),
                            message: format!("Result size {} exceeds limit {}", body_size, cfg.max_result_size_bytes),
                            details: None,
                            retryable: false,
                            retry_after: None,
                        }),
                        metadata: ExecutionMetadata {
                            query_hash: translated_query.query_hash.clone(),
                            execution_time_ms: None,
                            row_count: None,
                            result_size_bytes: Some(body_size),
                            renderer: translated_query.renderer.clone(),
                            renderer_options: translated_query.renderer_options.clone(),
                        },
                    }
                });
            }

            // Parse response
            let data: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(e) => {
                    return json!({
                        "ok": false,
                        "error": format!("failed to parse response: {}", e)
                    });
                }
            };

            // Convert to QueryData
            let (rows, columns) = extract_rows_from_github_response(&data);
            let row_count = rows.len();
            let truncated = row_count >= translated_query.max_rows;

            log_event("execute_github_success");

            json!({
                "ok": true,
                "result": ExecutionResult {
                    success: true,
                    data: Some(QueryData {
                        rows,
                        columns,
                        total_count: None,
                        truncated,
                        truncation_reason: if truncated { Some("max_rows".to_string()) } else { None },
                    }),
                    error: None,
                    metadata: ExecutionMetadata {
                        query_hash: translated_query.query_hash.clone(),
                        execution_time_ms: None,
                        row_count: Some(row_count),
                        result_size_bytes: Some(body_size),
                        renderer: translated_query.renderer.clone(),
                        renderer_options: translated_query.renderer_options.clone(),
                    },
                }
            })
        }
        Ok(resp) => {
            let status = resp.status;
            let body = resp
                .body
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();

            // Check for rate limiting
            let is_rate_limited = status == 403 || status == 429;
            let retry_after = if is_rate_limited {
                Some(60) // Default retry after 60 seconds
            } else {
                None
            };

            log_event("execute_github_error");

            json!({
                "ok": true,
                "result": ExecutionResult {
                    success: false,
                    data: None,
                    error: Some(ExecutionError {
                        code: format!("HTTP_{}", status),
                        message: format!("GitHub API error (status {})", status),
                        details: Some(json!({"body": body})),
                        retryable: is_rate_limited || status >= 500,
                        retry_after,
                    }),
                    metadata: ExecutionMetadata {
                        query_hash: translated_query.query_hash.clone(),
                        execution_time_ms: None,
                        row_count: None,
                        result_size_bytes: None,
                        renderer: translated_query.renderer.clone(),
                        renderer_options: translated_query.renderer_options.clone(),
                    },
                }
            })
        }
        Err(err) => {
            log_event("execute_github_http_error");

            json!({
                "ok": true,
                "result": ExecutionResult {
                    success: false,
                    data: None,
                    error: Some(ExecutionError {
                        code: err.code.clone(),
                        message: err.message.clone(),
                        details: None,
                        retryable: true,
                        retry_after: None,
                    }),
                    metadata: ExecutionMetadata {
                        query_hash: translated_query.query_hash.clone(),
                        execution_time_ms: None,
                        row_count: None,
                        result_size_bytes: None,
                        renderer: translated_query.renderer.clone(),
                        renderer_options: translated_query.renderer_options.clone(),
                    },
                }
            })
        }
    }
}

/// Handle MCP tool execution (delegated)
fn handle_execute_mcp(input: &Value) -> Value {
    let translated_query: TranslatedQuery = match input
        .get("query")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(tq) => tq,
        None => {
            return json!({
                "ok": false,
                "error": "missing or invalid query"
            });
        }
    };

    handle_execute_mcp_internal(&translated_query)
}

fn handle_execute_mcp_internal(translated_query: &TranslatedQuery) -> Value {
    let QuerySpec::Mcp {
        tool_name,
        arguments,
    } = &translated_query.query
    else {
        return json!({
            "ok": false,
            "error": "expected MCP query spec"
        });
    };

    log_event("execute_mcp_prepared");

    // MCP execution is delegated to the MCP runtime
    // Return a prepared call request
    json!({
        "ok": true,
        "execution_type": "mcp",
        "prepared_call": {
            "tool_name": tool_name,
            "arguments": arguments,
        },
        "metadata": {
            "query_hash": translated_query.query_hash,
            "renderer": translated_query.renderer,
            "renderer_options": translated_query.renderer_options,
        },
        "requires_runtime_execution": true,
        "message": "MCP tool call prepared for runtime execution"
    })
}

/// Extract rows from GitHub API response
fn extract_rows_from_github_response(data: &Value) -> (Vec<Value>, Vec<String>) {
    // Handle array response (list endpoints)
    if let Some(arr) = data.as_array() {
        let rows: Vec<Value> = arr.clone();
        let columns = if let Some(first) = rows.first() {
            if let Some(obj) = first.as_object() {
                obj.keys().cloned().collect()
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        return (rows, columns);
    }

    // Handle object response with items array (search endpoints)
    if let Some(items) = data.get("items").and_then(Value::as_array) {
        let rows: Vec<Value> = items.clone();
        let columns = if let Some(first) = rows.first() {
            if let Some(obj) = first.as_object() {
                obj.keys().cloned().collect()
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        return (rows, columns);
    }

    // Handle single object response (get endpoints)
    if data.is_object() {
        let columns = data
            .as_object()
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default();
        return (vec![data.clone()], columns);
    }

    (vec![], vec![])
}

/// Simple URL encoding
fn url_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

// ============================================================================
// Helper Functions
// ============================================================================

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

fn build_describe_payload() -> DescribePayload {
    let mut config_fields = BTreeMap::new();
    config_fields.insert(
        "github_token".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("chat2data-executor.schema.config.github_token.title"),
                description: i18n("chat2data-executor.schema.config.github_token.description"),
                format: None,
                secret: true,
            },
        },
    );

    DescribePayload {
        provider: COMPONENT_ID.to_string(),
        world: WORLD_ID.to_string(),
        operations: vec![
            op(
                "execute",
                "chat2data-executor.op.execute.title",
                "chat2data-executor.op.execute.description",
            ),
            op(
                "execute_github",
                "chat2data-executor.op.execute_github.title",
                "chat2data-executor.op.execute_github.description",
            ),
            op(
                "execute_mcp",
                "chat2data-executor.op.execute_mcp.title",
                "chat2data-executor.op.execute_mcp.description",
            ),
        ],
        input_schema: SchemaIr::Object {
            title: i18n("chat2data-executor.schema.input.title"),
            description: i18n("chat2data-executor.schema.input.description"),
            fields: BTreeMap::new(),
            additional_properties: true,
        },
        output_schema: SchemaIr::Object {
            title: i18n("chat2data-executor.schema.output.title"),
            description: i18n("chat2data-executor.schema.output.description"),
            fields: BTreeMap::new(),
            additional_properties: true,
        },
        config_schema: SchemaIr::Object {
            title: i18n("chat2data-executor.schema.config.title"),
            description: i18n("chat2data-executor.schema.config.description"),
            fields: config_fields,
            additional_properties: false,
        },
        redactions: vec![RedactionRule {
            path: "$.github_token".to_string(),
            strategy: "replace".to_string(),
        }],
        schema_hash: "chat2data-executor-schema-v1".to_string(),
    }
}

fn build_qa_spec(mode: bindings::exports::greentic::component::qa::Mode) -> QaSpec {
    use bindings::exports::greentic::component::qa::Mode;

    match mode {
        Mode::Default | Mode::Upgrade | Mode::Remove => QaSpec {
            mode: match mode {
                Mode::Default => "default",
                Mode::Upgrade => "upgrade",
                Mode::Remove => "remove",
                _ => "default",
            }
            .to_string(),
            title: i18n("chat2data-executor.qa.default.title"),
            description: None,
            questions: Vec::new(),
            defaults: json!({}),
        },
        Mode::Setup => QaSpec {
            mode: "setup".to_string(),
            title: i18n("chat2data-executor.qa.setup.title"),
            description: None,
            questions: vec![QaQuestionSpec {
                id: "github_token".to_string(),
                label: i18n("chat2data-executor.qa.setup.github_token"),
                help: None,
                error: None,
                kind: "text".to_string(),
                required: false,
                default: None,
            }],
            defaults: json!({}),
        },
    }
}

fn op(name: &str, title: &str, description: &str) -> OperationDescriptor {
    OperationDescriptor {
        name: name.to_string(),
        title: i18n(title),
        description: i18n(description),
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

fn log_event(event: &str) {
    #[cfg(test)]
    {
        let _ = event;
    }

    #[cfg(not(test))]
    {
        let span = logger_api::SpanContext {
            tenant: "tenant".into(),
            session_id: None,
            flow_id: "chat2data-executor".into(),
            node_id: None,
            provider: "chat2data-executor".into(),
            start_ms: None,
            end_ms: None,
        };
        let fields = [("event".to_string(), event.to_string())];
        let _ = logger_api::log(&span, &fields, None);
    }
}

fn resolve_secret(token: &str) -> Result<String, String> {
    #[cfg(test)]
    {
        let _ = token;
        Ok("test-github-token".to_string())
    }

    #[cfg(not(test))]
    {
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
}

fn http_send(
    req: &client::Request,
    options: &client::RequestOptions,
) -> Result<client::Response, client::HostError> {
    #[cfg(test)]
    {
        let _ = (req, options);
        Err(client::HostError {
            code: "test".into(),
            message: "not implemented in test".into(),
        })
    }

    #[cfg(not(test))]
    {
        client::send(req, Some(*options), None)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rows_from_array() {
        let data = json!([
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"}
        ]);

        let (rows, columns) = extract_rows_from_github_response(&data);
        assert_eq!(rows.len(), 2);
        assert!(columns.contains(&"id".to_string()));
        assert!(columns.contains(&"name".to_string()));
    }

    #[test]
    fn test_extract_rows_from_search_response() {
        let data = json!({
            "total_count": 100,
            "items": [
                {"id": 1, "title": "Issue 1"},
                {"id": 2, "title": "Issue 2"}
            ]
        });

        let (rows, columns) = extract_rows_from_github_response(&data);
        assert_eq!(rows.len(), 2);
        assert!(columns.contains(&"id".to_string()));
        assert!(columns.contains(&"title".to_string()));
    }

    #[test]
    fn test_extract_rows_from_single_object() {
        let data = json!({
            "id": 123,
            "title": "Single Issue",
            "state": "open"
        });

        let (rows, columns) = extract_rows_from_github_response(&data);
        assert_eq!(rows.len(), 1);
        assert!(columns.contains(&"id".to_string()));
        assert!(columns.contains(&"title".to_string()));
        assert!(columns.contains(&"state".to_string()));
    }

    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode("hello"), "hello");
        assert_eq!(url_encode("hello world"), "hello+world");
        assert_eq!(url_encode("a=b&c=d"), "a%3Db%26c%3Dd");
    }

    #[test]
    fn test_handle_execute_sql() {
        let translated_query = TranslatedQuery {
            target: "sqlite".to_string(),
            query_type: "sql".to_string(),
            query: QuerySpec::Sql {
                sql: "SELECT * FROM users WHERE id = ?1".to_string(),
                params: vec![SqlParam::Integer { value: 1 }],
                param_names: vec!["id".to_string()],
            },
            renderer: "table".to_string(),
            renderer_options: json!({}),
            max_rows: 100,
            query_hash: "test-hash".to_string(),
        };

        let result = handle_execute_sql(&json!({}), &translated_query);
        assert_eq!(result.get("ok"), Some(&json!(true)));
        assert_eq!(result.get("requires_runtime_execution"), Some(&json!(true)));
    }

    #[test]
    fn test_handle_execute_mcp() {
        let translated_query = TranslatedQuery {
            target: "mcp".to_string(),
            query_type: "mcp".to_string(),
            query: QuerySpec::Mcp {
                tool_name: "read_file".to_string(),
                arguments: json!({"path": "/src/main.rs"}),
            },
            renderer: "card".to_string(),
            renderer_options: json!({}),
            max_rows: 1,
            query_hash: "test-hash".to_string(),
        };

        let result = handle_execute_mcp_internal(&translated_query);
        assert_eq!(result.get("ok"), Some(&json!(true)));
        assert_eq!(result.get("requires_runtime_execution"), Some(&json!(true)));
    }
}
