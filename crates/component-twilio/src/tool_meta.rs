//! Static metadata for the `send_sms` tool. Pure (no WIT imports).

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several input structs exist for the TOOL surface
// and are unused by the node surface, and `HttpReq`'s fields are read only on
// the wasm target. Silencing it here keeps the rest of the file diffable
// against its source.
#![allow(dead_code)]
pub const SEND_SMS_TOOL: &str = "send_sms";

const SEND_SMS_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["to", "body"],
  "properties": {
    "to": { "type": "string", "description": "Destination phone number in E.164 format, e.g. +15551234567" },
    "body": { "type": "string", "description": "The SMS message text" }
  }
}"#;

const SEND_SMS_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["message_sid"],
  "properties": {
    "message_sid": { "type": "string" },
    "status": { "type": "string" }
  }
}"#;

const SEND_SMS_AW_META: &str = r#"{"usage_hint":"Send an SMS via Twilio. Provide to (E.164 phone number) and body (the message text). The sender number and credentials are operator-configured secrets. Returns the Twilio message sid and status.","examples":[{"when":"the worker needs to text a user a code or notification","input":{"to":"+15551234567","body":"Your code is 123456"}}],"side_effects":"write","cost":"low","confirmation_required":false}"#;

pub struct ToolMeta {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema_json: &'static str,
    pub output_schema_json: &'static str,
    pub capabilities: Vec<String>,
    pub agentic_worker_metadata: &'static str,
}

#[must_use]
pub fn send_sms_tool() -> ToolMeta {
    ToolMeta {
        name: SEND_SMS_TOOL,
        description: "Send an SMS through Twilio. The account credentials and sender number are injected by the host from secrets and never returned.",
        input_schema_json: SEND_SMS_INPUT_SCHEMA,
        output_schema_json: SEND_SMS_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: SEND_SMS_AW_META,
    }
}

#[must_use]
pub fn all_tools() -> [ToolMeta; 1] {
    [send_sms_tool()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_declares_agentic_worker_capability() {
        let t = send_sms_tool();
        assert_eq!(t.name, "send_sms");
        assert!(t.capabilities.iter().any(|c| c == "agentic_worker"));
    }

    #[test]
    fn schemas_and_metadata_are_valid_json() {
        let t = send_sms_tool();
        serde_json::from_str::<serde_json::Value>(t.input_schema_json).expect("input schema JSON");
        serde_json::from_str::<serde_json::Value>(t.output_schema_json)
            .expect("output schema JSON");
        let meta: serde_json::Value =
            serde_json::from_str(t.agentic_worker_metadata).expect("aw metadata JSON");
        assert_eq!(meta["side_effects"], "write");
        assert_eq!(meta["confirmation_required"], false);
    }
}
