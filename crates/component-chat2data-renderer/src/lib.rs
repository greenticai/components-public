#![allow(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]
#![allow(clippy::collapsible_if)]

//! Chat2Data Renderer Component
//!
//! This component converts query execution results into AdaptiveCards for
//! display in chat interfaces. It supports multiple rendering styles based
//! on the data shape and user preferences.
//!
//! # Operations
//!
//! - `render` - Render query results to AdaptiveCard
//! - `auto_select` - Auto-select best renderer for data
//!
//! # Renderers
//!
//! - `list` - For collections (issues, users, items)
//! - `table` - For tabular data with multiple columns
//! - `card` - For single item details
//! - `graph` - For aggregated/statistical data (basic text representation)
//! - `auto` - Automatically select based on data shape

use greentic_types::cbor::canonical;
use greentic_types::i18n_text::I18nText as CanonicalI18nText;
use greentic_types::schemas::component::v0_6_0::{
    ComponentQaSpec, QaMode as CanonicalQaMode, Question as CanonicalQuestion,
    QuestionKind as CanonicalQuestionKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::component_v0_6::node;
#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::telemetry_logger as logger_api;

const COMPONENT_ID: &str = "chat2data-renderer";
const WORLD_ID: &str = "component-v0-v6-v0";
const COMPONENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const ADAPTIVE_CARD_VERSION: &str = "1.5";

const I18N_KEYS: &[&str] = &[
    "chat2data-renderer.op.render.title",
    "chat2data-renderer.op.render.description",
    "chat2data-renderer.op.auto_select.title",
    "chat2data-renderer.op.auto_select.description",
    "chat2data-renderer.schema.input.title",
    "chat2data-renderer.schema.input.description",
    "chat2data-renderer.schema.output.title",
    "chat2data-renderer.schema.output.description",
    "chat2data-renderer.schema.config.title",
    "chat2data-renderer.schema.config.description",
    "chat2data-renderer.qa.default.title",
    "chat2data-renderer.qa.default.description",
    "chat2data-renderer.qa.setup.title",
    "chat2data-renderer.qa.setup.description",
    "chat2data-renderer.qa.update.title",
    "chat2data-renderer.qa.update.description",
    "chat2data-renderer.qa.remove.title",
    "chat2data-renderer.qa.remove.description",
    "chat2data-renderer.label.no_results",
    "chat2data-renderer.label.showing_n_of_m",
    "chat2data-renderer.label.truncated",
];

// ============================================================================
// Data Structures
// ============================================================================

/// Component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComponentConfig {
    /// Maximum items to show in list view
    #[serde(default = "default_max_list_items")]
    max_list_items: usize,
    /// Maximum columns to show in table view
    #[serde(default = "default_max_table_columns")]
    max_table_columns: usize,
    /// Theme colors
    #[serde(default)]
    theme: ThemeConfig,
}

impl Default for ComponentConfig {
    fn default() -> Self {
        Self {
            max_list_items: default_max_list_items(),
            max_table_columns: default_max_table_columns(),
            theme: ThemeConfig::default(),
        }
    }
}

/// Theme configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ThemeConfig {
    /// Accent color for headers
    #[serde(default = "default_accent_color")]
    accent_color: String,
    /// Background color
    #[serde(default = "default_background_color")]
    background_color: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            accent_color: default_accent_color(),
            background_color: default_background_color(),
        }
    }
}

/// Query result data from executor
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

/// Render input
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RenderInput {
    /// Query result data
    data: QueryData,
    /// Renderer to use
    #[serde(default = "default_renderer")]
    renderer: String,
    /// Renderer options
    #[serde(default)]
    renderer_options: RendererOptions,
    /// Title for the card
    #[serde(default)]
    title: Option<String>,
}

