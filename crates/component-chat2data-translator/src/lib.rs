#![allow(unsafe_op_in_unsafe_fn)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::unnecessary_lazy_evaluations)]

//! Chat2Data Translator Component
//!
//! This component translates validated QueryIntent into executable queries using
//! deterministic, parameterized query building. NO LLM is involved in this process,
//! ensuring predictable and secure query generation.
//!
//! # Operations
//!
//! - `translate` - Translate validated intent to executable query
//! - `translate_batch` - Translate multiple intents
//!
//! # Supported Targets
//!
//! - `sqlite` - Generates parameterized SQL queries
//! - `github` - Generates GitHub API request specifications
//! - `mcp` - Generates MCP tool call specifications
//!
//! # Security Features
//!
//! - Parameterized queries (SQL injection prevention)
//! - Identifier validation (table/column names)
//! - Type-safe parameter binding
//! - No dynamic SQL construction

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[allow(clippy::too_many_arguments)]
mod bindings {
    wit_bindgen::generate!({ path: "wit/chat2data-translator", world: "component-v0-v6-v0", generate_all });
}

#[cfg(not(test))]
use bindings::greentic::telemetry::logger_api;

const COMPONENT_ID: &str = "chat2data-translator";
const WORLD_ID: &str = "component-v0-v6-v0";

const I18N_KEYS: &[&str] = &[
    "chat2data-translator.op.translate.title",
    "chat2data-translator.op.translate.description",
    "chat2data-translator.op.translate_batch.title",
    "chat2data-translator.op.translate_batch.description",
    "chat2data-translator.schema.input.title",
    "chat2data-translator.schema.input.description",
    "chat2data-translator.schema.output.title",
    "chat2data-translator.schema.output.description",
    "chat2data-translator.schema.config.title",
    "chat2data-translator.schema.config.description",
    "chat2data-translator.qa.default.title",
    "chat2data-translator.qa.setup.title",
];

// ============================================================================
// Data Structures
// ============================================================================

/// Component configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ComponentConfig {
    /// Default row limit if not specified
    #[serde(default = "default_max_rows")]
    default_max_rows: usize,
    /// GitHub API base URL
    #[serde(default = "default_github_api_url")]
    github_api_url: String,
}

/// Validated intent from validator component
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ValidatedIntent {
    /// Original intent
    intent: Intent,
    /// Enforced maximum rows
    max_rows: usize,
    /// Sanitized columns
    #[serde(default)]
    sanitized_columns: Option<Vec<String>>,
}

/// Intent details
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

/// Translated query output
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
    /// Query hash for audit logging
    query_hash: String,
}

/// Query specification (type-specific)
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

/// SQL parameter with type information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SqlParam {
    Null,
    Integer { value: i64 },
    Real { value: f64 },
    Text { value: String },
    Blob { value: Vec<u8> },
}

/// Translation error
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TranslationError {
    code: String,
    message: String,
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

fn default_max_rows() -> usize {
    1000
}

fn default_renderer() -> String {
    "auto".to_string()
}

fn default_github_api_url() -> String {
    "https://api.github.com".to_string()
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
            "translate" => handle_translate(&input),
            "translate_batch" => handle_translate_batch(&input),
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
                default_max_rows: answers
                    .get("default_max_rows")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize)
                    .unwrap_or(default_max_rows()),
                github_api_url: answers
                    .get("github_api_url")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_else(default_github_api_url),
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
// Core Translation Logic
// ============================================================================

/// Handle translate operation
fn handle_translate(input: &Value) -> Value {
    let cfg = load_config(input).unwrap_or_default();

    // Extract validated intent
    let validated_intent: ValidatedIntent = match input
        .get("validated_intent")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(vi) => vi,
        None => {
            return json!({
                "ok": false,
                "error": "missing or invalid validated_intent"
            });
        }
    };

    // Translate based on target
    match translate_intent(&validated_intent, &cfg) {
        Ok(query) => {
            log_event("translate_success");
            json!({
                "ok": true,
                "query": query,
            })
        }
        Err(err) => {
            log_event("translate_error");
            json!({
                "ok": false,
                "error": err.message,
                "code": err.code,
                "context": err.context,
            })
        }
    }
}

