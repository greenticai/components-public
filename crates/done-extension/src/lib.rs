//! Greentic Done design extension (`greentic.done`).
//!
//! Done @ Greentic lets a flow or an agentic worker ask a person for something
//! and react when they answer. This extension ships:
//!
//! - node `done.create_request`, node `done.read_request` (flow steps backed by
//!   the `done-ops` component, `component-done`);
//! - trigger node `done.event`, a webhook trigger pre-filled to verify the
//!   Core's HMAC-signed events (`x-trigger-kind` lives inside `config_schema`,
//!   never as a new `contributions` key; see `describe_tests.rs`);
//! - the tool `done_api`, which carries the secret requirements for all of it.
//!
//! The wasm exports for prompting / validation / knowledge are no-op stubs.

#[allow(warnings)]
mod bindings;
// Off-wasm the host seams do not exist, so only the tests reach `tools`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
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

/// The host as this extension sees it: `extension-host` secrets and http, plus
/// the clock and entropy the request id needs.
#[cfg(target_arch = "wasm32")]
struct HostSeams;

#[cfg(target_arch = "wasm32")]
impl tools::Host for HostSeams {
    fn secret(&self, uri: &str) -> Result<String, String> {
        bindings::greentic::extension_host::secrets::get(uri)
    }

    fn fetch(
        &self,
        req: component_done::env::HttpReq,
    ) -> Result<component_done::env::Response, String> {
        use bindings::greentic::extension_host::http;
        let resp = http::fetch(&http::Request {
            method: req.method,
            url: req.url,
            headers: req.headers,
            body: req.body,
        })?;
        Ok(component_done::env::Response {
            status: resp.status,
            body: resp.body,
        })
    }

    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
            .unwrap_or(0)
    }

    fn random(&self) -> [u8; 10] {
        let mut buf = [0u8; 10];
        if getrandom::fill(&mut buf).is_err() {
            // Never zeros: two ids in one millisecond would share a suffix.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            buf.copy_from_slice(&nanos.to_be_bytes()[6..16]);
        }
        buf
    }
}

impl manifest::Guest for Component {
    fn get_identity() -> types::ExtensionIdentity {
        types::ExtensionIdentity {
            id: "greentic.done".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        vec![types::CapabilityRef {
            id: "greentic:done/request".into(),
            version: "1.0.0".into(),
        }]
    }

    fn get_required() -> Vec<types::CapabilityRef> {
        Vec::new()
    }
}

impl lifecycle::Guest for Component {
    fn init(_config_json: String) -> Result<(), types::ExtensionError> {
        Ok(())
    }

    fn shutdown() {}
}

impl wit_tools::Guest for Component {
    fn list_tools() -> Vec<wit_tools::ToolDefinition> {
        vec![wit_tools::ToolDefinition {
            name: tools::TOOL.into(),
            description: tools::DESCRIPTION.into(),
            input_schema_json: tools::INPUT_SCHEMA.into(),
            output_schema_json: None,
        }]
    }

    #[cfg(target_arch = "wasm32")]
    fn invoke_tool(name: String, args_json: String) -> Result<String, types::ExtensionError> {
        tools::invoke(&HostSeams, &name, &args_json).map_err(types::ExtensionError::InvalidInput)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn invoke_tool(_name: String, _args_json: String) -> Result<String, types::ExtensionError> {
        // Host builds have no extension host to call; `tools` is tested directly.
        Err(types::ExtensionError::Internal(
            "invoke_tool runs only inside the extension host".into(),
        ))
    }
}

impl validation::Guest for Component {
    fn validate_content(content_type: String, _content_json: String) -> validation::ValidateResult {
        validation::ValidateResult {
            valid: false,
            diagnostics: vec![types::Diagnostic {
                severity: types::Severity::Error,
                code: "unsupported-content-type".into(),
                message: format!(
                    "greentic.done does not validate content types (got '{content_type}')"
                ),
                path: None,
            }],
        }
    }
}

impl prompting::Guest for Component {
    fn system_prompt_fragments() -> Vec<prompting::PromptFragment> {
        Vec::new()
    }
}

impl knowledge::Guest for Component {
    fn list_entries(_category_filter: Option<String>) -> Vec<knowledge::EntrySummary> {
        Vec::new()
    }

    fn get_entry(id: String) -> Result<knowledge::Entry, types::ExtensionError> {
        Err(types::ExtensionError::InvalidInput(format!(
            "greentic.done exposes no knowledge entries (got '{id}')"
        )))
    }

    fn suggest_entries(_query: String, _limit: u32) -> Vec<knowledge::EntrySummary> {
        Vec::new()
    }
}

// Exporting on a host build makes the cdylib link fail on the component-model
// symbol names; only the wasm artefact exports.
#[cfg(target_family = "wasm")]
bindings::export!(Component with_types_in bindings);
