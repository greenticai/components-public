//! Static metadata for the seven Firecrawl-backed web tools: names, JSON
//! schemas, capability flags, and the `agentic_worker` metadata blob. Pure (no
//! WIT imports) so the `agentic_worker` opt-in is asserted by a host test.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several input structs exist for the TOOL surface
// and are unused by the node surface, and `HttpReq`'s fields are read only on
// the wasm target. Silencing it here keeps the rest of the file diffable
// against its source.
#![allow(dead_code)]
pub const WEB_SCRAPE_TOOL: &str = "web_scrape";
pub const WEB_EXTRACT_TOOL: &str = "web_extract";
pub const WEB_SCREENSHOT_TOOL: &str = "web_screenshot";
pub const WEB_SEARCH_TOOL: &str = "web_search";
pub const WEB_CRAWL_TOOL: &str = "web_crawl_start";
pub const WEB_CRAWL_RESULT_TOOL: &str = "web_crawl_result";
pub const BROWSER_TASK_TOOL: &str = "browser_task";

/// Plain (non-WIT) description of one tool definition.
pub struct ToolMeta {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema_json: &'static str,
    pub output_schema_json: &'static str,
    pub capabilities: Vec<String>,
    pub agentic_worker_metadata: &'static str,
}

// --- Schema and agentic-worker metadata consts ---

const SCRAPE_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["url"],
  "properties": {
    "url": { "type": "string", "description": "Absolute URL of the page to scrape" },
    "formats": { "type": "array", "items": { "type": "string", "enum": ["markdown", "html", "links"] }, "description": "Content formats to return (default [\"markdown\"])" }
  }
}"#;
const SCRAPE_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "markdown": { "type": ["string", "null"] },
    "html": { "type": ["string", "null"] },
    "links": { "type": ["array", "null"], "items": { "type": "string" } },
    "metadata": { "type": ["object", "null"] }
  }
}"#;
const SCRAPE_AW_META: &str = r#"{"usage_hint":"Read a single web page and get clean markdown/html/links. Provide an absolute url; optionally choose formats.","examples":[{"when":"the worker needs the readable content of a known page","input":{"url":"https://example.com/article","formats":["markdown"]}}],"side_effects":"read","cost":"medium","confirmation_required":false}"#;

const EXTRACT_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["url"],
  "properties": {
    "url": { "type": "string", "description": "Absolute URL to extract structured data from" },
    "schema": { "type": "object", "description": "JSON Schema describing the fields to extract" },
    "prompt": { "type": "string", "description": "Natural-language instruction for what to extract" }
  }
}"#;
const EXTRACT_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["json"],
  "properties": { "json": { "type": "object", "description": "Extracted structured data" } }
}"#;
const EXTRACT_AW_META: &str = r#"{"usage_hint":"Extract structured JSON from a page. Supply a JSON schema and/or a prompt describing the fields you want.","examples":[{"when":"the worker needs specific fields (price, title) from a product page","input":{"url":"https://shop.example.com/item/1","schema":{"type":"object","properties":{"price":{"type":"number"},"title":{"type":"string"}}}}}],"side_effects":"read","cost":"medium","confirmation_required":false}"#;

const SCREENSHOT_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["url"],
  "properties": {
    "url": { "type": "string", "description": "Absolute URL to screenshot" },
    "full_page": { "type": "boolean", "description": "Capture the full scrollable page (default false)" }
  }
}"#;
const SCREENSHOT_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["screenshot_url"],
  "properties": { "screenshot_url": { "type": "string" } }
}"#;
const SCREENSHOT_AW_META: &str = r#"{"usage_hint":"Capture a screenshot of a page for visual verification. Returns a URL to the image.","examples":[{"when":"the worker needs visual proof of a page state","input":{"url":"https://example.com","full_page":true}}],"side_effects":"read","cost":"medium","confirmation_required":false}"#;

const SEARCH_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["query"],
  "properties": {
    "query": { "type": "string", "description": "The web search query" },
    "limit": { "type": "integer", "minimum": 1, "maximum": 20, "description": "Number of results (default 5)" }
  }
}"#;
const SEARCH_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["results"],
  "properties": {
    "results": { "type": "array", "items": { "type": "object", "properties": {
      "url": { "type": "string" }, "title": { "type": "string" }, "description": { "type": "string" }
    } } }
  }
}"#;
const SEARCH_AW_META: &str = r#"{"usage_hint":"Search the live web and get ranked results with URLs. Follow up with web_scrape or web_extract on a result URL.","examples":[{"when":"the worker needs current information from the web","input":{"query":"latest stable Rust release","limit":5}}],"side_effects":"read","cost":"medium","confirmation_required":false}"#;

