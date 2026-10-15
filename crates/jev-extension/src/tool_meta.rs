//! Static metadata for the `decide` tool. Pure (no WIT imports) so the
//! `agentic_worker` opt-in is asserted by a host test.

pub const DECIDE_TOOL: &str = "decide";

const DECIDE_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["state", "questions"],
  "properties": {
    "state": { "description": "The data to evaluate: text or a JSON object.", "type": ["string", "object"] },
    "questions": {
      "type": "object",
      "description": "Questions keyed by an id you choose; each answer comes back under the same id. Ask one narrow question per entry.",
      "additionalProperties": {
        "type": "object",
        "required": ["type", "instructions"],
        "properties": {
          "type": { "type": "string", "enum": ["choice", "score", "noul"] },
          "instructions": { "type": "string", "description": "What to decide." },
          "criteria": { "description": "choice: object of option key to description (two or more). score: ordered array of level descriptions (two or more). noul: not used." }
        }
      }
    },
    "model": { "type": "string", "description": "Jev model id. Defaults to jev-latest." }
  }
}"#;

const DECIDE_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["answers"],
  "properties": {
    "model": { "type": "string" },
    "answers": { "type": "object", "description": "Per question id: choice/score/noul with probabilities and confidence." },
    "usage": { "type": "object" }
  }
}"#;

const DECIDE_AW_META: &str = r#"{"usage_hint":"Get a typed decision with calibrated probabilities from Jev (TypeSafe AI). Ask narrow questions: choice (pick one option), score (rate on ordered levels) or noul (is this true). Act on the answer only when its confidence is high; otherwise escalate to a human.","examples":[{"when":"the worker must classify a message before acting","input":{"state":"I want my money back","questions":{"intent":{"type":"choice","instructions":"What does the sender want?","criteria":{"refund":"asks for money back","other":"anything else"}}}}}],"side_effects":"none","cost":"low","confirmation_required":false}"#;

pub struct ToolMeta {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema_json: &'static str,
    pub output_schema_json: &'static str,
    pub capabilities: Vec<String>,
    pub agentic_worker_metadata: &'static str,
}

#[must_use]
pub fn decide_tool() -> ToolMeta {
    ToolMeta {
        name: DECIDE_TOOL,
        description: "Ask Jev (TypeSafe AI) typed questions about some data and get choices, scores or yes/no probabilities with confidence. The API key is injected by the host and never returned.",
        input_schema_json: DECIDE_INPUT_SCHEMA,
        output_schema_json: DECIDE_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: DECIDE_AW_META,
    }
}

#[must_use]
pub fn all_tools() -> [ToolMeta; 1] {
    [decide_tool()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_declares_agentic_worker_capability() {
        let t = decide_tool();
        assert_eq!(t.name, "decide");
        assert!(t.capabilities.iter().any(|c| c == "agentic_worker"));
    }

    #[test]
    fn schemas_and_metadata_are_valid_json() {
        let t = decide_tool();
        serde_json::from_str::<serde_json::Value>(t.input_schema_json).expect("input schema JSON");
        serde_json::from_str::<serde_json::Value>(t.output_schema_json)
            .expect("output schema JSON");
        let meta: serde_json::Value =
            serde_json::from_str(t.agentic_worker_metadata).expect("aw metadata JSON");
        assert_eq!(meta["side_effects"], "none");
        assert_eq!(meta["confirmation_required"], false);
    }
}