/// Renderer-specific options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RendererOptions {
    /// Primary column for list title
    #[serde(default)]
    title_column: Option<String>,
    /// Secondary column for list subtitle
    #[serde(default)]
    subtitle_column: Option<String>,
    /// Columns to display (for table)
    #[serde(default)]
    display_columns: Vec<String>,
    /// Column to use for sorting
    #[serde(default)]
    sort_column: Option<String>,
    /// Sort direction
    #[serde(default)]
    sort_direction: Option<String>,
    /// Show row numbers
    #[serde(default)]
    show_row_numbers: bool,
    /// Compact mode
    #[serde(default)]
    compact: bool,
}

/// Render output
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RenderOutput {
    /// The AdaptiveCard JSON
    card: Value,
    /// Plain text summary
    summary: String,
    /// Renderer that was used
    renderer_used: String,
    /// Number of items rendered
    items_rendered: usize,
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

fn default_max_list_items() -> usize {
    10
}

fn default_max_table_columns() -> usize {
    6
}

fn default_accent_color() -> String {
    "#0078D4".to_string() // Microsoft Blue
}

fn default_background_color() -> String {
    "#FFFFFF".to_string()
}

fn default_renderer() -> String {
    "auto".to_string()
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
            summary: Some(
                "Renderer that converts chat2data results into Adaptive Cards".to_string(),
            ),
            capabilities: vec!["host:telemetry".to_string()],
            ops: vec![
                node::Op {
                    name: "render".to_string(),
                    summary: Some("Render query results to an Adaptive Card".to_string()),
                    input: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(
                            &json!({"type":"object"}),
                        )),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    output: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(
                            &json!({"type":"object"}),
                        )),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    examples: Vec::new(),
                },
                node::Op {
                    name: "auto_select".to_string(),
                    summary: Some("Choose the best renderer for a result shape".to_string()),
                    input: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(
                            &json!({"type":"object"}),
                        )),
                        content_type: "application/cbor".to_string(),
                        schema_version: None,
                    },
                    output: node::IoSchema {
                        schema: node::SchemaSource::InlineCbor(canonical_cbor_bytes(
                            &json!({"type":"object"}),
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
                        &json!({"ok": false, "error": format!("invalid input cbor: {err}")}),
                    ),
                    output_metadata_cbor: None,
                });
            }
        };

        let output = match op.as_str() {
            "render" => handle_render(&input),
            "auto_select" => handle_auto_select(&input),
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
                let cfg = crate::ComponentConfig {
                    max_list_items: answers
                        .get("max_list_items")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize)
                        .unwrap_or_else(crate::default_max_list_items),
                    max_table_columns: answers
                        .get("max_table_columns")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize)
                        .unwrap_or_else(crate::default_max_table_columns),
                    theme: crate::ThemeConfig::default(),
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
// Core Rendering Logic
// ============================================================================

/// Handle render operation
fn handle_render(input: &Value) -> Value {
    let cfg = load_config(input).unwrap_or_default();

    // Extract render input
    let render_input: RenderInput = match input.get("data").map(|data| RenderInput {
        data: serde_json::from_value(data.clone()).unwrap_or(QueryData {
            rows: vec![],
            columns: vec![],
            total_count: None,
            truncated: false,
            truncation_reason: None,
        }),
        renderer: input
            .get("renderer")
            .and_then(Value::as_str)
            .unwrap_or("auto")
            .to_string(),
        renderer_options: input
            .get("renderer_options")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        title: input.get("title").and_then(Value::as_str).map(String::from),
    }) {
        Some(ri) => ri,
        None => {
            return json!({
                "ok": false,
                "error": "missing data"
            });
        }
    };

    // Auto-select renderer if needed
    let renderer = if render_input.renderer == "auto" {
        auto_select_renderer(&render_input.data)
    } else {
        render_input.renderer.clone()
    };

    // Render based on selected renderer
    let output = match renderer.as_str() {
        "list" => render_list(&render_input, &cfg),
        "table" => render_table(&render_input, &cfg),
        "card" => render_card(&render_input, &cfg),
        "graph" => render_graph(&render_input, &cfg),
        _ => render_table(&render_input, &cfg), // fallback to table
    };

    log_event(&format!("render_{}", renderer));

    json!({
        "ok": true,
        "output": output,
    })
}