/// Handle batch translate operation
fn handle_translate_batch(input: &Value) -> Value {
    let cfg = load_config(input).unwrap_or_default();

    let intents: Vec<ValidatedIntent> = match input
        .get("validated_intents")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(vi) => vi,
        None => {
            return json!({
                "ok": false,
                "error": "missing or invalid validated_intents array"
            });
        }
    };

    let mut queries = Vec::new();
    let mut errors = Vec::new();

    for (index, intent) in intents.iter().enumerate() {
        match translate_intent(intent, &cfg) {
            Ok(query) => queries.push(query),
            Err(err) => errors.push(json!({
                "index": index,
                "error": err,
            })),
        }
    }

    json!({
        "ok": errors.is_empty(),
        "queries": queries,
        "errors": errors,
    })
}

/// Translate a validated intent to a query
fn translate_intent(
    validated_intent: &ValidatedIntent,
    cfg: &ComponentConfig,
) -> Result<TranslatedQuery, TranslationError> {
    let intent = &validated_intent.intent;

    let query = match intent.target.as_str() {
        "sqlite" => translate_sqlite(intent, validated_intent, cfg)?,
        "github" => translate_github(intent, cfg)?,
        "mcp" => translate_mcp(intent)?,
        other => {
            return Err(TranslationError {
                code: "UNSUPPORTED_TARGET".to_string(),
                message: format!("Target '{}' is not supported", other),
                context: None,
            });
        }
    };

    // Generate query hash for audit
    let query_hash = hash_query(&query);

    Ok(TranslatedQuery {
        target: intent.target.clone(),
        query_type: match &query {
            QuerySpec::Sql { .. } => "sql".to_string(),
            QuerySpec::Http { .. } => "http".to_string(),
            QuerySpec::Mcp { .. } => "mcp".to_string(),
        },
        query,
        renderer: intent.renderer.clone(),
        renderer_options: intent.renderer_options.clone(),
        max_rows: validated_intent.max_rows,
        query_hash,
    })
}

/// Translate SQLite intent to parameterized SQL
fn translate_sqlite(
    intent: &Intent,
    validated_intent: &ValidatedIntent,
    _cfg: &ComponentConfig,
) -> Result<QuerySpec, TranslationError> {
    let params = &intent.params;

    match intent.action.as_str() {
        "select" => build_select_query(params, validated_intent),
        "count" => build_count_query(params),
        "aggregate" => build_aggregate_query(params, validated_intent),
        other => Err(TranslationError {
            code: "UNSUPPORTED_ACTION".to_string(),
            message: format!("SQLite action '{}' is not supported", other),
            context: None,
        }),
    }
}

/// Build SELECT query with parameterized values
fn build_select_query(
    params: &Value,
    validated_intent: &ValidatedIntent,
) -> Result<QuerySpec, TranslationError> {
    let mut sql = String::new();
    let mut sql_params = Vec::new();
    let mut param_names = Vec::new();

    // Extract table
    let table = params
        .get("table")
        .and_then(Value::as_str)
        .ok_or_else(|| TranslationError {
            code: "MISSING_TABLE".to_string(),
            message: "Missing table parameter".to_string(),
            context: None,
        })?;

    // Use sanitized columns if available, otherwise from params
    let columns = if let Some(ref sanitized) = validated_intent.sanitized_columns {
        if sanitized.is_empty() {
            vec!["*".to_string()]
        } else {
            sanitized.clone()
        }
    } else {
        params
            .get("columns")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_else(|| vec!["*".to_string()])
    };

    // Build SELECT clause
    sql.push_str("SELECT ");
    sql.push_str(&columns.join(", "));
    sql.push_str(" FROM ");
    sql.push_str(table);

    // Build WHERE clause with parameterized values
    if let Some(where_clause) = params.get("where") {
        if let Value::Object(conditions) = where_clause {
            if !conditions.is_empty() {
                sql.push_str(" WHERE ");
                let mut first = true;

                for (column, condition) in conditions {
                    if !first {
                        sql.push_str(" AND ");
                    }
                    first = false;

                    build_condition(
                        &mut sql,
                        &mut sql_params,
                        &mut param_names,
                        column,
                        condition,
                    )?;
                }
            }
        }
    }

    // Build ORDER BY clause
    if let Some(order_by) = params.get("order_by").and_then(Value::as_array) {
        if !order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            let orders: Vec<String> = order_by
                .iter()
                .filter_map(|item| {
                    let col = item.get("column").and_then(Value::as_str)?;
                    let dir = item
                        .get("direction")
                        .and_then(Value::as_str)
                        .unwrap_or("ASC")
                        .to_uppercase();
                    // Validate direction
                    let dir = if dir == "DESC" { "DESC" } else { "ASC" };
                    Some(format!("{} {}", col, dir))
                })
                .collect();
            sql.push_str(&orders.join(", "));
        }
    }

    // Add LIMIT
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .unwrap_or(validated_intent.max_rows)
        .min(validated_intent.max_rows);

    sql.push_str(&format!(" LIMIT {}", limit));

    // Add OFFSET if present
    if let Some(offset) = params.get("offset").and_then(Value::as_u64) {
        sql.push_str(&format!(" OFFSET {}", offset));
    }

    Ok(QuerySpec::Sql {
        sql,
        params: sql_params,
        param_names,
    })
}

