//! Tool dispatch layer for the HTTP DesignExtension.
pub mod auth_suggest;
pub mod card_submit;
pub mod curl_import;
pub mod generate;
pub mod validate;

pub const RUNTIME_VERSION: &str = env!("GREENTIC_HTTP_RUNTIME_VERSION");

pub fn runtime_component_ref() -> String {
    format!("oci://ghcr.io/greenticai/component/component-http:{RUNTIME_VERSION}")
}