/// Handle auto-select operation
fn handle_auto_select(input: &Value) -> Value {
    let data: QueryData = match input
        .get("data")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(d) => d,
        None => {
            return json!({
                "ok": false,
                "error": "missing or invalid data"
            });
        }
    };

    let renderer = auto_select_renderer(&data);
    let reasoning = auto_select_reasoning(&data, &renderer);

    json!({
        "ok": true,
        "renderer": renderer,
        "reasoning": reasoning,
    })
}

/// Auto-select the best renderer based on data shape
fn auto_select_renderer(data: &QueryData) -> String {
    let row_count = data.rows.len();
    let col_count = data.columns.len();

    // Single item -> card
    if row_count == 1 {
        return "card".to_string();
    }

    // Empty results -> table (will show "no results")
    if row_count == 0 {
        return "table".to_string();
    }

    // Few columns with label-value pattern -> might be graph
    if col_count == 2 {
        // Check if it looks like aggregate data
        if let Some(first_row) = data.rows.first() {
            if first_row.is_object() {
                let has_numeric = first_row
                    .as_object()
                    .map(|obj| obj.values().any(|v| v.is_number()))
                    .unwrap_or(false);
                if has_numeric && row_count <= 10 {
                    return "graph".to_string();
                }
            }
        }
    }

    // Many columns -> table
    if col_count > 4 {
        return "table".to_string();
    }

    // Default to list for moderate data
    if row_count <= 20 {
        return "list".to_string();
    }

    "table".to_string()
}

/// Get reasoning for auto-selection
fn auto_select_reasoning(data: &QueryData, renderer: &str) -> String {
    let row_count = data.rows.len();
    let col_count = data.columns.len();

    match renderer {
        "card" => format!("Single item detected ({} row)", row_count),
        "graph" => format!(
            "Aggregate data pattern ({} cols, {} rows)",
            col_count, row_count
        ),
        "list" => format!(
            "Collection of {} items with {} fields",
            row_count, col_count
        ),
        "table" => format!(
            "Tabular data with {} columns and {} rows",
            col_count, row_count
        ),
        _ => format!("Default selection for {} rows", row_count),
    }
}

/// Render as list
fn render_list(input: &RenderInput, cfg: &ComponentConfig) -> RenderOutput {
    let data = &input.data;
    let opts = &input.renderer_options;

    let mut body: Vec<Value> = Vec::new();

    // Add title if provided
    if let Some(title) = &input.title {
        body.push(json!({
            "type": "TextBlock",
            "text": title,
            "weight": "Bolder",
            "size": "Large",
            "wrap": true
        }));
    }

    // Determine title and subtitle columns
    let title_col = opts
        .title_column
        .clone()
        .or_else(|| data.columns.first().cloned())
        .unwrap_or_else(|| "title".to_string());
    let subtitle_col = opts
        .subtitle_column
        .clone()
        .or_else(|| data.columns.get(1).cloned());

    // Add items
    let max_items = cfg.max_list_items.min(data.rows.len());
    for (idx, row) in data.rows.iter().take(max_items).enumerate() {
        let title_val = get_string_value(row, &title_col);
        let subtitle_val = subtitle_col
            .as_ref()
            .map(|col| get_string_value(row, col))
            .unwrap_or_default();

        let mut item = json!({
            "type": "Container",
            "items": [
                {
                    "type": "TextBlock",
                    "text": title_val,
                    "weight": "Bolder",
                    "wrap": true
                }
            ],
            "spacing": if idx == 0 { "None" } else { "Medium" }
        });

        if !subtitle_val.is_empty() {
            if let Some(items) = item.get_mut("items").and_then(Value::as_array_mut) {
                items.push(json!({
                    "type": "TextBlock",
                    "text": subtitle_val,
                    "isSubtle": true,
                    "wrap": true,
                    "size": "Small"
                }));
            }
        }

        // Add separator between items
        if idx < max_items - 1 {
            if let Some(items) = item.get_mut("items").and_then(Value::as_array_mut) {
                items.push(json!({
                    "type": "Container",
                    "style": "default",
                    "bleed": true,
                    "spacing": "Small"
                }));
            }
        }

        body.push(item);
    }

    // Add truncation notice
    if data.rows.len() > max_items || data.truncated {
        body.push(json!({
            "type": "TextBlock",
            "text": format!("Showing {} of {} results", max_items, data.total_count.unwrap_or(data.rows.len())),
            "isSubtle": true,
            "size": "Small",
            "spacing": "Medium"
        }));
    }

    // Handle empty results
    if data.rows.is_empty() {
        body.push(json!({
            "type": "TextBlock",
            "text": "No results found",
            "isSubtle": true,
            "wrap": true
        }));
    }

    let card = build_adaptive_card(body);
    let summary = format!("List with {} items", data.rows.len().min(max_items));

    RenderOutput {
        card,
        summary,
        renderer_used: "list".to_string(),
        items_rendered: data.rows.len().min(max_items),
    }
}

