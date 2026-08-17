//! Pure request-building and response-parsing layer for Notion read operations.
//!
//! Covers `notion_search` (`POST /v1/search`) and
//! `notion_retrieve_block_children` (`GET /v1/blocks/{block_id}/children`).
//! This module is free of any WIT/bindings imports and can be tested on the
//! host with plain `cargo test`. Every request produced here carries the
//! mandatory `Notion-Version` header because both builders delegate to
//! [`crate::notion::base_headers`].

// Copied verbatim from the greentic.notion design extension. The only edit is
// this attribute: several structs are consumed through `Deserialize` to VALIDATE
// a response rather than to read fields, and `HttpReq`'s fields are read only on
// the wasm target, so an off-wasm build sees them as dead. Silencing it here
// keeps the rest of the file diffable against its source.
#![allow(dead_code)]
use std::fmt::Write as FmtWrite;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::notion::{HttpReq, NOTION_BASE, base_headers, validate_id};

// ─── Query-value encoding ─────────────────────────────────────────────────────

/// Percent-encode a string for use as a URL query-parameter value.
///
/// Unreserved characters (`A-Z`, `a-z`, `0-9`, `-`, `_`, `.`, `~`) are passed
/// through as-is; every other byte is encoded as `%XX` (uppercase hex) per
/// RFC 3986 §2.3.
#[must_use]
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric()
            || byte == b'-'
            || byte == b'_'
            || byte == b'.'
            || byte == b'~'
        {
            out.push(byte as char);
        } else {
            write!(out, "%{byte:02X}").expect("write to String is infallible");
        }
    }
    out
}

// ─── Request builders ─────────────────────────────────────────────────────────

/// Build a `POST /search` request.
///
/// - `page_size` is capped at 100 (Notion's maximum).
/// - `query`, `filter`, `sort`, and `start_cursor` are included in the body
///   only when `Some`; omitting them lets Notion use its defaults.
#[must_use]
pub fn build_search_request(
    token: &str,
    query: Option<&str>,
    filter: Option<&Value>,
    sort: Option<&Value>,
    page_size: u32,
    start_cursor: Option<&str>,
) -> HttpReq {
    let url = format!("{NOTION_BASE}/search");
    let mut payload = serde_json::Map::new();
    payload.insert("page_size".to_owned(), json!(page_size.min(100)));
    if let Some(q) = query {
        payload.insert("query".to_owned(), json!(q));
    }
    if let Some(f) = filter {
        payload.insert("filter".to_owned(), f.clone());
    }
    if let Some(s) = sort {
        payload.insert("sort".to_owned(), s.clone());
    }
    if let Some(c) = start_cursor {
        payload.insert("start_cursor".to_owned(), json!(c));
    }

    let body = serde_json::to_vec(&Value::Object(payload))
        .expect("serializing a serde_json::Value is infallible");

    HttpReq {
        method: "POST".to_owned(),
        url,
        headers: base_headers(token),
        body: Some(body),
    }
}

/// Build a `GET /blocks/{block_id}/children` request.
///
/// - `block_id` is validated by [`validate_id`] (path-injection guard; a page
///   ID is also a valid block ID in Notion).
/// - `page_size` is capped at 100 (Notion's maximum).
/// - `start_cursor`, when present, is percent-encoded before appending.
///
/// Returns `Err` when `block_id` fails validation.
pub fn build_retrieve_children_request(
    token: &str,
    block_id: &str,
    page_size: u32,
    start_cursor: Option<&str>,
) -> Result<HttpReq, String> {
    validate_id(block_id)?;

    let path = format!("{NOTION_BASE}/blocks/{block_id}/children");
    let mut query = format!("page_size={}", page_size.min(100));
    if let Some(cursor) = start_cursor {
        write!(query, "&start_cursor={}", percent_encode(cursor))
            .expect("write to String is infallible");
    }
    let url = format!("{path}?{query}");

    Ok(HttpReq {
        method: "GET".to_owned(),
        url,
        headers: base_headers(token),
        body: None,
    })
}

// ─── Response types ───────────────────────────────────────────────────────────

