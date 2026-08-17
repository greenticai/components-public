//! Static metadata for the `send_email` tool. Pure (no WIT imports) so the
//! `agentic_worker` opt-in is asserted by a host test.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
pub const SEND_EMAIL_TOOL: &str = "send_email";

const SEND_EMAIL_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["from", "to", "subject"],
  "properties": {
    "from": { "type": "string", "description": "Verified Resend sender, e.g. \"Bot <bot@yourdomain.com>\"" },
    "to": { "description": "Recipient email or array of emails", "type": ["string", "array"], "items": { "type": "string" } },
    "subject": { "type": "string" },
    "text": { "type": "string", "description": "Plain-text body (provide text and/or html)" },
    "html": { "type": "string", "description": "HTML body (provide text and/or html)" },
    "cc": { "type": ["string", "array"], "items": { "type": "string" } },
    "bcc": { "type": ["string", "array"], "items": { "type": "string" } },
    "reply_to": { "type": ["string", "array"], "items": { "type": "string" } }
  }
}"#;

const SEND_EMAIL_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["message_id"],
  "properties": { "message_id": { "type": "string" } }
}"#;

const SEND_EMAIL_AW_META: &str = r#"{"usage_hint":"Send an email via Resend. Provide from (a verified sender), to, subject, and at least one of text or html. Returns the provider message id.","examples":[{"when":"the worker needs to email a result or notification","input":{"from":"Bot <bot@yourdomain.com>","to":"user@example.com","subject":"Your report","text":"Done."}}],"side_effects":"write","cost":"low","confirmation_required":false}"#;

pub struct ToolMeta {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema_json: &'static str,
    pub output_schema_json: &'static str,
    pub capabilities: Vec<String>,
    pub agentic_worker_metadata: &'static str,
}

#[must_use]
pub fn send_email_tool() -> ToolMeta {
    ToolMeta {
        name: SEND_EMAIL_TOOL,
        description: "Send an email through Resend. The API key is injected by the host and never returned.",
        input_schema_json: SEND_EMAIL_INPUT_SCHEMA,
        output_schema_json: SEND_EMAIL_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: SEND_EMAIL_AW_META,
    }
}

#[must_use]
pub fn all_tools() -> [ToolMeta; 1] {
    [send_email_tool()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_declares_agentic_worker_capability() {
        let t = send_email_tool();
        assert_eq!(t.name, "send_email");
        assert!(t.capabilities.iter().any(|c| c == "agentic_worker"));
    }

    #[test]
    fn schemas_and_metadata_are_valid_json() {
        let t = send_email_tool();
        serde_json::from_str::<serde_json::Value>(t.input_schema_json).expect("input schema JSON");
        serde_json::from_str::<serde_json::Value>(t.output_schema_json)
            .expect("output schema JSON");
        let meta: serde_json::Value =
            serde_json::from_str(t.agentic_worker_metadata).expect("aw metadata JSON");
        assert_eq!(meta["side_effects"], "write");
        assert_eq!(meta["confirmation_required"], false);
    }
}
