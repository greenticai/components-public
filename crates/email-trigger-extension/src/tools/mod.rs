//! Tool dispatch layer for the email trigger DesignExtension.
//!
//! One tool, speaking the trigger contract v1 node config shape: the mailbox
//! fields sit at the TOP level of the node config, beside the common
//! `trigger_id` / `enabled`.
//!
//! - `validate_email_trigger` — diagnostics on an email trigger config block
pub mod validate;

use serde_json::Value;

pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema_json: String,
    pub output_schema_json: Option<String>,
}

pub fn list_tools() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "validate_email_trigger".into(),
        description: "Validate an email trigger config block (host, username, password_ref and the rest at the top level) — returns diagnostics".into(),
        input_schema_json: r#"{"type":"object","properties":{"node":{"type":"object"}},"required":["node"]}"#.into(),
        output_schema_json: None,
    }]
}

pub fn invoke_tool(name: &str, args_json: &str) -> Result<String, String> {
    let args: Value = serde_json::from_str(args_json).map_err(|e| format!("args json: {e}"))?;
    match name {
        "validate_email_trigger" => {
            let node = args.get("node").cloned().unwrap_or(Value::Null);
            let (valid, diags) = validate::validate_email_trigger(&node);
            Ok(
                serde_json::to_string(&serde_json::json!({"valid": valid, "diagnostics": diags}))
                    .unwrap_or_else(|_| r#"{"valid":false,"diagnostics":[]}"#.to_string()),
            )
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn list_tools_returns_one_definition_with_a_readable_schema() {
        let tools = list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "validate_email_trigger");
        let _: Value = serde_json::from_str(&tools[0].input_schema_json).expect("schema");
    }

    #[test]
    fn invoke_unknown_tool_returns_error() {
        let err = invoke_tool("nope", "{}").expect_err("must fail");
        assert!(err.contains("unknown tool"));
    }
}
