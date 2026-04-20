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

use serde_json::Value;

pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema_json: String,
    pub output_schema_json: Option<String>,
}

pub fn list_tools() -> Vec<ToolDef> {
    let defs = [
        (
            "generate_http_node",
            "Generate a YGTc HTTP node from natural language intent",
            r#"{"type":"object","properties":{"intent":{"type":"string"},"context":{"type":"object"}},"required":["intent"]}"#,
        ),
        (
            "validate_http_config",
            "Validate a generated YGTc HTTP node — returns diagnostics",
            r#"{"type":"object","properties":{"node":{"type":"object"}},"required":["node"]}"#,
        ),
        (
            "curl_to_node",
            "Convert a curl command into a YGTc HTTP node",
            r#"{"type":"object","properties":{"curl_cmd":{"type":"string"},"node_id":{"type":"string"}},"required":["curl_cmd"]}"#,
        ),
        (
            "suggest_auth",
            "Recommend auth configuration from a natural-language API description",
            r#"{"type":"object","properties":{"api_description":{"type":"string"}},"required":["api_description"]}"#,
        ),
        (
            "generate_from_card_submit",
            "Generate an HTTP node that maps Adaptive Card submit fields to an API body",
            r#"{"type":"object","properties":{"card_schema":{"type":"object"},"api_intent":{"type":"string"},"node_id":{"type":"string"}},"required":["card_schema","api_intent"]}"#,
        ),
    ];
    defs.iter()
        .map(|(n, d, s)| ToolDef {
            name: (*n).into(),
            description: (*d).into(),
            input_schema_json: (*s).into(),
            output_schema_json: None,
        })
        .collect()
}

pub fn invoke_tool(name: &str, args_json: &str) -> Result<String, String> {
    let args: Value = serde_json::from_str(args_json).map_err(|e| format!("args json: {e}"))?;
    match name {
        "generate_http_node" => generate::generate_http_node(&args),
        "validate_http_config" => {
            let node = args.get("node").cloned().unwrap_or(Value::Null);
            let (valid, diags) = validate::validate_http_config(&node);
            Ok(
                serde_json::to_string(&serde_json::json!({"valid": valid, "diagnostics": diags}))
                    .unwrap(),
            )
        }
        "curl_to_node" => curl_import::curl_to_node(&args),
        "suggest_auth" => auth_suggest::suggest_auth(&args),
        "generate_from_card_submit" => card_submit::generate_from_card_submit(&args),
        other => Err(format!("unknown tool: {other}")),
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn list_tools_returns_five_definitions() {
        let tools = list_tools();
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), 5);
        for expected in [
            "generate_http_node",
            "validate_http_config",
            "curl_to_node",
            "suggest_auth",
            "generate_from_card_submit",
        ] {
            assert!(names.contains(&expected), "missing tool: {expected}");
        }
        // Each tool has valid JSON Schema
        for t in &tools {
            let _: serde_json::Value = serde_json::from_str(&t.input_schema_json)
                .unwrap_or_else(|_| panic!("bad schema: {}", t.name));
        }
    }

    #[test]
    fn invoke_unknown_tool_returns_error() {
        let err = invoke_tool("nope", "{}").expect_err("must fail");
        assert!(err.contains("unknown tool"));
    }

    #[test]
    fn invoke_generate_http_node_roundtrips() {
        let args = r#"{"intent":"GET from https://x/a","context":{}}"#;
        let out = invoke_tool("generate_http_node", args).expect("ok");
        assert!(out.contains("\"component\""));
    }
}
