//! Static metadata for the two Tavily tools: names, JSON schemas, capability
//! flags, and the `agentic_worker` metadata blob. Pure (no WIT imports) so the
//! `agentic_worker` opt-in is asserted by a host test.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
pub const TAVILY_SEARCH_TOOL: &str = "tavily_search";
pub const TAVILY_EXTRACT_TOOL: &str = "tavily_extract";

const SEARCH_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["query"],
  "properties": {
    "query": { "type": "string", "description": "The search query" },
    "max_results": { "type": "integer", "minimum": 1, "maximum": 20, "description": "Number of results (default 5)" },
    "search_depth": { "type": "string", "enum": ["basic", "advanced"], "description": "Search depth (default basic)" },
    "topic": { "type": "string", "enum": ["general", "news"], "description": "Search topic (default general)" },
    "include_answer": { "type": "boolean", "description": "Include Tavily's summarised answer (default true)" },
    "include_domains": { "type": "array", "items": { "type": "string" }, "description": "Restrict to these domains" },
    "exclude_domains": { "type": "array", "items": { "type": "string" }, "description": "Exclude these domains" },
    "time_range": { "type": "string", "enum": ["day", "week", "month", "year"], "description": "Recency window" }
  }
}"#;

const SEARCH_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["results", "query"],
  "properties": {
    "answer": { "type": ["string", "null"] },
    "results": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "title": { "type": "string" },
          "url": { "type": "string" },
          "content": { "type": "string" },
          "score": { "type": "number" }
        }
      }
    },
    "query": { "type": "string" }
  }
}"#;

const SEARCH_AW_META: &str = r#"{
  "usage_hint": "Search the live web for current facts or anything outside the model's knowledge. Provide a query; optionally tune max_results, search_depth, topic, and domain filters. Returns a summarised answer plus ranked results with URLs and snippets.",
  "examples": [
    {
      "when": "the agentic worker needs current information from the web",
      "input": { "query": "latest stable Rust release version", "max_results": 5 }
    }
  ],
  "side_effects": "read",
  "cost": "medium",
  "confirmation_required": false
}"#;

const EXTRACT_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["urls"],
  "properties": {
    "urls": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "URLs to extract clean content from" },
    "extract_depth": { "type": "string", "enum": ["basic", "advanced"], "description": "Extraction depth (default basic)" }
  }
}"#;

const EXTRACT_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["results", "failed_results"],
  "properties": {
    "results": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "url": { "type": "string" },
          "raw_content": { "type": "string" }
        }
      }
    },
    "failed_results": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "url": { "type": "string" },
          "error": { "type": "string" }
        }
      }
    }
  }
}"#;

const EXTRACT_AW_META: &str = r#"{
  "usage_hint": "Fetch clean, readable content from one or more known URLs (e.g. links returned by tavily_search). Returns extracted text per URL plus a list of URLs that failed.",
  "examples": [
    {
      "when": "the agentic worker has a URL and needs its full readable content",
      "input": { "urls": ["https://example.com/article"] }
    }
  ],
  "side_effects": "read",
  "cost": "medium",
  "confirmation_required": false
}"#;

/// Plain (non-WIT) description of one tool definition.
pub struct ToolMeta {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema_json: &'static str,
    pub output_schema_json: &'static str,
    pub capabilities: Vec<String>,
    pub agentic_worker_metadata: &'static str,
}

#[must_use]
pub fn tavily_search_tool() -> ToolMeta {
    ToolMeta {
        name: TAVILY_SEARCH_TOOL,
        description: "Search the web with Tavily and return an optional summarised answer plus \
                      ranked results. The Tavily API key is injected by the host and never returned.",
        input_schema_json: SEARCH_INPUT_SCHEMA,
        output_schema_json: SEARCH_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: SEARCH_AW_META,
    }
}

#[must_use]
pub fn tavily_extract_tool() -> ToolMeta {
    ToolMeta {
        name: TAVILY_EXTRACT_TOOL,
        description: "Extract clean, readable content from known URLs using Tavily. The Tavily \
                      API key is injected by the host and never returned.",
        input_schema_json: EXTRACT_INPUT_SCHEMA,
        output_schema_json: EXTRACT_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: EXTRACT_AW_META,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_tools_declare_agentic_worker_capability() {
        for tool in [tavily_search_tool(), tavily_extract_tool()] {
            assert!(
                tool.capabilities.iter().any(|cap| cap == "agentic_worker"),
                "{} must opt into the agentic worker",
                tool.name
            );
        }
    }

    #[test]
    fn tool_names_match_constants() {
        assert_eq!(tavily_search_tool().name, "tavily_search");
        assert_eq!(tavily_extract_tool().name, "tavily_extract");
    }

    #[test]
    fn schemas_and_metadata_are_valid_json() {
        for tool in [tavily_search_tool(), tavily_extract_tool()] {
            serde_json::from_str::<serde_json::Value>(tool.input_schema_json)
                .expect("input schema is valid JSON");
            serde_json::from_str::<serde_json::Value>(tool.output_schema_json)
                .expect("output schema is valid JSON");
            let meta: serde_json::Value = serde_json::from_str(tool.agentic_worker_metadata)
                .expect("aw metadata is valid JSON");
            assert_eq!(meta["side_effects"], "read");
            assert_eq!(meta["confirmation_required"], false);
        }
    }
}
