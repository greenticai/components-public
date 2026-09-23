//! Tool dispatch layer for the webhook DesignExtension.
//!
//! Four tools are exposed to the designer LLM, all speaking the trigger
//! contract v1 node config shape (greentic-designer
//! `docs/trigger-contract-v1.md`):
//! - `validate_webhook_config` — diagnostics on a webhook trigger config block
//! - `suggest_path` — slugify an intent into a `trigger_id` and its route
//! - `infer_auth_from_curl` — derive the `verify` block from a sample curl
//! - `suggest_verification` — `verify` / `challenge` presets per sender
pub mod curl_import;
pub mod presets;
pub mod suggest_path;
pub mod validate;

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
            "validate_webhook_config",
            "Validate a webhook trigger config block — returns diagnostics",
            r#"{"type":"object","properties":{"node":{"type":"object"}},"required":["node"]}"#,
        ),
        (
            "suggest_path",
            "Slugify an intent into a trigger_id and the route it mounts at (e.g. 'Receive Stripe events' → trigger_id 'stripe_events', route /trigger/stripe_events)",
            r#"{"type":"object","properties":{"intent":{"type":"string"}},"required":["intent"]}"#,
        ),
        (
            "infer_auth_from_curl",
            "Infer the webhook trigger's verify block (hmac-sha256/sha1 or bearer) from a sample curl that an upstream system would send; secret refs are names, never the token in the curl",
            r#"{"type":"object","properties":{"curl_cmd":{"type":"string"}},"required":["curl_cmd"]}"#,
        ),
        (
            "suggest_verification",
            "Return the verify (and challenge) block for a known sender: threads/instagram/facebook/whatsapp/meta, github, or any other name for a bearer preset",
            r#"{"type":"object","properties":{"provider":{"type":"string"}},"required":["provider"]}"#,
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
        "validate_webhook_config" => {
            let node = args.get("node").cloned().unwrap_or(Value::Null);
            let (valid, diags) = validate::validate_webhook_config(&node);
            Ok(
                serde_json::to_string(&serde_json::json!({"valid": valid, "diagnostics": diags}))
                    .unwrap(),
            )
        }
        "suggest_path" => suggest_path::suggest_path(&args),
        "infer_auth_from_curl" => curl_import::infer_auth_from_curl(&args),
        "suggest_verification" => presets::suggest_verification(&args),
        other => Err(format!("unknown tool: {other}")),
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn list_tools_returns_four_definitions() {
        let tools = list_tools();
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), 4);
        for expected in [
            "validate_webhook_config",
            "suggest_path",
            "infer_auth_from_curl",
            "suggest_verification",
        ] {
            assert!(names.contains(&expected), "missing tool: {expected}");
        }
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
}