const CRAWL_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["url"],
  "properties": {
    "url": { "type": "string", "description": "Root URL to crawl from" },
    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max pages to crawl (default 10)" }
  }
}"#;
const CRAWL_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["job_id", "status"],
  "properties": { "job_id": { "type": "string" }, "status": { "type": "string" } }
}"#;
const CRAWL_AW_META: &str = r#"{"usage_hint":"Start crawling a site. Returns a job_id immediately; call web_crawl_result with it until status is completed. Crawls are expensive.","examples":[{"when":"the worker needs many pages from one site","input":{"url":"https://docs.example.com","limit":25}}],"side_effects":"read","cost":"high","confirmation_required":false}"#;

const CRAWL_RESULT_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["job_id"],
  "properties": { "job_id": { "type": "string", "description": "Job id returned by web_crawl_start" } }
}"#;
const CRAWL_RESULT_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["status"],
  "properties": {
    "status": { "type": "string" },
    "completed": { "type": "integer" },
    "total": { "type": "integer" },
    "data": { "type": ["array", "null"] }
  }
}"#;
const CRAWL_RESULT_AW_META: &str = r#"{"usage_hint":"Poll a crawl job by job_id. Re-call until status is completed, then read data.","examples":[{"when":"a crawl job was started and the worker is waiting for pages","input":{"job_id":"abc-123"}}],"side_effects":"read","cost":"low","confirmation_required":false}"#;

const BROWSER_TASK_INPUT_SCHEMA: &str = r##"{
  "type": "object",
  "required": ["url", "actions"],
  "properties": {
    "url": { "type": "string", "description": "Absolute URL to open" },
    "actions": { "type": "array", "minItems": 1, "items": { "type": "object" },
      "description": "Ordered Firecrawl actions, e.g. {\"type\":\"write\",\"selector\":\"#email\",\"text\":\"a@b.c\"}, {\"type\":\"click\",\"selector\":\"#submit\"}, {\"type\":\"wait\",\"milliseconds\":1000}, {\"type\":\"screenshot\"}, {\"type\":\"scrape\"}" },
    "formats": { "type": "array", "items": { "type": "string", "enum": ["markdown", "html"] }, "description": "Final content formats (default [\"markdown\"])" }
  }
}"##;
const BROWSER_TASK_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "markdown": { "type": ["string", "null"] },
    "html": { "type": ["string", "null"] },
    "screenshot": { "type": ["string", "null"] },
    "metadata": { "type": ["object", "null"] }
  }
}"#;
const BROWSER_TASK_AW_META: &str = r##"{"usage_hint":"Drive an interactive page: supply an ordered list of actions (write/click/press/wait/scroll/screenshot/scrape). Use for login, form-fill, multi-step flows. Can mutate remote state.","examples":[{"when":"the worker must log in and read a protected page","input":{"url":"https://portal.example.com/login","actions":[{"type":"write","selector":"#user","text":"agent"},{"type":"write","selector":"#pass","text":"secret"},{"type":"click","selector":"#login"},{"type":"wait","milliseconds":1500},{"type":"scrape"}]}}],"side_effects":"write","cost":"high","confirmation_required":false}"##;

// --- Builders ---

#[must_use]
pub fn web_scrape_tool() -> ToolMeta {
    ToolMeta {
        name: WEB_SCRAPE_TOOL,
        description: "Fetch one web page and return its content as markdown/html/links. \
                      Firecrawl runs a headless browser; the API key is injected by the host and never returned.",
        input_schema_json: SCRAPE_INPUT_SCHEMA,
        output_schema_json: SCRAPE_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: SCRAPE_AW_META,
    }
}

#[must_use]
pub fn web_extract_tool() -> ToolMeta {
    ToolMeta {
        name: WEB_EXTRACT_TOOL,
        description: "Extract structured JSON from one page using an LLM, guided by a JSON schema \
                      and/or a prompt. The API key is injected by the host and never returned.",
        input_schema_json: EXTRACT_INPUT_SCHEMA,
        output_schema_json: EXTRACT_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: EXTRACT_AW_META,
    }
}

