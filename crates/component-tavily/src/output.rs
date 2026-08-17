//! Pure response-mapping logic for the Tavily tools.
//!
//! Free of WIT/bindings imports so it unit-tests on the host. We deserialize
//! the Tavily payloads with permissive structs (unknown fields ignored) and
//! re-serialize the stable shape the tool returns to the agent.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub score: f64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(default)]
    pub results: Vec<SearchResult>,
    pub query: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtractResult {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub raw_content: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct FailedResult {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtractOutput {
    #[serde(default)]
    pub results: Vec<ExtractResult>,
    #[serde(default)]
    pub failed_results: Vec<FailedResult>,
}

/// Parse a Tavily `/search` response body into [`SearchOutput`].
pub fn map_search_response(body: &[u8]) -> Result<SearchOutput, String> {
    serde_json::from_slice::<SearchOutput>(body)
        .map_err(|error| format!("decode tavily search response: {error}"))
}

/// Parse a Tavily `/extract` response body into [`ExtractOutput`].
pub fn map_extract_response(body: &[u8]) -> Result<ExtractOutput, String> {
    serde_json::from_slice::<ExtractOutput>(body)
        .map_err(|error| format!("decode tavily extract response: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_search_response_with_answer_and_results() {
        let raw = br#"{
          "query": "rust",
          "answer": "Rust is a systems language.",
          "results": [
            {"title": "Rust", "url": "https://rust-lang.org", "content": "snippet", "score": 0.98}
          ],
          "response_time": 1.2
        }"#;
        let out = map_search_response(raw).unwrap();
        assert_eq!(out.query, "rust");
        assert_eq!(out.answer.as_deref(), Some("Rust is a systems language."));
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].url, "https://rust-lang.org");
        assert!((out.results[0].score - 0.98).abs() < f64::EPSILON);
    }

    #[test]
    fn maps_search_response_without_answer() {
        let raw = br#"{"query":"q","results":[]}"#;
        let out = map_search_response(raw).unwrap();
        assert!(out.answer.is_none());
        assert!(out.results.is_empty());
    }

    #[test]
    fn search_response_malformed_json_is_err() {
        assert!(map_search_response(b"{not json").is_err());
    }

    #[test]
    fn maps_extract_response_with_results_and_failures() {
        let raw = br#"{
          "results": [{"url": "https://x.com/a", "raw_content": "hello"}],
          "failed_results": [{"url": "https://x.com/b", "error": "timeout"}]
        }"#;
        let out = map_extract_response(raw).unwrap();
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].raw_content, "hello");
        assert_eq!(out.failed_results.len(), 1);
        assert_eq!(out.failed_results[0].error, "timeout");
    }

    #[test]
    fn extract_response_missing_failed_defaults_empty() {
        let raw = br#"{"results":[{"url":"u","raw_content":"c"}]}"#;
        let out = map_extract_response(raw).unwrap();
        assert!(out.failed_results.is_empty());
    }

    #[test]
    fn extract_response_malformed_json_is_err() {
        assert!(map_extract_response(b"{bad").is_err());
    }
}