/// One item (page or database) returned by `notion_search`.
#[derive(Debug, PartialEq)]
pub struct SearchHit {
    pub id: String,
    /// Object kind: `"page"` or `"database"`.
    pub object: String,
    /// Direct URL to the object in Notion. Defaults to `""` when absent.
    pub url: String,
}

/// Parsed `notion_search` response.
#[derive(Debug, PartialEq)]
pub struct SearchResult {
    pub results: Vec<SearchHit>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// Parsed `notion_retrieve_block_children` response.
#[derive(Debug)]
pub struct ChildrenResult {
    /// Raw Notion block objects, passed through verbatim.
    pub blocks: Vec<Value>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

// ─── Response parsers ─────────────────────────────────────────────────────────

/// Intermediate serde shape for a single result in the search response.
#[derive(Deserialize)]
struct RawHit {
    id: String,
    #[serde(default)]
    object: String,
    #[serde(default)]
    url: String,
}

/// Intermediate serde shape for the full search response.
#[derive(Deserialize)]
struct RawSearchResponse {
    #[serde(default)]
    results: Vec<RawHit>,
    #[serde(default)]
    has_more: bool,
    next_cursor: Option<String>,
}

/// Parse a JSON string from the Notion `POST /search` endpoint.
pub fn parse_search_response(json: &str) -> Result<SearchResult, String> {
    let raw: RawSearchResponse =
        serde_json::from_str(json).map_err(|e| format!("parse Notion search response: {e}"))?;
    Ok(SearchResult {
        results: raw
            .results
            .into_iter()
            .map(|h| SearchHit {
                id: h.id,
                object: h.object,
                url: h.url,
            })
            .collect(),
        has_more: raw.has_more,
        next_cursor: raw.next_cursor,
    })
}

/// Intermediate serde shape for the block-children list response.
#[derive(Deserialize)]
struct RawChildrenResponse {
    #[serde(default)]
    results: Vec<Value>,
    #[serde(default)]
    has_more: bool,
    next_cursor: Option<String>,
}

/// Parse a JSON string from the Notion `GET /blocks/{id}/children` endpoint.
pub fn parse_children_response(json: &str) -> Result<ChildrenResult, String> {
    let raw: RawChildrenResponse =
        serde_json::from_str(json).map_err(|e| format!("parse Notion children response: {e}"))?;
    Ok(ChildrenResult {
        blocks: raw.results,
        has_more: raw.has_more,
        next_cursor: raw.next_cursor,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── percent_encode ────────────────────────────────────────────────────────

    #[test]
    fn percent_encode_unreserved_passthrough() {
        assert_eq!(percent_encode("abc-123_.~"), "abc-123_.~");
    }

    #[test]
    fn percent_encode_slash_is_encoded() {
        assert_eq!(percent_encode("CUR/2"), "CUR%2F2");
    }

    #[test]
    fn percent_encode_space_is_encoded() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
    }

    // ── build_search_request ──────────────────────────────────────────────────

    #[test]
    fn search_request_minimal() {
        let req = build_search_request("tok", None, None, None, 100, None);
        assert_eq!(req.method, "POST");
        assert_eq!(req.url, "https://api.notion.com/v1/search");
        assert!(
            req.headers
                .contains(&("notion-version".to_owned(), "2022-06-28".to_owned()))
        );
        assert!(
            req.headers
                .contains(&("authorization".to_owned(), "Bearer tok".to_owned()))
        );

        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["page_size"], 100);
        assert!(body.get("query").is_none());
        assert!(body.get("filter").is_none());
        assert!(body.get("sort").is_none());
        assert!(body.get("start_cursor").is_none());
    }

    #[test]
    fn search_request_with_query_filter_and_oversized_page_size() {
        let filter = json!({"value": "page", "property": "object"});
        let req = build_search_request("tok", Some("proj"), Some(&filter), None, 200, None);

        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["page_size"], 100, "page_size must be capped at 100");
        assert_eq!(body["query"], "proj");
        assert_eq!(body["filter"]["value"], "page");
        assert!(body.get("sort").is_none());
        assert!(body.get("start_cursor").is_none());
    }

