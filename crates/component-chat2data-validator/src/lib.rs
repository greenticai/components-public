#![allow(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]
#![allow(clippy::collapsible_if)]

//! Chat2Data Validator Component
//!
//! This component validates QueryIntent against a whitelist configuration to ensure
//! only authorized operations are allowed. It implements Layer 2 of the Chat2Data
//! security architecture.
//!
//! # Operations
//!
//! - `validate` - Validate a QueryIntent against whitelist
//! - `load_whitelist` - Load/reload whitelist configuration
//!
//! # Configuration
//!
//! - `whitelist` - Inline whitelist configuration (JSON/YAML)
//! - `whitelist_url` - URL to fetch whitelist from
//! - `strict_mode` - Reject on any warning (default: true)
//!
//! # Security Features
//!
//! - Table/column allowlist enforcement
//! - Forbidden column blocking
//! - SQL injection pattern detection
//! - Operation type restrictions
//! - Row limit enforcement

use greentic_types::cbor::canonical;
use greentic_types::i18n_text::I18nText as CanonicalI18nText;
use greentic_types::schemas::component::v0_6_0::{
    ComponentQaSpec, QaMode as CanonicalQaMode, Question as CanonicalQuestion,
    QuestionKind as CanonicalQuestionKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;
#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::telemetry_logger as logger_api;

const COMPONENT_ID: &str = "chat2data-validator";
const WORLD_ID: &str = "component-v0-v6-v0";
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");

const I18N_KEYS: &[&str] = &[
    "chat2data-validator.op.validate.title",
    "chat2data-validator.op.validate.description",
    "chat2data-validator.op.load_whitelist.title",
    "chat2data-validator.op.load_whitelist.description",
    "chat2data-validator.schema.input.title",
    "chat2data-validator.schema.input.description",
    "chat2data-validator.schema.input.intent.title",
    "chat2data-validator.schema.input.intent.description",
    "chat2data-validator.schema.output.title",
    "chat2data-validator.schema.output.description",
    "chat2data-validator.schema.output.valid.title",
    "chat2data-validator.schema.output.valid.description",
    "chat2data-validator.schema.config.title",
    "chat2data-validator.schema.config.description",
    "chat2data-validator.schema.config.whitelist.title",
    "chat2data-validator.schema.config.whitelist.description",
    "chat2data-validator.schema.config.strict_mode.title",
    "chat2data-validator.schema.config.strict_mode.description",
    "chat2data-validator.qa.default.title",
    "chat2data-validator.qa.default.description",
    "chat2data-validator.qa.setup.title",
    "chat2data-validator.qa.setup.description",
    "chat2data-validator.qa.update.title",
    "chat2data-validator.qa.update.description",
    "chat2data-validator.qa.remove.title",
    "chat2data-validator.qa.remove.description",
    "chat2data-validator.error.target_not_allowed",
    "chat2data-validator.error.action_not_allowed",
    "chat2data-validator.error.table_not_allowed",
    "chat2data-validator.error.column_not_allowed",
    "chat2data-validator.error.column_forbidden",
    "chat2data-validator.error.forbidden_pattern",
    "chat2data-validator.error.invalid_identifier",
];

/// Default forbidden SQL patterns that indicate injection attempts
const DEFAULT_FORBIDDEN_PATTERNS: &[&str] = &[
    "UNION",
    "INTO OUTFILE",
    "LOAD_FILE",
    "--",
    "/*",
    "*/",
    "xp_",
    "exec(",
    "EXECUTE(",
    "sp_",
    "@@",
    "CHAR(",
    "CONCAT(",
    "BENCHMARK(",
    "SLEEP(",
    "WAITFOR",
    "PG_SLEEP",
    "DROP ",
    "DELETE ",
    "TRUNCATE ",
    "INSERT ",
    "UPDATE ",
    "ALTER ",
    "CREATE ",
    "GRANT ",
    "REVOKE ",
];

// ============================================================================
// Data Structures
// ============================================================================

/// Component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComponentConfig {
    /// Inline whitelist configuration
    #[serde(default)]
    whitelist: Option<Whitelist>,
    /// Strict mode - reject on any warning
    #[serde(default = "default_strict_mode")]
    strict_mode: bool,
}

/// Whitelist configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Whitelist {
    /// Version of whitelist format
    #[serde(default)]
    version: String,
    /// SQLite target configuration
    #[serde(default)]
    sqlite: SqliteConfig,
    /// GitHub target configuration
    #[serde(default)]
    github: GitHubConfig,
    /// MCP target configuration
    #[serde(default)]
    mcp: McpConfig,
}

/// SQLite whitelist configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SqliteConfig {
    /// Allowed tables with their configurations
    #[serde(default)]
    allowed_tables: BTreeMap<String, TableConfig>,
    /// Allowed operations
    #[serde(default = "default_sqlite_operations")]
    allowed_operations: Vec<String>,
    /// Forbidden patterns in values
    #[serde(default)]
    forbidden_patterns: Vec<String>,
}