/// Render as table
fn render_table(input: &RenderInput, cfg: &ComponentConfig) -> RenderOutput {
    let data = &input.data;
    let opts = &input.renderer_options;

    let mut body: Vec<Value> = Vec::new();

    // Add title if provided
    if let Some(title) = &input.title {
        body.push(json!({
            "type": "TextBlock",
            "text": title,
            "weight": "Bolder",
            "size": "Large",
            "wrap": true
        }));
    }

    // Determine columns to display
    let columns: Vec<String> = if opts.display_columns.is_empty() {
        data.columns
            .iter()
            .take(cfg.max_table_columns)
            .cloned()
            .collect()
    } else {
        opts.display_columns
            .iter()
            .filter(|c| data.columns.contains(c))
            .take(cfg.max_table_columns)
            .cloned()
            .collect()
    };

    if data.rows.is_empty() {
        body.push(json!({
            "type": "TextBlock",
            "text": "No results found",
            "isSubtle": true,
            "wrap": true
        }));
    } else {
        // Build table using ColumnSet
        // Header row
        let header_columns: Vec<Value> = columns
            .iter()
            .map(|col| {
                json!({
                    "type": "Column",
                    "width": "stretch",
                    "items": [{
                        "type": "TextBlock",
                        "text": col,
                        "weight": "Bolder",
                        "wrap": true
                    }]
                })
            })
            .collect();

        body.push(json!({
            "type": "ColumnSet",
            "columns": header_columns,
            "style": "emphasis"
        }));

        // Data rows
        let max_rows = cfg.max_list_items.min(data.rows.len());
        for row in data.rows.iter().take(max_rows) {
            let row_columns: Vec<Value> = columns
                .iter()
                .map(|col| {
                    let value = get_string_value(row, col);
                    json!({
                        "type": "Column",
                        "width": "stretch",
                        "items": [{
                            "type": "TextBlock",
                            "text": value,
                            "wrap": true,
                            "size": if opts.compact { "Small" } else { "Default" }
                        }]
                    })
                })
                .collect();

            body.push(json!({
                "type": "ColumnSet",
                "columns": row_columns,
                "separator": true
            }));
        }

        // Add truncation notice
        if data.rows.len() > max_rows || data.truncated {
            body.push(json!({
                "type": "TextBlock",
                "text": format!("Showing {} of {} rows", max_rows, data.total_count.unwrap_or(data.rows.len())),
                "isSubtle": true,
                "size": "Small",
                "spacing": "Medium"
            }));
        }
    }

    let card = build_adaptive_card(body);
    let summary = format!(
        "Table with {} columns and {} rows",
        columns.len(),
        data.rows.len().min(cfg.max_list_items)
    );

    RenderOutput {
        card,
        summary,
        renderer_used: "table".to_string(),
        items_rendered: data.rows.len().min(cfg.max_list_items),
    }
}