    #[test]
    fn search_request_with_cursor() {
        let req = build_search_request("tok", None, None, None, 50, Some("CURSOR"));
        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["start_cursor"], "CURSOR");
    }

    #[test]
    fn search_request_with_sort() {
        let sort = json!({"direction": "ascending", "timestamp": "last_edited_time"});
        let req = build_search_request("tok", None, None, Some(&sort), 10, None);
        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["sort"]["direction"], "ascending");
    }

    // ── build_retrieve_children_request ──────────────────────────────────────

    #[test]
    fn retrieve_children_request_shape() {
        let req = build_retrieve_children_request("tok", "blk1", 50, Some("CUR/2")).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(
            req.url,
            "https://api.notion.com/v1/blocks/blk1/children?page_size=50&start_cursor=CUR%2F2"
        );
        assert!(
            req.headers
                .contains(&("notion-version".to_owned(), "2022-06-28".to_owned()))
        );
        assert!(
            req.headers
                .contains(&("authorization".to_owned(), "Bearer tok".to_owned()))
        );
        assert!(req.body.is_none());
    }

    #[test]
    fn retrieve_children_request_no_cursor() {
        let req = build_retrieve_children_request("tok", "blk1", 100, None).unwrap();
        assert_eq!(
            req.url,
            "https://api.notion.com/v1/blocks/blk1/children?page_size=100"
        );
    }

    #[test]
    fn retrieve_children_request_caps_page_size() {
        let req = build_retrieve_children_request("tok", "blk1", 999, None).unwrap();
        assert!(req.url.contains("page_size=100"));
    }

    #[test]
    fn retrieve_children_request_rejects_slash_in_id() {
        assert!(build_retrieve_children_request("tok", "a/b", 100, None).is_err());
    }

    #[test]
    fn retrieve_children_request_rejects_empty_id() {
        assert!(build_retrieve_children_request("tok", "", 100, None).is_err());
    }

    // ── parse_search_response ─────────────────────────────────────────────────

    #[test]
    fn parse_search_full_response() {
        let json = r#"{"results":[{"object":"page","id":"p1","url":"https://n.so/p1"},{"object":"database","id":"d1"}],"has_more":false}"#;
        let result = parse_search_response(json).unwrap();
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.results[0].object, "page");
        assert_eq!(result.results[0].id, "p1");
        assert_eq!(result.results[0].url, "https://n.so/p1");
        assert_eq!(result.results[1].id, "d1");
        assert_eq!(
            result.results[1].url, "",
            "missing url must default to empty"
        );
        assert!(!result.has_more);
        assert!(result.next_cursor.is_none());
    }

    #[test]
    fn parse_search_with_next_cursor() {
        let json = r#"{"results":[],"has_more":true,"next_cursor":"NX"}"#;
        let result = parse_search_response(json).unwrap();
        assert!(result.has_more);
        assert_eq!(result.next_cursor, Some("NX".to_owned()));
    }

    #[test]
    fn parse_search_malformed_json_returns_err() {
        assert!(parse_search_response("not json").is_err());
    }

    // ── parse_children_response ───────────────────────────────────────────────

    #[test]
    fn parse_children_full_response() {
        let json =
            r#"{"results":[{"type":"paragraph","id":"b1"}],"has_more":true,"next_cursor":"NX"}"#;
        let result = parse_children_response(json).unwrap();
        assert_eq!(result.blocks.len(), 1);
        assert_eq!(result.blocks[0]["type"], "paragraph");
        assert!(result.has_more);
        assert_eq!(result.next_cursor, Some("NX".to_owned()));
    }

    #[test]
    fn parse_children_empty_results() {
        let json = r#"{"results":[],"has_more":false}"#;
        let result = parse_children_response(json).unwrap();
        assert!(result.blocks.is_empty());
        assert!(!result.has_more);
        assert!(result.next_cursor.is_none());
    }

    #[test]
    fn parse_children_malformed_json_returns_err() {
        assert!(parse_children_response("{bad}").is_err());
    }
}
