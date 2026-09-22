//! Greentic schedule design extension.
//!
//! Carries the `schedule trigger` nodeType under trigger contract v1
//! (`docs/trigger-contract-v1.md` in greentic-designer): the flow is started
//! by greentic-start's scheduler when a cron expression comes due, and enters
//! the flow at the node AFTER the trigger.
//!
//! The one property worth stating up front is that **the schedule is checked
//! with the same crate the runtime schedules with** (`cron`, five fields
//! normalised by prepending seconds). A design-time validator that is only
//! nearly the runtime's parser moves a class of typos from the canvas, where
//! an operator can see the node, to a deploy minutes later — so
//! `tools::cron_expr` is the single reader and everything else goes through
//! it.
//!
//! This extension ships:
//!
//! - the `trigger.schedule` nodeType + its JSON Schema, whose root carries the
//!   `x-trigger-kind: "cron"` keyword the designer reads to decide the node
//!   emits a declaration into `assets/triggers.json` (see `describe_tests.rs`
//!   for why the marker lives there and not in `contributions`); and
//! - three design-time tools:
//!     * `validate_cron`
//!     * `describe_schedule`
//!     * `suggest_cron`
//!
//! The WASM exports for prompting / validation / knowledge are no-op stubs —
//! the extension contributes no prompt fragments, no knowledge base and no
//! content-type validators.

#[allow(warnings)]
mod bindings;
mod tools;

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "describe_tests.rs"]
mod describe_tests;

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
            id: "greentic.schedule".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        vec![types::CapabilityRef {
            id: "greentic:schedule/trigger-spec".into(),
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

// ---- extension-design/validation — no content types claimed ----
impl validation::Guest for Component {
    fn validate_content(content_type: String, _content_json: String) -> validation::ValidateResult {
        validation::ValidateResult {
            valid: false,
            diagnostics: vec![types::Diagnostic {
                severity: types::Severity::Error,
                code: "unsupported-content-type".into(),
                message: format!(
                    "greentic.schedule does not validate content types (got '{content_type}')"
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
            "greentic.schedule exposes no knowledge entries (got '{id}')"
        )))
    }

    fn suggest_entries(_query: String, _limit: u32) -> Vec<knowledge::EntrySummary> {
        Vec::new()
    }
}

// The generated `export_name`s carry WIT-qualified names containing `:` and
// `@`, which the native linker version script cannot parse. They are only
// meaningful for the wasm component anyway, so keep them off the native build
// (http-extension already does this).
#[cfg(target_arch = "wasm32")]
bindings::export!(Component with_types_in bindings);