/// Table-level configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TableConfig {
    /// Allowed columns ("*" means all)
    #[serde(default)]
    allowed_columns: Vec<String>,
    /// Forbidden columns (always blocked)
    #[serde(default)]
    forbidden_columns: Vec<String>,
    /// Maximum rows to return
    #[serde(default = "default_max_rows")]
    max_rows: usize,
    /// Table description for context
    #[serde(default)]
    description: Option<String>,
}

/// GitHub whitelist configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GitHubConfig {
    /// Allowed repository patterns
    #[serde(default)]
    allowed_repos: Vec<String>,
    /// Forbidden repository patterns
    #[serde(default)]
    forbidden_repos: Vec<String>,
    /// Allowed operations
    #[serde(default = "default_github_operations")]
    allowed_operations: Vec<String>,
}

/// MCP whitelist configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct McpConfig {
    /// Allowed tools
    #[serde(default)]
    allowed_tools: Vec<String>,
    /// Tool-specific restrictions
    #[serde(default)]
    tool_restrictions: BTreeMap<String, ToolRestriction>,
}

/// Tool-specific restrictions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ToolRestriction {
    /// Maximum file size in bytes
    #[serde(default)]
    max_size_bytes: Option<u64>,
    /// Allowed file extensions
    #[serde(default)]
    allowed_extensions: Vec<String>,
    /// Forbidden paths (glob patterns)
    #[serde(default)]
    forbidden_paths: Vec<String>,
}

/// Validation input
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ValidateInput {
    /// The intent to validate
    intent: QueryIntent,
}

/// QueryIntent from LLM parser
#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueryIntent {
    /// Processing status
    status: String,
    /// Parsed intent details
    #[serde(default)]
    intent: Option<Intent>,
    /// Clarification question
    #[serde(default)]
    clarification: Option<String>,
    /// Error reason
    #[serde(default)]
    error_reason: Option<String>,
    /// Confidence score
    #[serde(default)]
    confidence: f64,
}

/// Parsed intent details
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Intent {
    /// Target data source
    target: String,
    /// Action to perform
    action: String,
    /// Action parameters
    params: Value,
    /// Preferred renderer
    #[serde(default = "default_renderer")]
    renderer: String,
    /// Renderer-specific options
    #[serde(default)]
    renderer_options: Value,
}

/// Validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ValidationResult {
    /// Whether validation passed
    valid: bool,
    /// Validated and sanitized intent (if valid)
    #[serde(skip_serializing_if = "Option::is_none")]
    validated_intent: Option<ValidatedIntent>,
    /// Validation errors
    #[serde(default)]
    errors: Vec<ValidationError>,
    /// Validation warnings
    #[serde(default)]
    warnings: Vec<String>,
}

/// Validated intent with enforced limits
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ValidatedIntent {
    /// Original intent
    intent: Intent,
    /// Enforced maximum rows
    max_rows: usize,
    /// Sanitized columns (forbidden columns removed)
    #[serde(default)]
    sanitized_columns: Option<Vec<String>>,
}