/// Build a WHERE condition with parameterized value
fn build_condition(
    sql: &mut String,
    params: &mut Vec<SqlParam>,
    param_names: &mut Vec<String>,
    column: &str,
    condition: &Value,
) -> Result<(), TranslationError> {
    let param_index = params.len() + 1;

    match condition {
        // Simple equality: {"column": "value"}
        Value::String(s) => {
            sql.push_str(&format!("{} = ?{}", column, param_index));
            params.push(SqlParam::Text { value: s.clone() });
            param_names.push(column.to_string());
        }
        Value::Number(n) => {
            sql.push_str(&format!("{} = ?{}", column, param_index));
            if let Some(i) = n.as_i64() {
                params.push(SqlParam::Integer { value: i });
            } else if let Some(f) = n.as_f64() {
                params.push(SqlParam::Real { value: f });
            }
            param_names.push(column.to_string());
        }
        Value::Bool(b) => {
            sql.push_str(&format!("{} = ?{}", column, param_index));
            params.push(SqlParam::Integer {
                value: if *b { 1 } else { 0 },
            });
            param_names.push(column.to_string());
        }
        Value::Null => {
            sql.push_str(&format!("{} IS NULL", column));
        }
        // Operator object: {"column": {"$gt": 10}}
        Value::Object(ops) => {
            let mut first = true;
            for (op, value) in ops {
                if !first {
                    sql.push_str(" AND ");
                }
                first = false;

                let param_idx = params.len() + 1;
                match op.as_str() {
                    "$eq" => {
                        sql.push_str(&format!("{} = ?{}", column, param_idx));
                        params.push(value_to_param(value));
                        param_names.push(format!("{}.$eq", column));
                    }
                    "$ne" => {
                        sql.push_str(&format!("{} != ?{}", column, param_idx));
                        params.push(value_to_param(value));
                        param_names.push(format!("{}.$ne", column));
                    }
                    "$gt" => {
                        sql.push_str(&format!("{} > ?{}", column, param_idx));
                        params.push(value_to_param(value));
                        param_names.push(format!("{}.$gt", column));
                    }
                    "$gte" => {
                        sql.push_str(&format!("{} >= ?{}", column, param_idx));
                        params.push(value_to_param(value));
                        param_names.push(format!("{}.$gte", column));
                    }
                    "$lt" => {
                        sql.push_str(&format!("{} < ?{}", column, param_idx));
                        params.push(value_to_param(value));
                        param_names.push(format!("{}.$lt", column));
                    }
                    "$lte" => {
                        sql.push_str(&format!("{} <= ?{}", column, param_idx));
                        params.push(value_to_param(value));
                        param_names.push(format!("{}.$lte", column));
                    }
                    "$like" => {
                        sql.push_str(&format!("{} LIKE ?{}", column, param_idx));
                        params.push(value_to_param(value));
                        param_names.push(format!("{}.$like", column));
                    }
                    "$in" => {
                        if let Value::Array(arr) = value {
                            let placeholders: Vec<String> = (0..arr.len())
                                .map(|i| format!("?{}", params.len() + i + 1))
                                .collect();
                            sql.push_str(&format!("{} IN ({})", column, placeholders.join(", ")));
                            for v in arr {
                                params.push(value_to_param(v));
                                param_names.push(format!("{}.$in[{}]", column, params.len() - 1));
                            }
                        }
                    }
                    "$null" => {
                        if value.as_bool().unwrap_or(false) {
                            sql.push_str(&format!("{} IS NULL", column));
                        } else {
                            sql.push_str(&format!("{} IS NOT NULL", column));
                        }
                    }
                    "$between" => {
                        if let Value::Array(arr) = value {
                            if arr.len() == 2 {
                                let idx1 = params.len() + 1;
                                let idx2 = params.len() + 2;
                                sql.push_str(&format!(
                                    "{} BETWEEN ?{} AND ?{}",
                                    column, idx1, idx2
                                ));
                                params.push(value_to_param(&arr[0]));
                                param_names.push(format!("{}.$between.low", column));
                                params.push(value_to_param(&arr[1]));
                                param_names.push(format!("{}.$between.high", column));
                            }
                        }
                    }
                    _ => {
                        return Err(TranslationError {
                            code: "UNSUPPORTED_OPERATOR".to_string(),
                            message: format!("Operator '{}' is not supported", op),
                            context: Some(json!({"column": column, "operator": op})),
                        });
                    }
                }
            }
        }
        _ => {
            return Err(TranslationError {
                code: "INVALID_CONDITION".to_string(),
                message: "Invalid condition value".to_string(),
                context: Some(json!({"column": column})),
            });
        }
    }

    Ok(())
}

