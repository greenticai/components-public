//! Greentic `greentic.jev` design extension — WIT export layer.
//!
//! Exports one agentic-worker tool `decide` via `greentic:extension-design/tools`.
//! Request/response shaping lives in the pure [`input`]/[`output`]/[`tool_meta`]
//! modules. This file is the thin WASM glue: deserialize input → resolve the API
//! key via host `secrets::get` → POST to api.typesafe.ai via host `http::fetch` →
//! map the response.
#![allow(clippy::used_underscore_items)]

#[allow(warnings)]
pub mod bindings;

pub mod input;
pub mod output;
pub mod tool_meta;

pub use bindings::exports;

use bindings::exports::greentic::extension_base::{lifecycle, manifest};
use bindings::exports::greentic::extension_design::tools;
use bindings::greentic::extension_base::types;
use bindings::greentic::extension_host::{http, secrets};

use input::DecideInput;

pub struct Component;

/// Secret URI for the Jev API key. Resolved by the host; never leaves this
/// component. Granted by the `secret://jev/` prefix in describe.json.
const JEV_KEY_REF: &str = "secret://jev/api_key";
const JEV_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

// ===== base::manifest =====
impl manifest::Guest for Component {
    fn get_identity() -> types::ExtensionIdentity {
        types::ExtensionIdentity {
            id: "greentic.jev".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            kind: types::Kind::Design,
        }
    }
    fn get_offered() -> Vec<types::CapabilityRef> {
        vec![types::CapabilityRef {
            id: "greentic:jev/decide".into(),
            version: "1.0.0".into(),
        }]
    }
    fn get_required() -> Vec<types::CapabilityRef> {
        vec![]
    }
}

// ===== base::lifecycle =====
impl lifecycle::Guest for Component {
    fn init(_config_json: String) -> Result<(), types::ExtensionError> {
        Ok(())
    }
    fn shutdown() {}
}

// ===== design::tools =====
impl tools::Guest for Component {
    fn list_tools() -> Vec<tools::ToolDefinition> {
        tool_meta::all_tools()
            .into_iter()
            .map(|meta| tools::ToolDefinition {
                name: meta.name.into(),
                description: meta.description.into(),
                input_schema_json: meta.input_schema_json.into(),
                output_schema_json: Some(meta.output_schema_json.into()),
                capabilities: Some(meta.capabilities),
                agentic_worker_metadata: Some(meta.agentic_worker_metadata.into()),
            })
            .collect()
    }

    fn invoke_tool(name: String, args_json: String) -> Result<String, types::ExtensionError> {
        match name.as_str() {
            tool_meta::DECIDE_TOOL => {
                let parsed: DecideInput = serde_json::from_str(&args_json).map_err(|error| {
                    types::ExtensionError::InvalidInput(format!("decode decide input: {error}"))
                })?;
                let body = input::build_request_body(&parsed)
                    .map_err(types::ExtensionError::InvalidInput)?;
                let raw = jev_post(&body)?;
                let output = output::map_response(&raw).map_err(types::ExtensionError::Internal)?;
                serde_json::to_string(&output).map_err(|error| {
                    types::ExtensionError::Internal(format!("encode output: {error}"))
                })
            }
            other => Err(types::ExtensionError::InvalidInput(format!(
                "unknown tool: {other}"
            ))),
        }
    }
}

/// Resolve the API key, POST `body` as JSON to the System One endpoint, return
/// response bytes for a 2xx status. Non-2xx maps to a typed `ExtensionError`.
fn jev_post(body: &serde_json::Value) -> Result<Vec<u8>, types::ExtensionError> {
    let api_key = secrets::get(JEV_KEY_REF).map_err(|error| {
        types::ExtensionError::PermissionDenied(format!("resolve secret {JEV_KEY_REF}: {error}"))
    })?;
    let body_bytes = serde_json::to_vec(body).map_err(|error| {
        types::ExtensionError::Internal(format!("encode request body: {error}"))
    })?;
    let request = http::Request {
        method: "POST".into(),
        url: JEV_ENDPOINT.into(),
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("authorization".into(), format!("Bearer {api_key}")),
        ],
        body: Some(body_bytes),
    };
    let response = http::fetch(&request)
        .map_err(|error| types::ExtensionError::Internal(format!("http fetch: {error}")))?;
    if (200..300).contains(&response.status) {
        return Ok(response.body);
    }
    let detail = String::from_utf8_lossy(&response.body);
    Err(match response.status {
        401 | 403 => types::ExtensionError::PermissionDenied(format!(
            "Jev rejected the API key ({}): {detail}",
            response.status
        )),
        400 | 422 => types::ExtensionError::InvalidInput(format!(
            "Jev rejected the request ({}): {detail}",
            response.status
        )),
        429 => types::ExtensionError::Internal(format!("Jev rate limit exceeded (429): {detail}")),
        status => {
            types::ExtensionError::Internal(format!("Jev request failed ({status}): {detail}"))
        }
    })
}

#[cfg(target_family = "wasm")]
bindings::export!(Component with_types_in bindings);