/// Render as single card
fn render_card(input: &RenderInput, _cfg: &ComponentConfig) -> RenderOutput {
    let data = &input.data;

    let mut body: Vec<Value> = Vec::new();

    // Add title if provided
    if let Some(title) = &input.title {
        body.push(json!({
            "type": "TextBlock",
            "text": title,
            "weight": "Bolder",
            "size": "Large",
            "wrap": true
        }));
    }

    if let Some(row) = data.rows.first() {
        if let Some(obj) = row.as_object() {
            // Create fact set for key-value pairs
            let facts: Vec<Value> = obj
                .iter()
                .map(|(key, value)| {
                    json!({
                        "title": key,
                        "value": format_value(value)
                    })
                })
                .collect();

            body.push(json!({
                "type": "FactSet",
                "facts": facts
            }));
        } else {
            // Fallback for non-object row
            body.push(json!({
                "type": "TextBlock",
                "text": row.to_string(),
                "wrap": true
            }));
        }
    } else {
        body.push(json!({
            "type": "TextBlock",
            "text": "No data available",
            "isSubtle": true,
            "wrap": true
        }));
    }

    let card = build_adaptive_card(body);
    let summary = "Single item card".to_string();

    RenderOutput {
        card,
        summary,
        renderer_used: "card".to_string(),
        items_rendered: 1,
    }
}

/// Render as graph (text-based representation)
fn render_graph(input: &RenderInput, _cfg: &ComponentConfig) -> RenderOutput {
    let data = &input.data;

    let mut body: Vec<Value> = Vec::new();

    // Add title if provided
    if let Some(title) = &input.title {
        body.push(json!({
            "type": "TextBlock",
            "text": title,
            "weight": "Bolder",
            "size": "Large",
            "wrap": true
        }));
    }

    // For simple bar chart representation using text
    if !data.rows.is_empty() && data.columns.len() >= 2 {
        let label_col = &data.columns[0];
        let value_col = &data.columns[1];

        // Find max value for scaling
        let max_value = data
            .rows
            .iter()
            .filter_map(|row| row.get(value_col).and_then(Value::as_f64))
            .fold(0.0f64, |a, b| a.max(b));

        for row in &data.rows {
            let label = get_string_value(row, label_col);
            let value = row.get(value_col).and_then(Value::as_f64).unwrap_or(0.0);

            // Create text-based bar
            let bar_width = if max_value > 0.0 {
                ((value / max_value) * 20.0) as usize
            } else {
                0
            };
            let bar = "\u{2588}".repeat(bar_width);
            let formatted_value = format_number(value);

            body.push(json!({
                "type": "ColumnSet",
                "columns": [
                    {
                        "type": "Column",
                        "width": "auto",
                        "items": [{
                            "type": "TextBlock",
                            "text": label,
                            "wrap": true
                        }]
                    },
                    {
                        "type": "Column",
                        "width": "stretch",
                        "items": [{
                            "type": "TextBlock",
                            "text": bar,
                            "color": "Accent"
                        }]
                    },
                    {
                        "type": "Column",
                        "width": "auto",
                        "items": [{
                            "type": "TextBlock",
                            "text": formatted_value,
                            "horizontalAlignment": "Right"
                        }]
                    }
                ],
                "spacing": "Small"
            }));
        }
    } else {
        body.push(json!({
            "type": "TextBlock",
            "text": "Insufficient data for graph",
            "isSubtle": true,
            "wrap": true
        }));
    }

    let card = build_adaptive_card(body);
    let summary = format!("Graph with {} data points", data.rows.len());

    RenderOutput {
        card,
        summary,
        renderer_used: "graph".to_string(),
        items_rendered: data.rows.len(),
    }
}

/// Build an AdaptiveCard wrapper
fn build_adaptive_card(body: Vec<Value>) -> Value {
    json!({
        "type": "AdaptiveCard",
        "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
        "version": ADAPTIVE_CARD_VERSION,
        "body": body
    })
}

