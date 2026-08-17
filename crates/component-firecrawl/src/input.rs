//! Pure request-shaping logic for the Firecrawl web tools. No WIT/network
//! imports, so it unit-tests on the host. `lib.rs` supplies the transport.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several input structs exist for the TOOL surface
// and are unused by the node surface, and `HttpReq`'s fields are read only on
// the wasm target. Silencing it here keeps the rest of the file diffable
// against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
pub struct ScrapeInput {
    pub url: String,
    #[serde(default)]
    pub formats: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ExtractInput {
    pub url: String,
    #[serde(default)]
    pub schema: Option<Value>,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScreenshotInput {
    pub url: String,
    #[serde(default)]
    pub full_page: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SearchInput {
    pub query: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CrawlInput {
    pub url: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CrawlResultInput {
    pub job_id: String,
}

#[derive(Debug, Deserialize)]
pub struct BrowserTaskInput {
    pub url: String,
    #[serde(default)]
    pub actions: Vec<Value>,
    #[serde(default)]
    pub formats: Option<Vec<String>>,
}

fn require_url(url: &str) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("url must not be empty".to_string());
    }
    Ok(())
}

/// `POST /v2/scrape` body for `web_scrape`.
pub fn build_scrape_body(input: &ScrapeInput) -> Result<Value, String> {
    require_url(&input.url)?;
    let formats = input
        .formats
        .clone()
        .unwrap_or_else(|| vec!["markdown".to_string()]);
    Ok(json!({ "url": input.url, "formats": formats }))
}

/// `POST /v2/scrape` body for `web_extract` (json format, LLM-guided).
pub fn build_extract_body(input: &ExtractInput) -> Result<Value, String> {
    require_url(&input.url)?;
    if input.schema.is_none() && input.prompt.as_deref().unwrap_or("").trim().is_empty() {
        return Err("web_extract requires a `schema` and/or a non-empty `prompt`".to_string());
    }
    let mut json_options = serde_json::Map::new();
    if let Some(schema) = &input.schema {
        json_options.insert("schema".to_string(), schema.clone());
    }
    if let Some(prompt) = &input.prompt
        && !prompt.trim().is_empty()
    {
        json_options.insert("prompt".to_string(), json!(prompt));
    }
    Ok(json!({
        "url": input.url,
        "formats": ["json"],
        "jsonOptions": Value::Object(json_options)
    }))
}

/// `POST /v2/scrape` body for `web_screenshot`.
pub fn build_screenshot_body(input: &ScreenshotInput) -> Result<Value, String> {
    require_url(&input.url)?;
    Ok(json!({
        "url": input.url,
        "formats": [{ "type": "screenshot", "fullPage": input.full_page.unwrap_or(false) }]
    }))
}

/// `POST /v2/search` body for `web_search`.
pub fn build_search_body(input: &SearchInput) -> Result<Value, String> {
    if input.query.trim().is_empty() {
        return Err("query must not be empty".to_string());
    }
    let limit = input.limit.unwrap_or(5);
    if !(1..=20).contains(&limit) {
        return Err(format!("limit must be between 1 and 20, got {limit}"));
    }
    Ok(json!({ "query": input.query, "limit": limit }))
}

/// `POST /v2/crawl` body for `web_crawl_start`.
pub fn build_crawl_body(input: &CrawlInput) -> Result<Value, String> {
    require_url(&input.url)?;
    let limit = input.limit.unwrap_or(10);
    if !(1..=100).contains(&limit) {
        return Err(format!("limit must be between 1 and 100, got {limit}"));
    }
    Ok(json!({ "url": input.url, "limit": limit }))
}

/// `POST /v2/scrape` body for `browser_task` (actions[] then final content).
pub fn build_browser_task_body(input: &BrowserTaskInput) -> Result<Value, String> {
    require_url(&input.url)?;
    if input.actions.is_empty() {
        return Err("browser_task requires at least one action".to_string());
    }
    let formats = input
        .formats
        .clone()
        .unwrap_or_else(|| vec!["markdown".to_string()]);
    Ok(json!({ "url": input.url, "actions": input.actions, "formats": formats }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrape_defaults_formats_to_markdown() {
        let input: ScrapeInput = serde_json::from_str(r#"{"url":"https://x.com"}"#).unwrap();
        let body = build_scrape_body(&input).unwrap();
        assert_eq!(body["url"], "https://x.com");
        assert_eq!(body["formats"], json!(["markdown"]));
    }

    #[test]
    fn scrape_rejects_empty_url() {
        let input: ScrapeInput = serde_json::from_str(r#"{"url":"  "}"#).unwrap();
        assert!(build_scrape_body(&input).is_err());
    }

    #[test]
    fn extract_requires_schema_or_prompt() {
        let input: ExtractInput = serde_json::from_str(r#"{"url":"https://x.com"}"#).unwrap();
        assert!(build_extract_body(&input).is_err());
        let input: ExtractInput =
            serde_json::from_str(r#"{"url":"https://x.com","prompt":"get title"}"#).unwrap();
        let body = build_extract_body(&input).unwrap();
        assert_eq!(body["formats"], json!(["json"]));
        assert_eq!(body["jsonOptions"]["prompt"], "get title");
    }

    #[test]
    fn extract_passes_schema_through() {
        let input: ExtractInput = serde_json::from_str(
            r#"{"url":"https://x.com","schema":{"type":"object","properties":{"a":{"type":"string"}}}}"#,
        )
        .unwrap();
        let body = build_extract_body(&input).unwrap();
        assert_eq!(body["jsonOptions"]["schema"]["type"], "object");
    }

    #[test]
    fn screenshot_sets_fullpage_flag() {
        let input: ScreenshotInput =
            serde_json::from_str(r#"{"url":"https://x.com","full_page":true}"#).unwrap();
        let body = build_screenshot_body(&input).unwrap();
        assert_eq!(body["formats"][0]["type"], "screenshot");
        assert_eq!(body["formats"][0]["fullPage"], true);
    }

    #[test]
    fn search_defaults_and_bounds_limit() {
        let input: SearchInput = serde_json::from_str(r#"{"query":"rust"}"#).unwrap();
        assert_eq!(build_search_body(&input).unwrap()["limit"], 5);
        let input: SearchInput = serde_json::from_str(r#"{"query":"rust","limit":0}"#).unwrap();
        assert!(build_search_body(&input).is_err());
        let input: SearchInput = serde_json::from_str(r#"{"query":"  "}"#).unwrap();
        assert!(build_search_body(&input).is_err());
    }

    #[test]
    fn crawl_defaults_and_bounds_limit() {
        let input: CrawlInput = serde_json::from_str(r#"{"url":"https://x.com"}"#).unwrap();
        assert_eq!(build_crawl_body(&input).unwrap()["limit"], 10);
        let input: CrawlInput =
            serde_json::from_str(r#"{"url":"https://x.com","limit":999}"#).unwrap();
        assert!(build_crawl_body(&input).is_err());
    }

    #[test]
    fn browser_task_requires_actions() {
        let input: BrowserTaskInput =
            serde_json::from_str(r#"{"url":"https://x.com","actions":[]}"#).unwrap();
        assert!(build_browser_task_body(&input).is_err());
        let input: BrowserTaskInput = serde_json::from_str(
            r##"{"url":"https://x.com","actions":[{"type":"click","selector":"#go"}]}"##,
        )
        .unwrap();
        let body = build_browser_task_body(&input).unwrap();
        assert_eq!(body["actions"][0]["type"], "click");
        assert_eq!(body["formats"], json!(["markdown"]));
    }

    #[test]
    fn crawl_result_decodes_job_id() {
        let input: CrawlResultInput = serde_json::from_str(r#"{"job_id":"abc-123"}"#).unwrap();
        assert_eq!(input.job_id, "abc-123");
    }

    #[test]
    fn search_rejects_over_upper_bound() {
        let input: SearchInput = serde_json::from_str(r#"{"query":"q","limit":21}"#).unwrap();
        assert!(build_search_body(&input).is_err());
    }

    #[test]
    fn crawl_rejects_zero_limit() {
        let input: CrawlInput =
            serde_json::from_str(r#"{"url":"https://x.com","limit":0}"#).unwrap();
        assert!(build_crawl_body(&input).is_err());
    }

    #[test]
    fn screenshot_defaults_fullpage_false() {
        let input: ScreenshotInput = serde_json::from_str(r#"{"url":"https://x.com"}"#).unwrap();
        let body = build_screenshot_body(&input).unwrap();
        assert_eq!(body["formats"][0]["fullPage"], false);
    }

    #[test]
    fn extract_schema_only_omits_prompt_key() {
        let input: ExtractInput =
            serde_json::from_str(r#"{"url":"https://x.com","schema":{"type":"object"}}"#).unwrap();
        let body = build_extract_body(&input).unwrap();
        assert!(body["jsonOptions"].get("prompt").is_none());
    }

    #[test]
    fn extract_blank_prompt_with_schema_omits_prompt_key() {
        let input: ExtractInput = serde_json::from_str(
            r#"{"url":"https://x.com","schema":{"type":"object"},"prompt":"   "}"#,
        )
        .unwrap();
        let body = build_extract_body(&input).unwrap();
        assert!(body["jsonOptions"].get("prompt").is_none());
    }
}