/// Validation error
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ValidationError {
    /// Error code
    code: String,
    /// Human-readable message
    message: String,
    /// Additional context
    #[serde(default)]
    context: Option<Value>,
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

fn default_strict_mode() -> bool {
    true
}

fn default_max_rows() -> usize {
    1000
}

fn default_renderer() -> String {
    "auto".to_string()
}

fn default_sqlite_operations() -> Vec<String> {
    vec![
        "select".to_string(),
        "count".to_string(),
        "aggregate".to_string(),
    ]
}

fn default_github_operations() -> Vec<String> {
    vec![
        "list_issues".to_string(),
        "get_issue".to_string(),
        "list_prs".to_string(),
        "get_pr".to_string(),
        "search_code".to_string(),
        "list_commits".to_string(),
    ]
}

// ============================================================================
// WASM Component Implementation
// ============================================================================

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
            summary: Some("Whitelist-based validator for chat2data query intents".to_string()),
            capabilities: vec!["host:telemetry".to_string()],
            ops: vec![
                node::Op {
                    name: "validate".to_string(),
                    summary: Some("Validate a query intent against the whitelist".to_string()),
                    input: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(&json!({
                            "type": "object"
                        }))),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    output: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(&json!({
                            "type": "object"
                        }))),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    examples: Vec::new(),
                },
                node::Op {
                    name: "load_whitelist".to_string(),
                    summary: Some("Load or validate a whitelist configuration".to_string()),
                    input: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(&json!({
                            "type": "object"
                        }))),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    output: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(&json!({
                            "type": "object"
                        }))),
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
                        &json!({"ok": false, "error": format!("invalid input cbor: {err}")}),
                    ),
                    output_metadata_cbor: None,
                });
            }
        };

        let output = match op.as_str() {
            "validate" => handle_validate(&input),
            "load_whitelist" => handle_load_whitelist(&input),
            other => json!({"ok": false, "error": format!("unsupported op: {other}")}),
        };

        Ok(node::InvocationResult {
            ok: true,
            output_cbor: canonical_cbor_bytes(&output),
            output_metadata_cbor: None,
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod qa_exports {
    use crate::Whitelist;
    use serde_json::Value;

    wit_bindgen::generate!({
        inline: r#"
            package greentic:component@0.6.0;

            interface component-qa {
                enum qa-mode {
                    default,
                    setup,
                    update,
                    remove
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
            crate::canonical_cbor_bytes(&crate::canonical_qa_spec(match mode {
                exports::greentic::component::component_qa::QaMode::Default => "default",
                exports::greentic::component::component_qa::QaMode::Setup => "setup",
                exports::greentic::component::component_qa::QaMode::Update => "update",
                exports::greentic::component::component_qa::QaMode::Remove => "remove",
            }))
        }

        fn apply_answers(
            mode: exports::greentic::component::component_qa::QaMode,
            _current_config: Vec<u8>,
            answers: Vec<u8>,
        ) -> Vec<u8> {
            let answers: Value = match crate::decode_cbor(&answers) {
                Ok(value) => value,
                Err(err) => {
                    return crate::canonical_cbor_bytes(&crate::ApplyAnswersResult {
                        ok: false,
                        config: None,
                        error: Some(format!("invalid answers cbor: {err}")),
                    });
                }
            };

            if mode == exports::greentic::component::component_qa::QaMode::Setup {
                let whitelist: Option<Whitelist> = answers
                    .get("whitelist")
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok());

                let cfg = crate::ComponentConfig {
                    whitelist,
                    strict_mode: answers
                        .get("strict_mode")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                };

                return crate::canonical_cbor_bytes(&crate::ApplyAnswersResult {
                    ok: true,
                    config: Some(cfg),
                    error: None,
                });
            }

            crate::canonical_cbor_bytes(&crate::ApplyAnswersResult {
                ok: true,
                config: None,
                error: None,
            })
        }
    }

    impl exports::greentic::component::component_i18n::Guest for WizardSupport {
        fn i18n_keys() -> Vec<String> {
            crate::I18N_KEYS.iter().map(|k| (*k).to_string()).collect()
        }
    }

    export!(WizardSupport with_types_in self);
}

#[cfg(target_arch = "wasm32")]
greentic_interfaces_guest::export_component_v060!(Component);

// ============================================================================
// Core Validation Logic
// ============================================================================

/// Handle the validate operation
fn handle_validate(input: &Value) -> Value {
    // Load configuration
    let cfg = match load_config(input) {
        Ok(cfg) => cfg,
        Err(err) => return json!({"ok": false, "error": err}),
    };

    // Extract intent
    let query_intent: QueryIntent = match input
        .get("intent")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(intent) => intent,
        None => {
            return json!({
                "ok": false,
                "error": "missing or invalid intent"
            });
        }
    };

    // Check status - only validate "ready" intents
    if query_intent.status != "ready" {
        return json!({
            "ok": true,
            "valid": true,
            "validated_intent": null,
            "pass_through": true,
            "reason": format!("Intent status is '{}', passing through", query_intent.status)
        });
    }

    // Get the intent details
    let intent = match query_intent.intent {
        Some(i) => i,
        None => {
            return json!({
                "ok": false,
                "error": "status is 'ready' but intent is missing"
            });
        }
    };

    // Get whitelist
    let whitelist = cfg.whitelist.unwrap_or_default();

    // Perform validation
    let result = validate_intent(&intent, &whitelist, cfg.strict_mode);

    log_event(if result.valid {
        "validation_passed"
    } else {
        "validation_failed"
    });

    json!({
        "ok": true,
        "valid": result.valid,
        "validated_intent": result.validated_intent,
        "errors": result.errors,
        "warnings": result.warnings,
    })
}

/// Handle loading whitelist configuration
fn handle_load_whitelist(input: &Value) -> Value {
    // Extract whitelist
    let whitelist: Whitelist = match input
        .get("whitelist")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(wl) => wl,
        None => {
            return json!({
                "ok": false,
                "error": "missing or invalid whitelist"
            });
        }
    };

    // Validate whitelist structure
    let mut warnings = Vec::new();

    if whitelist.sqlite.allowed_tables.is_empty()
        && whitelist.github.allowed_repos.is_empty()
        && whitelist.mcp.allowed_tools.is_empty()
    {
        warnings.push("Whitelist has no allowed targets configured".to_string());
    }

    json!({
        "ok": true,
        "whitelist": whitelist,
        "warnings": warnings,
    })
}

/// Validate an intent against whitelist
fn validate_intent(intent: &Intent, whitelist: &Whitelist, strict_mode: bool) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Validate based on target
    let validated = match intent.target.as_str() {
        "sqlite" => validate_sqlite_intent(intent, &whitelist.sqlite, &mut errors, &mut warnings),
        "github" => validate_github_intent(intent, &whitelist.github, &mut errors, &mut warnings),
        "mcp" => validate_mcp_intent(intent, &whitelist.mcp, &mut errors, &mut warnings),
        other => {
            errors.push(ValidationError {
                code: "TARGET_NOT_ALLOWED".to_string(),
                message: format!("Target '{}' is not supported", other),
                context: None,
            });
            None
        }
    };

    // In strict mode, warnings become errors
    if strict_mode && !warnings.is_empty() {
        for warning in &warnings {
            errors.push(ValidationError {
                code: "STRICT_MODE_WARNING".to_string(),
                message: warning.clone(),
                context: None,
            });
        }
    }

    ValidationResult {
        valid: errors.is_empty() && validated.is_some(),
        validated_intent: validated,
        errors,
        warnings,
    }
}