/// Build COUNT query
fn build_count_query(params: &Value) -> Result<QuerySpec, TranslationError> {
    let table = params
        .get("table")
        .and_then(Value::as_str)
        .ok_or_else(|| TranslationError {
            code: "MISSING_TABLE".to_string(),
            message: "Missing table parameter".to_string(),
            context: None,
        })?;

    let mut sql = format!("SELECT COUNT(*) as count FROM {}", table);
    let mut sql_params = Vec::new();
    let mut param_names = Vec::new();

    // Build WHERE clause
    if let Some(where_clause) = params.get("where") {
        if let Value::Object(conditions) = where_clause {
            if !conditions.is_empty() {
                sql.push_str(" WHERE ");
                let mut first = true;

                for (column, condition) in conditions {
                    if !first {
                        sql.push_str(" AND ");
                    }
                    first = false;

                    build_condition(
                        &mut sql,
                        &mut sql_params,
                        &mut param_names,
                        column,
                        condition,
                    )?;
                }
            }
        }
    }

    Ok(QuerySpec::Sql {
        sql,
        params: sql_params,
        param_names,
    })
}

/// Build aggregate query (SUM, AVG, MIN, MAX, etc.)
fn build_aggregate_query(
    params: &Value,
    validated_intent: &ValidatedIntent,
) -> Result<QuerySpec, TranslationError> {
    let table = params
        .get("table")
        .and_then(Value::as_str)
        .ok_or_else(|| TranslationError {
            code: "MISSING_TABLE".to_string(),
            message: "Missing table parameter".to_string(),
            context: None,
        })?;

    let aggregates = params
        .get("aggregates")
        .and_then(Value::as_array)
        .ok_or_else(|| TranslationError {
            code: "MISSING_AGGREGATES".to_string(),
            message: "Missing aggregates array".to_string(),
            context: None,
        })?;

    // Build aggregate functions
    let mut select_parts = Vec::new();
    for agg in aggregates {
        let func = agg
            .get("function")
            .and_then(Value::as_str)
            .unwrap_or("COUNT");
        let column = agg.get("column").and_then(Value::as_str).unwrap_or("*");
        let alias = agg
            .get("alias")
            .and_then(Value::as_str)
            .unwrap_or_else(|| func);

        // Validate function
        let valid_functions = ["COUNT", "SUM", "AVG", "MIN", "MAX"];
        let func_upper = func.to_uppercase();
        if !valid_functions.contains(&func_upper.as_str()) {
            return Err(TranslationError {
                code: "INVALID_AGGREGATE".to_string(),
                message: format!("Aggregate function '{}' is not supported", func),
                context: None,
            });
        }

        select_parts.push(format!("{}({}) as {}", func_upper, column, alias));
    }

    // Group by
    if let Some(group_by) = params.get("group_by").and_then(Value::as_array) {
        for col in group_by {
            if let Some(c) = col.as_str() {
                select_parts.push(c.to_string());
            }
        }
    }

    let mut sql = format!("SELECT {} FROM {}", select_parts.join(", "), table);
    let mut sql_params = Vec::new();
    let mut param_names = Vec::new();

    // WHERE clause
    if let Some(where_clause) = params.get("where") {
        if let Value::Object(conditions) = where_clause {
            if !conditions.is_empty() {
                sql.push_str(" WHERE ");
                let mut first = true;

                for (column, condition) in conditions {
                    if !first {
                        sql.push_str(" AND ");
                    }
                    first = false;

                    build_condition(
                        &mut sql,
                        &mut sql_params,
                        &mut param_names,
                        column,
                        condition,
                    )?;
                }
            }
        }
    }

    // GROUP BY clause
    if let Some(group_by) = params.get("group_by").and_then(Value::as_array) {
        if !group_by.is_empty() {
            let cols: Vec<&str> = group_by.iter().filter_map(Value::as_str).collect();
            sql.push_str(&format!(" GROUP BY {}", cols.join(", ")));
        }
    }

    // HAVING clause
    if let Some(having) = params.get("having") {
        if let Value::Object(conditions) = having {
            if !conditions.is_empty() {
                sql.push_str(" HAVING ");
                let mut first = true;

                for (column, condition) in conditions {
                    if !first {
                        sql.push_str(" AND ");
                    }
                    first = false;

                    build_condition(
                        &mut sql,
                        &mut sql_params,
                        &mut param_names,
                        column,
                        condition,
                    )?;
                }
            }
        }
    }

    // LIMIT
    sql.push_str(&format!(" LIMIT {}", validated_intent.max_rows));

    Ok(QuerySpec::Sql {
        sql,
        params: sql_params,
        param_names,
    })
}