#[must_use]
pub fn web_screenshot_tool() -> ToolMeta {
    ToolMeta {
        name: WEB_SCREENSHOT_TOOL,
        description: "Capture a screenshot of a web page (optionally full-page) and return its URL. \
                      The API key is injected by the host and never returned.",
        input_schema_json: SCREENSHOT_INPUT_SCHEMA,
        output_schema_json: SCREENSHOT_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: SCREENSHOT_AW_META,
    }
}

#[must_use]
pub fn web_search_tool() -> ToolMeta {
    ToolMeta {
        name: WEB_SEARCH_TOOL,
        description: "Search the web and return ranked results (title, url, description). \
                      The API key is injected by the host and never returned.",
        input_schema_json: SEARCH_INPUT_SCHEMA,
        output_schema_json: SEARCH_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: SEARCH_AW_META,
    }
}

#[must_use]
pub fn web_crawl_tool() -> ToolMeta {
    ToolMeta {
        name: WEB_CRAWL_TOOL,
        description: "Start crawling a site from a root URL (bounded by `limit`). Returns a job_id; \
                      poll web_crawl_result to fetch pages. The API key is injected by the host and never returned.",
        input_schema_json: CRAWL_INPUT_SCHEMA,
        output_schema_json: CRAWL_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: CRAWL_AW_META,
    }
}

#[must_use]
pub fn web_crawl_result_tool() -> ToolMeta {
    ToolMeta {
        name: WEB_CRAWL_RESULT_TOOL,
        description: "Fetch the status and results of a crawl job started by web_crawl_start. \
                      Re-call until status is `completed`. The API key is injected by the host and never returned.",
        input_schema_json: CRAWL_RESULT_INPUT_SCHEMA,
        output_schema_json: CRAWL_RESULT_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: CRAWL_RESULT_AW_META,
    }
}

#[must_use]
pub fn browser_task_tool() -> ToolMeta {
    ToolMeta {
        name: BROWSER_TASK_TOOL,
        description: "Run an interactive browser flow on one page: a sequence of actions \
                      (click/write/press/wait/scroll/screenshot/scrape), then return the final content. \
                      Use for logins, form-fill, and multi-step transactions. The API key is injected by the host and never returned.",
        input_schema_json: BROWSER_TASK_INPUT_SCHEMA,
        output_schema_json: BROWSER_TASK_OUTPUT_SCHEMA,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: BROWSER_TASK_AW_META,
    }
}

#[must_use]
pub fn all_tools() -> [ToolMeta; 7] {
    [
        web_scrape_tool(),
        web_extract_tool(),
        web_screenshot_tool(),
        web_search_tool(),
        web_crawl_tool(),
        web_crawl_result_tool(),
        browser_task_tool(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_declare_agentic_worker_capability() {
        for tool in all_tools() {
            assert!(
                tool.capabilities.iter().any(|cap| cap == "agentic_worker"),
                "{} must opt into the agentic worker",
                tool.name
            );
        }
    }

    #[test]
    fn all_seven_tool_names_are_unique_and_expected() {
        let names: Vec<&str> = all_tools().iter().map(|t| t.name).collect();
        assert_eq!(names.len(), 7);
        for expected in [
            "web_scrape",
            "web_extract",
            "web_screenshot",
            "web_search",
            "web_crawl_start",
            "web_crawl_result",
            "browser_task",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
    }

    #[test]
    fn schemas_and_metadata_are_valid_json() {
        for tool in all_tools() {
            serde_json::from_str::<serde_json::Value>(tool.input_schema_json)
                .unwrap_or_else(|e| panic!("{} input schema invalid: {e}", tool.name));
            serde_json::from_str::<serde_json::Value>(tool.output_schema_json)
                .unwrap_or_else(|e| panic!("{} output schema invalid: {e}", tool.name));
            let meta: serde_json::Value = serde_json::from_str(tool.agentic_worker_metadata)
                .unwrap_or_else(|e| panic!("{} aw metadata invalid: {e}", tool.name));
            assert_eq!(meta["confirmation_required"], false);
        }
    }
}