/// Validate SQLite intent
fn validate_sqlite_intent(
    intent: &Intent,
    config: &SqliteConfig,
    errors: &mut Vec<ValidationError>,
    warnings: &mut Vec<String>,
) -> Option<ValidatedIntent> {
    let params = &intent.params;

    // Check operation is allowed
    if !config.allowed_operations.contains(&intent.action) {
        errors.push(ValidationError {
            code: "ACTION_NOT_ALLOWED".to_string(),
            message: format!(
                "SQLite action '{}' is not allowed. Allowed: {:?}",
                intent.action, config.allowed_operations
            ),
            context: None,
        });
        return None;
    }

    // Extract table name
    let table = match params.get("table").and_then(Value::as_str) {
        Some(t) => t,
        None => {
            errors.push(ValidationError {
                code: "MISSING_TABLE".to_string(),
                message: "SQLite query missing table parameter".to_string(),
                context: None,
            });
            return None;
        }
    };

    // Validate table name format (prevent injection)
    if !is_valid_identifier(table) {
        errors.push(ValidationError {
            code: "INVALID_IDENTIFIER".to_string(),
            message: format!("Invalid table name: '{}'", table),
            context: Some(json!({"table": table})),
        });
        return None;
    }

    // Check table is allowed
    let table_config = match config.allowed_tables.get(table) {
        Some(tc) => tc,
        None => {
            errors.push(ValidationError {
                code: "TABLE_NOT_ALLOWED".to_string(),
                message: format!(
                    "Table '{}' is not in the allowed list. Allowed: {:?}",
                    table,
                    config.allowed_tables.keys().collect::<Vec<_>>()
                ),
                context: None,
            });
            return None;
        }
    };

    // Validate and sanitize columns
    let mut sanitized_columns = None;
    if let Some(columns) = params.get("columns").and_then(Value::as_array) {
        let mut valid_columns = Vec::new();

        for col in columns {
            let col_name = match col.as_str() {
                Some(c) => c,
                None => continue,
            };

            // Skip wildcard
            if col_name == "*" {
                valid_columns.push("*".to_string());
                continue;
            }

            // Validate identifier format
            if !is_valid_identifier(col_name) {
                errors.push(ValidationError {
                    code: "INVALID_IDENTIFIER".to_string(),
                    message: format!("Invalid column name: '{}'", col_name),
                    context: Some(json!({"column": col_name})),
                });
                continue;
            }

            // Check forbidden columns
            if table_config
                .forbidden_columns
                .contains(&col_name.to_string())
            {
                errors.push(ValidationError {
                    code: "COLUMN_FORBIDDEN".to_string(),
                    message: format!("Column '{}' is forbidden for security reasons", col_name),
                    context: Some(json!({"column": col_name, "table": table})),
                });
                continue;
            }

            // Check allowed columns
            let allowed = table_config.allowed_columns.contains(&"*".to_string())
                || table_config.allowed_columns.contains(&col_name.to_string());

            if !allowed {
                warnings.push(format!(
                    "Column '{}' is not in allowed list, removing from query",
                    col_name
                ));
                continue;
            }

            valid_columns.push(col_name.to_string());
        }

        sanitized_columns = Some(valid_columns);
    }

    // Check for forbidden patterns in all string values
    let forbidden_patterns = if config.forbidden_patterns.is_empty() {
        DEFAULT_FORBIDDEN_PATTERNS
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        config.forbidden_patterns.clone()
    };

    if let Err(pattern) = check_forbidden_patterns(params, &forbidden_patterns) {
        errors.push(ValidationError {
            code: "FORBIDDEN_PATTERN".to_string(),
            message: format!("Forbidden pattern detected: '{}'", pattern),
            context: Some(json!({"pattern": pattern})),
        });
        return None;
    }

    // Validate where clause
    if let Some(where_clause) = params.get("where") {
        if let Err(e) = validate_where_clause(where_clause, table_config, &forbidden_patterns) {
            errors.push(e);
            return None;
        }
    }

    if !errors.is_empty() {
        return None;
    }

    Some(ValidatedIntent {
        intent: intent.clone(),
        max_rows: table_config.max_rows,
        sanitized_columns,
    })
}