/// Translate GitHub intent to HTTP request
fn translate_github(intent: &Intent, cfg: &ComponentConfig) -> Result<QuerySpec, TranslationError> {
    let params = &intent.params;

    let (method, path, query_params) =
        match intent.action.as_str() {
            "list_issues" => {
                let repo =
                    params
                        .get("repo")
                        .and_then(Value::as_str)
                        .ok_or_else(|| TranslationError {
                            code: "MISSING_REPO".to_string(),
                            message: "Missing repo parameter".to_string(),
                            context: None,
                        })?;

                let mut qp = BTreeMap::new();
                if let Some(state) = params.get("state").and_then(Value::as_str) {
                    qp.insert("state".to_string(), state.to_string());
                }
                if let Some(assignee) = params.get("assignee").and_then(Value::as_str) {
                    qp.insert("assignee".to_string(), assignee.to_string());
                }
                if let Some(labels) = params.get("labels").and_then(Value::as_array) {
                    let labels_str: Vec<&str> = labels.iter().filter_map(Value::as_str).collect();
                    qp.insert("labels".to_string(), labels_str.join(","));
                }
                qp.insert("per_page".to_string(), "100".to_string());

                ("GET", format!("/repos/{}/issues", repo), qp)
            }
            "get_issue" => {
                let repo =
                    params
                        .get("repo")
                        .and_then(Value::as_str)
                        .ok_or_else(|| TranslationError {
                            code: "MISSING_REPO".to_string(),
                            message: "Missing repo parameter".to_string(),
                            context: None,
                        })?;
                let number = params
                    .get("number")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| TranslationError {
                        code: "MISSING_NUMBER".to_string(),
                        message: "Missing issue number".to_string(),
                        context: None,
                    })?;

                (
                    "GET",
                    format!("/repos/{}/issues/{}", repo, number),
                    BTreeMap::new(),
                )
            }
            "list_prs" => {
                let repo =
                    params
                        .get("repo")
                        .and_then(Value::as_str)
                        .ok_or_else(|| TranslationError {
                            code: "MISSING_REPO".to_string(),
                            message: "Missing repo parameter".to_string(),
                            context: None,
                        })?;

                let mut qp = BTreeMap::new();
                if let Some(state) = params.get("state").and_then(Value::as_str) {
                    qp.insert("state".to_string(), state.to_string());
                }
                qp.insert("per_page".to_string(), "100".to_string());

                ("GET", format!("/repos/{}/pulls", repo), qp)
            }
            "get_pr" => {
                let repo =
                    params
                        .get("repo")
                        .and_then(Value::as_str)
                        .ok_or_else(|| TranslationError {
                            code: "MISSING_REPO".to_string(),
                            message: "Missing repo parameter".to_string(),
                            context: None,
                        })?;
                let number = params
                    .get("number")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| TranslationError {
                        code: "MISSING_NUMBER".to_string(),
                        message: "Missing PR number".to_string(),
                        context: None,
                    })?;

                (
                    "GET",
                    format!("/repos/{}/pulls/{}", repo, number),
                    BTreeMap::new(),
                )
            }
            "search_code" => {
                let query = params.get("query").and_then(Value::as_str).ok_or_else(|| {
                    TranslationError {
                        code: "MISSING_QUERY".to_string(),
                        message: "Missing search query".to_string(),
                        context: None,
                    }
                })?;

                let mut full_query = query.to_string();
                if let Some(repo) = params.get("repo").and_then(Value::as_str) {
                    full_query.push_str(&format!(" repo:{}", repo));
                }
                if let Some(path) = params.get("path").and_then(Value::as_str) {
                    full_query.push_str(&format!(" path:{}", path));
                }

                let mut qp = BTreeMap::new();
                qp.insert("q".to_string(), full_query);
                qp.insert("per_page".to_string(), "100".to_string());

                ("GET", "/search/code".to_string(), qp)
            }
            "list_commits" => {
                let repo =
                    params
                        .get("repo")
                        .and_then(Value::as_str)
                        .ok_or_else(|| TranslationError {
                            code: "MISSING_REPO".to_string(),
                            message: "Missing repo parameter".to_string(),
                            context: None,
                        })?;

                let mut qp = BTreeMap::new();
                if let Some(sha) = params.get("sha").and_then(Value::as_str) {
                    qp.insert("sha".to_string(), sha.to_string());
                }
                if let Some(author) = params.get("author").and_then(Value::as_str) {
                    qp.insert("author".to_string(), author.to_string());
                }
                qp.insert("per_page".to_string(), "100".to_string());

                ("GET", format!("/repos/{}/commits", repo), qp)
            }
            other => {
                return Err(TranslationError {
                    code: "UNSUPPORTED_ACTION".to_string(),
                    message: format!("GitHub action '{}' is not supported", other),
                    context: None,
                });
            }
        };

    let mut headers = BTreeMap::new();
    headers.insert(
        "Accept".to_string(),
        "application/vnd.github+json".to_string(),
    );
    headers.insert("X-GitHub-Api-Version".to_string(), "2022-11-28".to_string());

    let _ = cfg; // cfg.github_api_url would be used in executor

    Ok(QuerySpec::Http {
        method: method.to_string(),
        path,
        query_params,
        headers,
    })
}

