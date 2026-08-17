//! Pure request-shaping logic for the Tavily tools.
//!
//! Free of any WIT/bindings/network imports so it unit-tests on the host via
//! `cargo test`. `lib.rs` calls these and supplies the actual transport
//! (`extension-host/http`) and secret resolution (`extension-host/secrets`).

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

/// Decoded `tavily_search` input.
#[derive(Debug, Deserialize)]
pub struct SearchInput {
    pub query: String,
    #[serde(default)]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub search_depth: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub include_answer: Option<bool>,
    #[serde(default)]
    pub include_domains: Option<Vec<String>>,
    #[serde(default)]
    pub exclude_domains: Option<Vec<String>>,
    #[serde(default)]
    pub time_range: Option<String>,
}

/// Decoded `tavily_extract` input.
#[derive(Debug, Deserialize)]
pub struct ExtractInput {
    pub urls: Vec<String>,
    #[serde(default)]
    pub extract_depth: Option<String>,
}

/// Build the JSON body for `POST /search`. Applies defaults and validates.
pub fn build_search_body(input: &SearchInput) -> Result<Value, String> {
    if input.query.trim().is_empty() {
        return Err("query must not be empty".to_string());
    }
    let max_results = input.max_results.unwrap_or(5);
    if !(1..=20).contains(&max_results) {
        return Err(format!(
            "max_results must be between 1 and 20, got {max_results}"
        ));
    }
    let mut body = json!({
        "query": input.query,
        "max_results": max_results,
        "search_depth": input.search_depth.as_deref().unwrap_or("basic"),
        "topic": input.topic.as_deref().unwrap_or("general"),
        "include_answer": input.include_answer.unwrap_or(true),
    });
    if let Some(domains) = &input.include_domains {
        body["include_domains"] = json!(domains);
    }
    if let Some(domains) = &input.exclude_domains {
        body["exclude_domains"] = json!(domains);
    }
    if let Some(range) = &input.time_range {
        body["time_range"] = json!(range);
    }
    Ok(body)
}

/// Build the JSON body for `POST /extract`. Applies defaults and validates.
pub fn build_extract_body(input: &ExtractInput) -> Result<Value, String> {
    if input.urls.is_empty() {
        return Err("urls must contain at least one URL".to_string());
    }
    Ok(json!({
        "urls": input.urls,
        "extract_depth": input.extract_depth.as_deref().unwrap_or("basic"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn search_decodes_minimal_and_applies_defaults() {
        let input: SearchInput = serde_json::from_str(r#"{"query":"rust 2024 edition"}"#).unwrap();
        let body = build_search_body(&input).unwrap();
        assert_eq!(body["query"], "rust 2024 edition");
        assert_eq!(body["max_results"], 5);
        assert_eq!(body["search_depth"], "basic");
        assert_eq!(body["topic"], "general");
        assert_eq!(body["include_answer"], true);
        assert!(body.get("include_domains").is_none());
    }

    #[test]
    fn search_passes_through_optional_fields() {
        let input: SearchInput = serde_json::from_str(
            r#"{"query":"q","max_results":3,"search_depth":"advanced","topic":"news","include_answer":false,"include_domains":["a.com"],"time_range":"week"}"#,
        )
        .unwrap();
        let body = build_search_body(&input).unwrap();
        assert_eq!(body["max_results"], 3);
        assert_eq!(body["search_depth"], "advanced");
        assert_eq!(body["topic"], "news");
        assert_eq!(body["include_answer"], false);
        assert_eq!(body["include_domains"], json!(["a.com"]));
        assert_eq!(body["time_range"], "week");
    }

    #[test]
    fn search_rejects_empty_query() {
        let input: SearchInput = serde_json::from_str(r#"{"query":"   "}"#).unwrap();
        assert!(build_search_body(&input).is_err());
    }

    #[test]
    fn search_rejects_out_of_range_max_results() {
        let input: SearchInput = serde_json::from_str(r#"{"query":"q","max_results":0}"#).unwrap();
        assert!(build_search_body(&input).is_err());
        let input: SearchInput = serde_json::from_str(r#"{"query":"q","max_results":99}"#).unwrap();
        assert!(build_search_body(&input).is_err());
    }

    #[test]
    fn extract_decodes_and_applies_defaults() {
        let input: ExtractInput = serde_json::from_str(r#"{"urls":["https://x.com/a"]}"#).unwrap();
        let body = build_extract_body(&input).unwrap();
        assert_eq!(body["urls"], json!(["https://x.com/a"]));
        assert_eq!(body["extract_depth"], "basic");
    }

    #[test]
    fn extract_rejects_empty_urls() {
        let input: ExtractInput = serde_json::from_str(r#"{"urls":[]}"#).unwrap();
        assert!(build_extract_body(&input).is_err());
    }
}