/// Validate GitHub intent
fn validate_github_intent(
    intent: &Intent,
    config: &GitHubConfig,
    errors: &mut Vec<ValidationError>,
    _warnings: &mut Vec<String>,
) -> Option<ValidatedIntent> {
    // Check operation is allowed
    if !config.allowed_operations.contains(&intent.action) {
        errors.push(ValidationError {
            code: "ACTION_NOT_ALLOWED".to_string(),
            message: format!(
                "GitHub action '{}' is not allowed. Allowed: {:?}",
                intent.action, config.allowed_operations
            ),
            context: None,
        });
        return None;
    }

    // Extract repository if present
    if let Some(repo) = intent.params.get("repo").and_then(Value::as_str) {
        // Check allowed repos
        let allowed = config.allowed_repos.iter().any(|pattern| {
            if pattern.contains('*') {
                matches_glob(repo, pattern)
            } else {
                repo == pattern
            }
        });

        if !allowed && !config.allowed_repos.is_empty() {
            errors.push(ValidationError {
                code: "REPO_NOT_ALLOWED".to_string(),
                message: format!("Repository '{}' is not in the allowed list", repo),
                context: Some(json!({"repo": repo})),
            });
            return None;
        }

        // Check forbidden repos
        let forbidden = config.forbidden_repos.iter().any(|pattern| {
            if pattern.contains('*') {
                matches_glob(repo, pattern)
            } else {
                repo == pattern
            }
        });

        if forbidden {
            errors.push(ValidationError {
                code: "REPO_FORBIDDEN".to_string(),
                message: format!("Repository '{}' is forbidden", repo),
                context: Some(json!({"repo": repo})),
            });
            return None;
        }
    }

    Some(ValidatedIntent {
        intent: intent.clone(),
        max_rows: 100, // GitHub API default
        sanitized_columns: None,
    })
}

/// Validate MCP intent
fn validate_mcp_intent(
    intent: &Intent,
    config: &McpConfig,
    errors: &mut Vec<ValidationError>,
    warnings: &mut Vec<String>,
) -> Option<ValidatedIntent> {
    let tool_name = &intent.action;

    // Check tool is allowed
    if !config.allowed_tools.contains(tool_name) {
        errors.push(ValidationError {
            code: "TOOL_NOT_ALLOWED".to_string(),
            message: format!(
                "MCP tool '{}' is not allowed. Allowed: {:?}",
                tool_name, config.allowed_tools
            ),
            context: None,
        });
        return None;
    }

    // Check tool-specific restrictions
    if let Some(restriction) = config.tool_restrictions.get(tool_name) {
        // Check path restrictions
        if let Some(path) = intent.params.get("path").and_then(Value::as_str) {
            // Check forbidden paths
            let is_forbidden = restriction
                .forbidden_paths
                .iter()
                .any(|pattern| matches_glob(path, pattern));

            if is_forbidden {
                errors.push(ValidationError {
                    code: "PATH_FORBIDDEN".to_string(),
                    message: format!("Path '{}' is forbidden", path),
                    context: Some(json!({"path": path})),
                });
                return None;
            }

            // Check allowed extensions
            if !restriction.allowed_extensions.is_empty() {
                let has_allowed_ext = restriction
                    .allowed_extensions
                    .iter()
                    .any(|ext| path.ends_with(ext));

                if !has_allowed_ext {
                    warnings.push(format!("File extension not in allowed list: {}", path));
                }
            }
        }
    }

    Some(ValidatedIntent {
        intent: intent.clone(),
        max_rows: 1000,
        sanitized_columns: None,
    })
}

/// Validate a where clause
fn validate_where_clause(
    where_clause: &Value,
    table_config: &TableConfig,
    forbidden_patterns: &[String],
) -> Result<(), ValidationError> {
    match where_clause {
        Value::Object(map) => {
            for (key, value) in map {
                // Validate column name
                if !is_valid_identifier(key) {
                    return Err(ValidationError {
                        code: "INVALID_IDENTIFIER".to_string(),
                        message: format!("Invalid column name in where clause: '{}'", key),
                        context: Some(json!({"column": key})),
                    });
                }

                // Check forbidden columns
                if table_config.forbidden_columns.contains(key) {
                    return Err(ValidationError {
                        code: "COLUMN_FORBIDDEN".to_string(),
                        message: format!("Column '{}' is forbidden", key),
                        context: Some(json!({"column": key})),
                    });
                }

                // Check forbidden patterns in values
                if let Err(pattern) = check_forbidden_patterns(value, forbidden_patterns) {
                    return Err(ValidationError {
                        code: "FORBIDDEN_PATTERN".to_string(),
                        message: format!("Forbidden pattern '{}' in where value", pattern),
                        context: Some(json!({"pattern": pattern, "column": key})),
                    });
                }
            }
        }
        _ => {
            return Err(ValidationError {
                code: "INVALID_WHERE".to_string(),
                message: "Where clause must be an object".to_string(),
                context: None,
            });
        }
    }

    Ok(())
}

/// Check for forbidden patterns in a value
fn check_forbidden_patterns(value: &Value, patterns: &[String]) -> Result<(), String> {
    let text = value.to_string().to_uppercase();

    for pattern in patterns {
        if text.contains(&pattern.to_uppercase()) {
            return Err(pattern.clone());
        }
    }

    // Recursively check nested values
    match value {
        Value::Object(map) => {
            for v in map.values() {
                check_forbidden_patterns(v, patterns)?;
            }
        }
        Value::Array(arr) => {
            for v in arr {
                check_forbidden_patterns(v, patterns)?;
            }
        }
        _ => {}
    }

    Ok(())
}