/// Translate MCP intent to tool call
fn translate_mcp(intent: &Intent) -> Result<QuerySpec, TranslationError> {
    Ok(QuerySpec::Mcp {
        tool_name: intent.action.clone(),
        arguments: intent.params.clone(),
    })
}

/// Convert JSON value to SQL parameter
fn value_to_param(value: &Value) -> SqlParam {
    match value {
        Value::Null => SqlParam::Null,
        Value::Bool(b) => SqlParam::Integer {
            value: if *b { 1 } else { 0 },
        },
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlParam::Integer { value: i }
            } else if let Some(f) = n.as_f64() {
                SqlParam::Real { value: f }
            } else {
                SqlParam::Null
            }
        }
        Value::String(s) => SqlParam::Text { value: s.clone() },
        _ => SqlParam::Text {
            value: value.to_string(),
        },
    }
}

/// Generate a hash for the query (for audit logging)
fn hash_query(query: &QuerySpec) -> String {
    let json_str = serde_json::to_string(query).unwrap_or_default();
    // Simple hash - in production would use blake3 or similar
    format!("{:x}", json_str.len() ^ 0xDEADBEEF)
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
    redactions: Vec<Value>,
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
    DescribePayload {
        provider: COMPONENT_ID.to_string(),
        world: WORLD_ID.to_string(),
        operations: vec![
            op(
                "translate",
                "chat2data-translator.op.translate.title",
                "chat2data-translator.op.translate.description",
            ),
            op(
                "translate_batch",
                "chat2data-translator.op.translate_batch.title",
                "chat2data-translator.op.translate_batch.description",
            ),
        ],
        input_schema: SchemaIr::Object {
            title: i18n("chat2data-translator.schema.input.title"),
            description: i18n("chat2data-translator.schema.input.description"),
            fields: BTreeMap::new(),
            additional_properties: true,
        },
        output_schema: SchemaIr::Object {
            title: i18n("chat2data-translator.schema.output.title"),
            description: i18n("chat2data-translator.schema.output.description"),
            fields: BTreeMap::new(),
            additional_properties: true,
        },
        config_schema: SchemaIr::Object {
            title: i18n("chat2data-translator.schema.config.title"),
            description: i18n("chat2data-translator.schema.config.description"),
            fields: BTreeMap::new(),
            additional_properties: true,
        },
        redactions: vec![],
        schema_hash: "chat2data-translator-schema-v1".to_string(),
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
            title: i18n("chat2data-translator.qa.default.title"),
            description: None,
            questions: Vec::new(),
            defaults: json!({}),
        },
        Mode::Setup => QaSpec {
            mode: "setup".to_string(),
            title: i18n("chat2data-translator.qa.setup.title"),
            description: None,
            questions: Vec::new(),
            defaults: json!({
                "default_max_rows": 1000,
            }),
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
            flow_id: "chat2data-translator".into(),
            node_id: None,
            provider: "chat2data-translator".into(),
            start_ms: None,
            end_ms: None,
        };
        let fields = [("event".to_string(), event.to_string())];
        let _ = logger_api::log(&span, &fields, None);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_validated_intent() -> ValidatedIntent {
        ValidatedIntent {
            intent: Intent {
                target: "sqlite".to_string(),
                action: "select".to_string(),
                params: json!({
                    "table": "users",
                    "columns": ["id", "name", "email"]
                }),
                renderer: "table".to_string(),
                renderer_options: json!({}),
            },
            max_rows: 100,
            sanitized_columns: None,
        }
    }

    #[test]
    fn test_simple_select() {
        let intent = sample_validated_intent();
        let cfg = ComponentConfig::default();

        let result = translate_sqlite(&intent.intent, &intent, &cfg).unwrap();

        if let QuerySpec::Sql { sql, params, .. } = result {
            assert!(sql.contains("SELECT id, name, email FROM users"));
            assert!(sql.contains("LIMIT 100"));
            assert!(params.is_empty());
        } else {
            panic!("Expected SQL query");
        }
    }

    #[test]
    fn test_select_with_where() {
        let intent = ValidatedIntent {
            intent: Intent {
                target: "sqlite".to_string(),
                action: "select".to_string(),
                params: json!({
                    "table": "users",
                    "columns": ["id", "name"],
                    "where": {
                        "status": "active",
                        "age": {"$gt": 18}
                    }
                }),
                renderer: "table".to_string(),
                renderer_options: json!({}),
            },
            max_rows: 100,
            sanitized_columns: None,
        };
        let cfg = ComponentConfig::default();

        let result = translate_sqlite(&intent.intent, &intent, &cfg).unwrap();

        if let QuerySpec::Sql {
            sql,
            params,
            param_names,
        } = result
        {
            assert!(sql.contains("WHERE"));
            // Note: BTreeMap iterates alphabetically, so "age" comes before "status"
            assert!(sql.contains("age > ?1"));
            assert!(sql.contains("status = ?2"));
            assert_eq!(params.len(), 2);
            assert_eq!(param_names.len(), 2);
        } else {
            panic!("Expected SQL query");
        }
    }

    #[test]
    fn test_select_with_in_operator() {
        let intent = ValidatedIntent {
            intent: Intent {
                target: "sqlite".to_string(),
                action: "select".to_string(),
                params: json!({
                    "table": "users",
                    "columns": ["*"],
                    "where": {
                        "id": {"$in": [1, 2, 3]}
                    }
                }),
                renderer: "table".to_string(),
                renderer_options: json!({}),
            },
            max_rows: 100,
            sanitized_columns: None,
        };
        let cfg = ComponentConfig::default();

        let result = translate_sqlite(&intent.intent, &intent, &cfg).unwrap();

        if let QuerySpec::Sql { sql, params, .. } = result {
            assert!(sql.contains("id IN (?1, ?2, ?3)"));
            assert_eq!(params.len(), 3);
        } else {
            panic!("Expected SQL query");
        }
    }

    #[test]
    fn test_count_query() {
        let params = json!({
            "table": "orders",
            "where": {
                "status": "completed"
            }
        });

        let result = build_count_query(&params).unwrap();

        if let QuerySpec::Sql { sql, params, .. } = result {
            assert!(sql.contains("SELECT COUNT(*) as count FROM orders"));
            assert!(sql.contains("WHERE status = ?1"));
            assert_eq!(params.len(), 1);
        } else {
            panic!("Expected SQL query");
        }
    }

    #[test]
    fn test_aggregate_query() {
        let intent = ValidatedIntent {
            intent: Intent {
                target: "sqlite".to_string(),
                action: "aggregate".to_string(),
                params: json!({
                    "table": "orders",
                    "aggregates": [
                        {"function": "SUM", "column": "total", "alias": "total_sum"},
                        {"function": "COUNT", "column": "*", "alias": "order_count"}
                    ],
                    "group_by": ["status"]
                }),
                renderer: "graph".to_string(),
                renderer_options: json!({}),
            },
            max_rows: 100,
            sanitized_columns: None,
        };

        let result = build_aggregate_query(&intent.intent.params, &intent).unwrap();

        if let QuerySpec::Sql { sql, .. } = result {
            assert!(sql.contains("SUM(total) as total_sum"));
            assert!(sql.contains("COUNT(*) as order_count"));
            assert!(sql.contains("GROUP BY status"));
        } else {
            panic!("Expected SQL query");
        }
    }

    #[test]
    fn test_github_list_issues() {
        let intent = Intent {
            target: "github".to_string(),
            action: "list_issues".to_string(),
            params: json!({
                "repo": "owner/repo",
                "state": "open",
                "labels": ["bug", "critical"]
            }),
            renderer: "list".to_string(),
            renderer_options: json!({}),
        };
        let cfg = ComponentConfig::default();

        let result = translate_github(&intent, &cfg).unwrap();

        if let QuerySpec::Http {
            method,
            path,
            query_params,
            ..
        } = result
        {
            assert_eq!(method, "GET");
            assert_eq!(path, "/repos/owner/repo/issues");
            assert_eq!(query_params.get("state"), Some(&"open".to_string()));
            assert_eq!(
                query_params.get("labels"),
                Some(&"bug,critical".to_string())
            );
        } else {
            panic!("Expected HTTP query");
        }
    }

    #[test]
    fn test_mcp_translation() {
        let intent = Intent {
            target: "mcp".to_string(),
            action: "read_file".to_string(),
            params: json!({
                "path": "/src/main.rs"
            }),
            renderer: "card".to_string(),
            renderer_options: json!({}),
        };

        let result = translate_mcp(&intent).unwrap();

        if let QuerySpec::Mcp {
            tool_name,
            arguments,
        } = result
        {
            assert_eq!(tool_name, "read_file");
            assert_eq!(arguments.get("path"), Some(&json!("/src/main.rs")));
        } else {
            panic!("Expected MCP query");
        }
    }

    #[test]
    fn test_value_to_param() {
        assert!(matches!(value_to_param(&Value::Null), SqlParam::Null));
        assert!(matches!(
            value_to_param(&json!(true)),
            SqlParam::Integer { value: 1 }
        ));
        assert!(matches!(
            value_to_param(&json!(false)),
            SqlParam::Integer { value: 0 }
        ));
        assert!(matches!(
            value_to_param(&json!(42)),
            SqlParam::Integer { value: 42 }
        ));
        assert!(matches!(
            value_to_param(&json!(3.5)),
            SqlParam::Real { value: _ }
        ));
        assert!(matches!(
            value_to_param(&json!("hello")),
            SqlParam::Text { value: _ }
        ));
    }
}