/// Get a string value from a row object
fn get_string_value(row: &Value, column: &str) -> String {
    row.get(column).map(format_value).unwrap_or_default()
}

/// Format a JSON value for display
fn format_value(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::Bool(b) => if *b { "Yes" } else { "No" }.to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                format_number(i as f64)
            } else if let Some(f) = n.as_f64() {
                format_number(f)
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().take(3).map(format_value).collect();
            if arr.len() > 3 {
                format!("{}, ...", items.join(", "))
            } else {
                items.join(", ")
            }
        }
        Value::Object(_) => "[Object]".to_string(),
    }
}

/// Format a number for display
fn format_number(n: f64) -> String {
    if n.abs() >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n.abs() >= 1_000.0 {
        format!("{:.1}K", n / 1_000.0)
    } else if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{:.2}", n)
    }
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
                "render",
                "chat2data-renderer.op.render.title",
                "chat2data-renderer.op.render.description",
            ),
            op(
                "auto_select",
                "chat2data-renderer.op.auto_select.title",
                "chat2data-renderer.op.auto_select.description",
            ),
        ],
        input_schema: SchemaIr::Object {
            title: i18n("chat2data-renderer.schema.input.title"),
            description: i18n("chat2data-renderer.schema.input.description"),
            fields: BTreeMap::new(),
            additional_properties: true,
        },
        output_schema: SchemaIr::Object {
            title: i18n("chat2data-renderer.schema.output.title"),
            description: i18n("chat2data-renderer.schema.output.description"),
            fields: BTreeMap::new(),
            additional_properties: true,
        },
        config_schema: SchemaIr::Object {
            title: i18n("chat2data-renderer.schema.config.title"),
            description: i18n("chat2data-renderer.schema.config.description"),
            fields: BTreeMap::new(),
            additional_properties: true,
        },
        redactions: vec![],
        schema_hash: "chat2data-renderer-schema-v1".to_string(),
    }
}

