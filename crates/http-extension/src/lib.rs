// Design extension guest for greentic.http.

#[allow(warnings)]
mod bindings;

pub mod tools;

use bindings::exports::greentic::extension_base::{lifecycle, manifest};
use bindings::exports::greentic::extension_design::{
    knowledge, prompting, tools as wit_tools, validation,
};
use bindings::greentic::extension_base::types;
use serde_json::Value;

struct Component;

// ---- extension-base/manifest ----
impl manifest::Guest for Component {
    fn get_identity() -> types::ExtensionIdentity {
        types::ExtensionIdentity {
            id: "greentic.http".to_string(),
            version: "0.1.0".to_string(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        Vec::new()
    }

    fn get_required() -> Vec<types::CapabilityRef> {
        Vec::new()
    }
}

// ---- extension-base/lifecycle ----
impl lifecycle::Guest for Component {
    fn init(_config_json: String) -> Result<(), types::ExtensionError> {
        Ok(())
    }

    fn shutdown() {}
}

// ---- extension-design/tools ----
impl wit_tools::Guest for Component {
    fn list_tools() -> Vec<wit_tools::ToolDefinition> {
        crate::tools::list_tools()
            .into_iter()
            .map(|t| wit_tools::ToolDefinition {
                name: t.name,
                description: t.description,
                input_schema_json: t.input_schema_json,
                output_schema_json: t.output_schema_json,
            })
            .collect()
    }

    fn invoke_tool(name: String, args_json: String) -> Result<String, types::ExtensionError> {
        crate::tools::invoke_tool(&name, &args_json).map_err(types::ExtensionError::InvalidInput)
    }
}

// ---- extension-design/validation ----
impl validation::Guest for Component {
    fn validate_content(content_type: String, content_json: String) -> validation::ValidateResult {
        if content_type != "http-node" {
            return validation::ValidateResult {
                valid: false,
                diagnostics: vec![types::Diagnostic {
                    severity: types::Severity::Error,
                    code: "unsupported-content-type".into(),
                    message: format!("this extension handles 'http-node', got '{content_type}'"),
                    path: None,
                }],
            };
        }
        let node: Value = match serde_json::from_str(&content_json) {
            Ok(v) => v,
            Err(e) => {
                return validation::ValidateResult {
                    valid: false,
                    diagnostics: vec![types::Diagnostic {
                        severity: types::Severity::Error,
                        code: "json-parse".into(),
                        message: e.to_string(),
                        path: None,
                    }],
                };
            }
        };
        let (valid, diags) = crate::tools::validate::validate_http_config(&node);
        let wit_diags = diags
            .into_iter()
            .map(|d| types::Diagnostic {
                severity: match d.severity {
                    crate::tools::validate::Severity::Error => types::Severity::Error,
                    crate::tools::validate::Severity::Warning => types::Severity::Warning,
                    crate::tools::validate::Severity::Info => types::Severity::Info,
                },
                code: d.code,
                message: d.message,
                path: d.path,
            })
            .collect();
        validation::ValidateResult {
            valid,
            diagnostics: wit_diags,
        }
    }
}

// ---- extension-design/prompting ----
impl prompting::Guest for Component {
    fn system_prompt_fragments() -> Vec<prompting::PromptFragment> {
        Vec::new()
    }
}

// ---- extension-design/knowledge ----
impl knowledge::Guest for Component {
    fn list_entries(_category_filter: Option<String>) -> Vec<knowledge::EntrySummary> {
        Vec::new()
    }

    fn get_entry(id: String) -> Result<knowledge::Entry, types::ExtensionError> {
        Err(types::ExtensionError::InvalidInput(format!(
            "unknown entry: {id}"
        )))
    }

    fn suggest_entries(_query: String, _limit: u32) -> Vec<knowledge::EntrySummary> {
        Vec::new()
    }
}

bindings::export!(Component with_types_in bindings);