/// Validate an identifier (table/column name)
fn is_valid_identifier(name: &str) -> bool {
    if name.is_empty() || name.len() > 128 {
        return false;
    }

    // Must start with letter or underscore
    let first = name.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }

    // Only allow alphanumeric and underscore
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }

    // Check against SQL keywords
    let upper = name.to_uppercase();
    let keywords: HashSet<&str> = [
        "SELECT",
        "FROM",
        "WHERE",
        "INSERT",
        "UPDATE",
        "DELETE",
        "DROP",
        "CREATE",
        "ALTER",
        "TABLE",
        "INDEX",
        "VIEW",
        "TRIGGER",
        "FUNCTION",
        "PROCEDURE",
        "DATABASE",
        "SCHEMA",
        "GRANT",
        "REVOKE",
        "UNION",
        "JOIN",
        "LEFT",
        "RIGHT",
        "INNER",
        "OUTER",
        "ON",
        "AS",
        "AND",
        "OR",
        "NOT",
        "NULL",
        "TRUE",
        "FALSE",
        "IS",
        "IN",
        "LIKE",
        "BETWEEN",
        "EXISTS",
        "CASE",
        "WHEN",
        "THEN",
        "ELSE",
        "END",
        "LIMIT",
        "OFFSET",
        "ORDER",
        "BY",
        "GROUP",
        "HAVING",
        "DISTINCT",
        "ALL",
        "ANY",
    ]
    .into_iter()
    .collect();

    !keywords.contains(upper.as_str())
}

/// Simple glob pattern matching
fn matches_glob(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if let Some(prefix) = pattern.strip_suffix("*") {
        return text.starts_with(prefix);
    }

    if let Some(suffix) = pattern.strip_prefix("*") {
        return text.ends_with(suffix);
    }

    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            return text.starts_with(parts[0]) && text.ends_with(parts[1]);
        }
    }

    text == pattern
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
    Bool {
        title: I18nText,
        description: I18nText,
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
struct DescribePayload {
    provider: String,
    world: String,
    operations: Vec<OperationDescriptor>,
    input_schema: SchemaIr,
    output_schema: SchemaIr,
    config_schema: SchemaIr,
    redactions: Vec<serde_json::Value>,
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
    let input_schema = input_schema();
    let output_schema = output_schema();
    let config_schema = config_schema();

    DescribePayload {
        provider: COMPONENT_ID.to_string(),
        world: WORLD_ID.to_string(),
        operations: vec![
            op(
                "validate",
                "chat2data-validator.op.validate.title",
                "chat2data-validator.op.validate.description",
            ),
            op(
                "load_whitelist",
                "chat2data-validator.op.load_whitelist.title",
                "chat2data-validator.op.load_whitelist.description",
            ),
        ],
        input_schema: input_schema.clone(),
        output_schema: output_schema.clone(),
        config_schema: config_schema.clone(),
        redactions: vec![],
        schema_hash: "chat2data-validator-schema-v1".to_string(),
    }
}

fn build_qa_spec(mode: &str) -> QaSpec {
    match mode {
        "default" => QaSpec {
            mode: "default".to_string(),
            title: i18n("chat2data-validator.qa.default.title"),
            description: Some(i18n("chat2data-validator.qa.default.description")),
            questions: Vec::new(),
            defaults: json!({}),
        },
        "setup" => QaSpec {
            mode: "setup".to_string(),
            title: i18n("chat2data-validator.qa.setup.title"),
            description: Some(i18n("chat2data-validator.qa.setup.description")),
            questions: vec![QaQuestionSpec {
                id: "strict_mode".to_string(),
                label: i18n("chat2data-validator.schema.config.strict_mode.title"),
                help: None,
                error: None,
                kind: "boolean".to_string(),
                required: false,
                default: Some(json!(true)),
            }],
            defaults: json!({
                "strict_mode": true,
            }),
        },
        "update" => QaSpec {
            mode: "update".to_string(),
            title: i18n("chat2data-validator.qa.update.title"),
            description: Some(i18n("chat2data-validator.qa.update.description")),
            questions: Vec::new(),
            defaults: json!({}),
        },
        "remove" => QaSpec {
            mode: "remove".to_string(),
            title: i18n("chat2data-validator.qa.remove.title"),
            description: Some(i18n("chat2data-validator.qa.remove.description")),
            questions: Vec::new(),
            defaults: json!({}),
        },
        _ => QaSpec {
            mode: "default".to_string(),
            title: i18n("chat2data-validator.qa.default.title"),
            description: Some(i18n("chat2data-validator.qa.default.description")),
            questions: Vec::new(),
            defaults: json!({}),
        },
    }
}

fn canonical_qa_spec(mode: &str) -> ComponentQaSpec {
    qa_spec_to_canonical(&build_qa_spec(mode))
}

fn qa_spec_to_canonical(spec: &QaSpec) -> ComponentQaSpec {
    ComponentQaSpec {
        mode: qa_mode_to_canonical(&spec.mode),
        title: i18n_to_canonical(&spec.title),
        description: spec.description.as_ref().map(i18n_to_canonical),
        questions: spec
            .questions
            .iter()
            .map(qa_question_to_canonical)
            .collect(),
        defaults: serde_json::from_value(spec.defaults.clone()).unwrap_or_default(),
    }
}

fn qa_mode_to_canonical(mode: &str) -> CanonicalQaMode {
    match mode {
        "setup" => CanonicalQaMode::Setup,
        "update" => CanonicalQaMode::Update,
        "remove" => CanonicalQaMode::Remove,
        _ => CanonicalQaMode::Default,
    }
}

fn qa_question_to_canonical(question: &QaQuestionSpec) -> CanonicalQuestion {
    CanonicalQuestion {
        id: question.id.clone(),
        label: i18n_to_canonical(&question.label),
        help: question.help.as_ref().map(i18n_to_canonical),
        error: question.error.as_ref().map(i18n_to_canonical),
        kind: qa_kind_to_canonical(&question.kind),
        required: question.required,
        default: question
            .default
            .clone()
            .and_then(|value| serde_json::from_value(value).ok()),
        skip_if: None,
    }
}

fn qa_kind_to_canonical(kind: &str) -> CanonicalQuestionKind {
    match kind {
        "number" | "int" | "integer" | "float" => CanonicalQuestionKind::Number,
        "bool" | "boolean" => CanonicalQuestionKind::Bool,
        _ => CanonicalQuestionKind::Text,
    }
}

fn i18n_to_canonical(text: &I18nText) -> CanonicalI18nText {
    CanonicalI18nText::new(text.key.clone(), None)
}

fn input_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "intent".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Object {
                title: i18n("chat2data-validator.schema.input.intent.title"),
                description: i18n("chat2data-validator.schema.input.intent.description"),
                fields: BTreeMap::new(),
                additional_properties: true,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("chat2data-validator.schema.input.title"),
        description: i18n("chat2data-validator.schema.input.description"),
        fields,
        additional_properties: true,
    }
}