fn build_qa_spec(mode: &str) -> QaSpec {
    match mode {
        "default" | "update" | "remove" | "setup" => QaSpec {
            mode: match mode {
                "default" => "default",
                "update" => "update",
                "remove" => "remove",
                "setup" => "setup",
                _ => "default",
            }
            .to_string(),
            title: i18n(match mode {
                "default" => "chat2data-renderer.qa.default.title",
                "setup" => "chat2data-renderer.qa.setup.title",
                "update" => "chat2data-renderer.qa.update.title",
                "remove" => "chat2data-renderer.qa.remove.title",
                _ => "chat2data-renderer.qa.default.title",
            }),
            description: Some(i18n(match mode {
                "default" => "chat2data-renderer.qa.default.description",
                "setup" => "chat2data-renderer.qa.setup.description",
                "update" => "chat2data-renderer.qa.update.description",
                "remove" => "chat2data-renderer.qa.remove.description",
                _ => "chat2data-renderer.qa.default.description",
            })),
            questions: Vec::new(),
            defaults: json!({}),
        },
        _ => QaSpec {
            mode: "default".to_string(),
            title: i18n("chat2data-renderer.qa.default.title"),
            description: Some(i18n("chat2data-renderer.qa.default.description")),
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
            flow_id: "chat2data-renderer".into(),
            node_id: None,
            provider: "chat2data-renderer".into(),
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

    #[test]
    fn test_auto_select_single_row() {
        let data = QueryData {
            rows: vec![json!({"id": 1, "name": "Alice"})],
            columns: vec!["id".to_string(), "name".to_string()],
            total_count: Some(1),
            truncated: false,
            truncation_reason: None,
        };

        assert_eq!(auto_select_renderer(&data), "card");
    }

    #[test]
    fn test_auto_select_many_columns() {
        let data = QueryData {
            rows: vec![
                json!({"a": 1, "b": 2, "c": 3, "d": 4, "e": 5}),
                json!({"a": 1, "b": 2, "c": 3, "d": 4, "e": 5}),
            ],
            columns: vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
                "e".to_string(),
            ],
            total_count: Some(2),
            truncated: false,
            truncation_reason: None,
        };

        assert_eq!(auto_select_renderer(&data), "table");
    }

    #[test]
    fn test_auto_select_few_items() {
        // Use 3 columns to avoid graph heuristic (graph only triggers with 2 columns + numeric)
        let data = QueryData {
            rows: vec![
                json!({"id": "1", "name": "Alice", "role": "admin"}),
                json!({"id": "2", "name": "Bob", "role": "user"}),
                json!({"id": "3", "name": "Carol", "role": "user"}),
            ],
            columns: vec!["id".to_string(), "name".to_string(), "role".to_string()],
            total_count: Some(3),
            truncated: false,
            truncation_reason: None,
        };

        assert_eq!(auto_select_renderer(&data), "list");
    }

    #[test]
    fn test_auto_select_aggregate() {
        let data = QueryData {
            rows: vec![
                json!({"status": "open", "count": 10}),
                json!({"status": "closed", "count": 25}),
            ],
            columns: vec!["status".to_string(), "count".to_string()],
            total_count: Some(2),
            truncated: false,
            truncation_reason: None,
        };

        assert_eq!(auto_select_renderer(&data), "graph");
    }

    #[test]
    fn test_format_value() {
        assert_eq!(format_value(&Value::Null), "-");
        assert_eq!(format_value(&json!(true)), "Yes");
        assert_eq!(format_value(&json!(false)), "No");
        assert_eq!(format_value(&json!("hello")), "hello");
        assert_eq!(format_value(&json!(42)), "42");
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(42.0), "42");
        assert_eq!(format_number(1234.0), "1.2K");
        assert_eq!(format_number(1234567.0), "1.2M");
        assert_eq!(format_number(3.56789), "3.57");
    }

    #[test]
    fn test_render_list() {
        let input = RenderInput {
            data: QueryData {
                rows: vec![
                    json!({"name": "Alice", "email": "alice@example.com"}),
                    json!({"name": "Bob", "email": "bob@example.com"}),
                ],
                columns: vec!["name".to_string(), "email".to_string()],
                total_count: Some(2),
                truncated: false,
                truncation_reason: None,
            },
            renderer: "list".to_string(),
            renderer_options: RendererOptions::default(),
            title: Some("Users".to_string()),
        };
        let cfg = ComponentConfig::default();

        let output = render_list(&input, &cfg);
        assert_eq!(output.renderer_used, "list");
        assert_eq!(output.items_rendered, 2);
        assert!(output.card.get("body").is_some());
    }

    #[test]
    fn test_render_table() {
        let input = RenderInput {
            data: QueryData {
                rows: vec![json!({"id": 1, "name": "Alice", "status": "active"})],
                columns: vec!["id".to_string(), "name".to_string(), "status".to_string()],
                total_count: Some(1),
                truncated: false,
                truncation_reason: None,
            },
            renderer: "table".to_string(),
            renderer_options: RendererOptions::default(),
            title: None,
        };
        let cfg = ComponentConfig::default();

        let output = render_table(&input, &cfg);
        assert_eq!(output.renderer_used, "table");
    }

    #[test]
    fn test_render_card() {
        let input = RenderInput {
            data: QueryData {
                rows: vec![json!({"id": 123, "name": "Alice", "role": "Admin"})],
                columns: vec!["id".to_string(), "name".to_string(), "role".to_string()],
                total_count: Some(1),
                truncated: false,
                truncation_reason: None,
            },
            renderer: "card".to_string(),
            renderer_options: RendererOptions::default(),
            title: Some("User Details".to_string()),
        };
        let cfg = ComponentConfig::default();

        let output = render_card(&input, &cfg);
        assert_eq!(output.renderer_used, "card");
        assert_eq!(output.items_rendered, 1);
    }
}
