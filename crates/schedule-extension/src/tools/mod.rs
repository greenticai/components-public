//! Tool dispatch layer for the schedule DesignExtension.
//!
//! Three tools, all speaking the trigger contract v1 node config shape
//! (greentic-designer `docs/trigger-contract-v1.md` §6.5): the schedule fields
//! sit at the TOP level of the node config as `expr` and `timezone`.
//!
//! - `validate_cron` — diagnostics on a schedule trigger config block
//! - `describe_schedule` — the schedule in words plus its next five fire times
//! - `suggest_cron` — a cron expression from a phrase, or an honest abstention
pub mod cron_expr;
pub mod describe_schedule;
pub mod suggest_cron;
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
            "validate_cron",
            "Validate a schedule trigger config block (expr + timezone at the top level) — returns diagnostics",
            r#"{"type":"object","properties":{"node":{"type":"object"}},"required":["node"]}"#,
        ),
        (
            "describe_schedule",
            "Describe a cron expression in words and list its next five fire times in the declared zone",
            r#"{"type":"object","properties":{"expr":{"type":"string"},"timezone":{"type":"string"},"from":{"type":"string","description":"RFC 3339 instant to count from; defaults to now."}},"required":["expr"]}"#,
        ),
        (
            "suggest_cron",
            "Turn a phrase such as 'every weekday at 9am Jakarta time' into a cron expression and a zone; abstains rather than guessing when the phrasing is not recognised",
            r#"{"type":"object","properties":{"text":{"type":"string"},"timezone":{"type":"string"}},"required":["text"]}"#,
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
        "validate_cron" => {
            let node = args.get("node").cloned().unwrap_or(Value::Null);
            let (valid, diags) = validate::validate_cron(&node);
            Ok(
                serde_json::to_string(&serde_json::json!({"valid": valid, "diagnostics": diags}))
                    .unwrap_or_else(|_| r#"{"valid":false,"diagnostics":[]}"#.to_string()),
            )
        }
        "describe_schedule" => describe_schedule::describe_schedule(&args),
        "suggest_cron" => suggest_cron::suggest_cron(&args),
        other => Err(format!("unknown tool: {other}")),
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn list_tools_returns_three_definitions_with_readable_schemas() {
        let tools = list_tools();
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), 3);
        for expected in ["validate_cron", "describe_schedule", "suggest_cron"] {
            assert!(names.contains(&expected), "missing tool: {expected}");
        }
        for t in &tools {
            let _: Value = serde_json::from_str(&t.input_schema_json)
                .unwrap_or_else(|_| panic!("bad schema: {}", t.name));
        }
    }

    #[test]
    fn invoke_unknown_tool_returns_error() {
        let err = invoke_tool("nope", "{}").expect_err("must fail");
        assert!(err.contains("unknown tool"));
    }

    #[test]
    fn the_two_cron_readers_agree_about_what_is_valid() {
        // `suggest_cron` hands its answer to an operator, `validate_cron`
        // judges what the operator ends up with. The two disagreeing means a
        // suggestion the extension itself then flags as broken.
        let suggested: Value = serde_json::from_str(
            &invoke_tool("suggest_cron", r#"{"text":"every weekday at 9am"}"#).expect("ok"),
        )
        .expect("json");
        let expr = suggested["expr"].as_str().expect("expr");
        let checked: Value = serde_json::from_str(
            &invoke_tool(
                "validate_cron",
                &serde_json::json!({ "node": { "config": { "expr": expr, "timezone": "UTC" } } })
                    .to_string(),
            )
            .expect("ok"),
        )
        .expect("json");
        assert_eq!(checked["valid"], true, "{checked}");
    }
}