fn output_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "valid".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Bool {
                title: i18n("chat2data-validator.schema.output.valid.title"),
                description: i18n("chat2data-validator.schema.output.valid.description"),
            },
        },
    );

    SchemaIr::Object {
        title: i18n("chat2data-validator.schema.output.title"),
        description: i18n("chat2data-validator.schema.output.description"),
        fields,
        additional_properties: true,
    }
}

fn config_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "whitelist".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::Object {
                title: i18n("chat2data-validator.schema.config.whitelist.title"),
                description: i18n("chat2data-validator.schema.config.whitelist.description"),
                fields: BTreeMap::new(),
                additional_properties: true,
            },
        },
    );
    fields.insert(
        "strict_mode".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::Bool {
                title: i18n("chat2data-validator.schema.config.strict_mode.title"),
                description: i18n("chat2data-validator.schema.config.strict_mode.description"),
            },
        },
    );

    SchemaIr::Object {
        title: i18n("chat2data-validator.schema.config.title"),
        description: i18n("chat2data-validator.schema.config.description"),
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
    canonical::to_canonical_cbor(value).unwrap_or_default()
}

fn decode_cbor(bytes: &[u8]) -> Result<Value, String> {
    canonical::from_cbor(bytes).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
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
            flow_id: "chat2data-validator".into(),
            node_id: None,
            provider: "chat2data-validator".into(),
            start_ms: None,
            end_ms: None,
        };
        let fields = [("event".to_string(), event.to_string())];
        let _ = logger_api::log(&span, &fields, None);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn log_event(_event: &str) {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_whitelist() -> Whitelist {
        let mut tables = BTreeMap::new();
        tables.insert(
            "users".to_string(),
            TableConfig {
                allowed_columns: vec!["id".to_string(), "name".to_string(), "email".to_string()],
                forbidden_columns: vec!["password_hash".to_string(), "api_key".to_string()],
                max_rows: 100,
                description: Some("User accounts".to_string()),
            },
        );
        tables.insert(
            "orders".to_string(),
            TableConfig {
                allowed_columns: vec!["*".to_string()],
                forbidden_columns: vec!["payment_token".to_string()],
                max_rows: 500,
                description: None,
            },
        );

        Whitelist {
            version: "1.0".to_string(),
            sqlite: SqliteConfig {
                allowed_tables: tables,
                allowed_operations: vec!["select".to_string(), "count".to_string()],
                forbidden_patterns: vec![],
            },
            github: GitHubConfig {
                allowed_repos: vec!["org/*".to_string()],
                forbidden_repos: vec!["org/secrets".to_string()],
                allowed_operations: default_github_operations(),
            },
            mcp: McpConfig {
                allowed_tools: vec!["read_file".to_string(), "search_files".to_string()],
                tool_restrictions: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn test_valid_sqlite_intent() {
        let whitelist = sample_whitelist();
        let intent = Intent {
            target: "sqlite".to_string(),
            action: "select".to_string(),
            params: json!({
                "table": "users",
                "columns": ["id", "name"]
            }),
            renderer: "table".to_string(),
            renderer_options: json!({}),
        };

        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let result = validate_sqlite_intent(&intent, &whitelist.sqlite, &mut errors, &mut warnings);

        assert!(result.is_some());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_forbidden_table() {
        let whitelist = sample_whitelist();
        let intent = Intent {
            target: "sqlite".to_string(),
            action: "select".to_string(),
            params: json!({
                "table": "secrets",
                "columns": ["*"]
            }),
            renderer: "table".to_string(),
            renderer_options: json!({}),
        };

        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let result = validate_sqlite_intent(&intent, &whitelist.sqlite, &mut errors, &mut warnings);

        assert!(result.is_none());
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "TABLE_NOT_ALLOWED");
    }

    #[test]
    fn test_forbidden_column() {
        let whitelist = sample_whitelist();
        let intent = Intent {
            target: "sqlite".to_string(),
            action: "select".to_string(),
            params: json!({
                "table": "users",
                "columns": ["id", "password_hash"]
            }),
            renderer: "table".to_string(),
            renderer_options: json!({}),
        };

        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let result = validate_sqlite_intent(&intent, &whitelist.sqlite, &mut errors, &mut warnings);

        assert!(result.is_none());
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "COLUMN_FORBIDDEN");
    }

    #[test]
    fn test_sql_injection_table_name() {
        let whitelist = sample_whitelist();
        let intent = Intent {
            target: "sqlite".to_string(),
            action: "select".to_string(),
            params: json!({
                "table": "users; DROP TABLE users;--",
                "columns": ["id"]
            }),
            renderer: "table".to_string(),
            renderer_options: json!({}),
        };

        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let result = validate_sqlite_intent(&intent, &whitelist.sqlite, &mut errors, &mut warnings);

        assert!(result.is_none());
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "INVALID_IDENTIFIER");
    }

    #[test]
    fn test_forbidden_pattern_in_where() {
        let whitelist = sample_whitelist();
        let intent = Intent {
            target: "sqlite".to_string(),
            action: "select".to_string(),
            params: json!({
                "table": "users",
                "columns": ["id"],
                "where": {
                    "name": "admin' UNION SELECT * FROM secrets--"
                }
            }),
            renderer: "table".to_string(),
            renderer_options: json!({}),
        };

        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let result = validate_sqlite_intent(&intent, &whitelist.sqlite, &mut errors, &mut warnings);

        assert!(result.is_none());
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "FORBIDDEN_PATTERN");
    }

    #[test]
    fn test_github_allowed_repo() {
        let whitelist = sample_whitelist();
        let intent = Intent {
            target: "github".to_string(),
            action: "list_issues".to_string(),
            params: json!({
                "repo": "org/my-repo"
            }),
            renderer: "list".to_string(),
            renderer_options: json!({}),
        };

        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let result = validate_github_intent(&intent, &whitelist.github, &mut errors, &mut warnings);

        assert!(result.is_some());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_github_forbidden_repo() {
        let whitelist = sample_whitelist();
        let intent = Intent {
            target: "github".to_string(),
            action: "list_issues".to_string(),
            params: json!({
                "repo": "org/secrets"
            }),
            renderer: "list".to_string(),
            renderer_options: json!({}),
        };

        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let result = validate_github_intent(&intent, &whitelist.github, &mut errors, &mut warnings);

        assert!(result.is_none());
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "REPO_FORBIDDEN");
    }

    #[test]
    fn test_is_valid_identifier() {
        assert!(is_valid_identifier("users"));
        assert!(is_valid_identifier("user_accounts"));
        assert!(is_valid_identifier("_private"));
        assert!(is_valid_identifier("Table123"));

        assert!(!is_valid_identifier("")); // empty
        assert!(!is_valid_identifier("123abc")); // starts with number
        assert!(!is_valid_identifier("user-name")); // contains hyphen
        assert!(!is_valid_identifier("user.name")); // contains dot
        assert!(!is_valid_identifier("SELECT")); // SQL keyword
        assert!(!is_valid_identifier("table; DROP")); // injection
    }

    #[test]
    fn test_matches_glob() {
        assert!(matches_glob("org/repo", "org/*"));
        assert!(matches_glob("org/any-repo", "org/*"));
        assert!(!matches_glob("other/repo", "org/*"));

        assert!(matches_glob("file.txt", "*.txt"));
        assert!(!matches_glob("file.md", "*.txt"));

        assert!(matches_glob("exact-match", "exact-match"));
        assert!(!matches_glob("not-match", "exact-match"));
    }

    #[test]
    fn test_action_not_allowed() {
        let whitelist = sample_whitelist();
        let intent = Intent {
            target: "sqlite".to_string(),
            action: "delete".to_string(), // Not in allowed_operations
            params: json!({
                "table": "users"
            }),
            renderer: "table".to_string(),
            renderer_options: json!({}),
        };

        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let result = validate_sqlite_intent(&intent, &whitelist.sqlite, &mut errors, &mut warnings);

        assert!(result.is_none());
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "ACTION_NOT_ALLOWED");
    }
}
