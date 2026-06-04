//! Greentic platform-bootstrap design extension.
//!
//! This extension exists for one purpose: ship the canonical
//! `nodeTypes` descriptors for the editor primitives the platform
//! itself owns (start, trigger, llm) so the designer's
//! `NodeTypeRegistry::with_builtins()` bootstrap fallback can finally
//! reach `Self::new()` and the editor catalog becomes 100%
//! extension-driven.
//!
//! Everything *interesting* lives in `describe.json` —
//! `contributions.nodeTypes`. The WASM exports below are no-op stubs
//! that satisfy the design-extension WIT contract; this extension
//! ships no tools, no schemas, no prompts, no knowledge.

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
            id: "greentic.platform-bootstrap".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        vec![types::CapabilityRef {
            id: "greentic:platform/bootstrap-nodes".into(),
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

// ---- extension-design/tools — no tools shipped ----
impl wit_tools::Guest for Component {
    fn list_tools() -> Vec<wit_tools::ToolDefinition> {
        Vec::new()
    }

    fn invoke_tool(name: String, _args_json: String) -> Result<String, types::ExtensionError> {
        Err(types::ExtensionError::InvalidInput(format!(
            "platform-bootstrap exposes no tools (got '{name}')"
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
                    "platform-bootstrap does not validate content types (got '{content_type}')"
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
            "platform-bootstrap exposes no knowledge entries (got '{id}')"
        )))
    }

    fn suggest_entries(_query: String, _limit: u32) -> Vec<knowledge::EntrySummary> {
        Vec::new()
    }
}

bindings::export!(Component with_types_in bindings);
