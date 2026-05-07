//! Greentic generic-LLM design extension.
//!
//! Carries the canonical `llm` nodeType — a fallback for prompt-driven
//! AI steps that aren't pinned to a specific provider. Provider-specific
//! extensions (`greentic.llm-openai` and friends) ship richer nodeTypes
//! that implement the actual completion call; this generic node is a
//! placeholder the planner can target when the provider is unknown or
//! pluggable.
//!
//! Pure metadata extension: WASM exports are no-op stubs. Design-time
//! tools (validate_llm_config, suggest_model, render_prompt_preview)
//! are deferred to a follow-up release.

#[allow(warnings)]
mod bindings;

use bindings::exports::greentic::extension_base::{lifecycle, manifest};
use bindings::exports::greentic::extension_design::{
    knowledge, prompting, tools as wit_tools, validation,
};
use bindings::greentic::extension_base::types;

struct Component;

// ---- extension-base/manifest ----
impl manifest::Guest for Component {
    fn get_identity() -> types::ExtensionIdentity {
        types::ExtensionIdentity {
            id: "greentic.llm-generic".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        vec![types::CapabilityRef {
            id: "greentic:llm/generic-step".into(),
            version: "1.0.0".into(),
        }]
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

// ---- extension-design/tools — no tools shipped (yet) ----
impl wit_tools::Guest for Component {
    fn list_tools() -> Vec<wit_tools::ToolDefinition> {
        Vec::new()
    }

    fn invoke_tool(name: String, _args_json: String) -> Result<String, types::ExtensionError> {
        Err(types::ExtensionError::InvalidInput(format!(
            "greentic.llm-generic exposes no tools yet (got '{name}')"
        )))
    }
}

// ---- extension-design/validation — no content types claimed ----
impl validation::Guest for Component {
    fn validate_content(content_type: String, _content_json: String) -> validation::ValidateResult {
        validation::ValidateResult {
            valid: false,
            diagnostics: vec![types::Diagnostic {
                severity: types::Severity::Error,
                code: "unsupported-content-type".into(),
                message: format!(
                    "greentic.llm-generic does not validate content types (got '{content_type}')"
                ),
                path: None,
            }],
        }
    }
}

// ---- extension-design/prompting — no prompt fragments contributed ----
impl prompting::Guest for Component {
    fn system_prompt_fragments() -> Vec<prompting::PromptFragment> {
        Vec::new()
    }
}

// ---- extension-design/knowledge — empty knowledge base ----
impl knowledge::Guest for Component {
    fn list_entries(_category_filter: Option<String>) -> Vec<knowledge::EntrySummary> {
        Vec::new()
    }

    fn get_entry(id: String) -> Result<knowledge::Entry, types::ExtensionError> {
        Err(types::ExtensionError::InvalidInput(format!(
            "greentic.llm-generic exposes no knowledge entries (got '{id}')"
        )))
    }

    fn suggest_entries(_query: String, _limit: u32) -> Vec<knowledge::EntrySummary> {
        Vec::new()
    }
}

bindings::export!(Component with_types_in bindings);
