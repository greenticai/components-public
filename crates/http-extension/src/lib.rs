// Design extension guest for greentic.http.
//
// This scaffold implements every export required by the extension-design
// contract as a TODO stub. Replace each body with real logic before shipping.

#[allow(warnings)]
mod bindings;

use bindings::exports::greentic::extension_base::{lifecycle, manifest};
use bindings::exports::greentic::extension_design::{knowledge, prompting, tools, validation};
use bindings::greentic::extension_base::types;

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
        // TODO: read configuration from `config_json` and initialize state.
        Ok(())
    }

    fn shutdown() {
        // TODO: release any resources the extension owns.
    }
}

// ---- extension-design/tools ----
impl tools::Guest for Component {
    fn list_tools() -> Vec<tools::ToolDefinition> {
        // TODO: return the list of tools the designer may invoke.
        Vec::new()
    }

    fn invoke_tool(
        name: String,
        _args_json: String,
    ) -> Result<String, types::ExtensionError> {
        // TODO: dispatch on `name` and return a JSON-encoded result.
        Err(types::ExtensionError::InvalidInput(format!(
            "unknown tool: {name}"
        )))
    }
}

// ---- extension-design/validation ----
impl validation::Guest for Component {
    fn validate_content(
        _content_type: String,
        _content_json: String,
    ) -> validation::ValidateResult {
        // TODO: run domain-specific validation and emit diagnostics.
        validation::ValidateResult {
            valid: true,
            diagnostics: Vec::new(),
        }
    }
}

// ---- extension-design/prompting ----
impl prompting::Guest for Component {
    fn system_prompt_fragments() -> Vec<prompting::PromptFragment> {
        // TODO: contribute prompt fragments for the designer LLM context.
        Vec::new()
    }
}

// ---- extension-design/knowledge ----
impl knowledge::Guest for Component {
    fn list_entries(
        _category_filter: Option<String>,
    ) -> Vec<knowledge::EntrySummary> {
        // TODO: return the knowledge entries this extension offers.
        Vec::new()
    }

    fn get_entry(id: String) -> Result<knowledge::Entry, types::ExtensionError> {
        // TODO: look up and return the knowledge entry.
        Err(types::ExtensionError::InvalidInput(format!(
            "unknown entry: {id}"
        )))
    }

    fn suggest_entries(
        _query: String,
        _limit: u32,
    ) -> Vec<knowledge::EntrySummary> {
        // TODO: rank knowledge entries for `query`.
        Vec::new()
    }
}

bindings::export!(Component with_types_in bindings);
