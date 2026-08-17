//! Pure request-building and response-parsing layer for the Notion REST API.
//!
//! This module is free of any WIT/bindings imports and can be tested on the
//! host with plain `cargo test`. All HTTP traffic must include the
//! `Notion-Version` header, which [`base_headers`] enforces — both request
//! builders call it so neither can omit the header.

// Copied verbatim from the greentic.notion design extension. The only edit is
// this attribute: several structs are consumed through `Deserialize` to VALIDATE
// a response rather than to read fields, and `HttpReq`'s fields are read only on
// the wasm target, so an off-wasm build sees them as dead. Silencing it here
// keeps the rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

// ─── Constants ────────────────────────────────────────────────────────────────

pub const NOTION_BASE: &str = "https://api.notion.com/v1";
pub const NOTION_VERSION: &str = "2022-06-28";

// ─── Plain HTTP request type (WIT-free) ───────────────────────────────────────

/// A self-contained HTTP request built by this module and handed to `lib.rs`
/// for dispatching via the host `http::fetch` import.
pub struct HttpReq {
    pub method: String,
    pub url: String,
    /// Headers as lowercase-name / value pairs (Notion requires lower-case per
    /// HTTP/2 convention; the host accepts either case).
    pub headers: Vec<(String, String)>,
    /// Serialized JSON body, present on POST requests.
    pub body: Option<Vec<u8>>,
}

// ─── Response types ───────────────────────────────────────────────────────────

/// One page / database row returned by `notion_query_database`.
pub struct PageHit {
    pub id: String,
    /// Direct URL to the page in Notion. May be empty for some object kinds.
    pub url: String,
    /// Raw Notion typed-property map, passed through verbatim.
    pub properties: Value,
}

/// Parsed `notion_query_database` response.
pub struct QueryResult {
    pub results: Vec<PageHit>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// Parsed `notion_create_page` response.
pub struct PageResult {
    pub id: String,
    pub url: String,
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// Build the three headers that every Notion request must carry.
///
/// Order: `authorization`, `notion-version`, `content-type`.
pub(crate) fn base_headers(token: &str) -> Vec<(String, String)> {
    vec![
        ("authorization".to_owned(), format!("Bearer {token}")),
        ("notion-version".to_owned(), NOTION_VERSION.to_owned()),
        ("content-type".to_owned(), "application/json".to_owned()),
    ]
}

/// Validate a Notion object ID used in a URL path segment.
///
/// Returns `Err` when `id` is empty, contains a `/` (path-injection), contains
/// any ASCII whitespace, or contains any Unicode control character.
pub fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err(format!("invalid Notion id: {id:?}"));
    }
    if id
        .chars()
        .any(|c| c == '/' || c.is_whitespace() || c.is_control())
    {
        return Err(format!("invalid Notion id: {id:?}"));
    }
    Ok(())
}

// ─── Request builders ─────────────────────────────────────────────────────────

/// Build a `POST /databases/{database_id}/query` request.
///
/// - `page_size` is capped at 100 (Notion's maximum).
/// - `filter`, `sorts`, and `start_cursor` are included in the body only when
///   `Some`; omitting them lets Notion use its defaults.
pub fn build_query_database_request(
    token: &str,
    database_id: &str,
    filter: Option<&Value>,
    sorts: Option<&Value>,
    page_size: u32,
    start_cursor: Option<&str>,
) -> Result<HttpReq, String> {
    validate_id(database_id)?;

    let url = format!("{NOTION_BASE}/databases/{database_id}/query");
    let mut payload = serde_json::Map::new();
    payload.insert("page_size".to_owned(), json!(page_size.min(100)));
    if let Some(f) = filter {
        payload.insert("filter".to_owned(), f.clone());
    }
    if let Some(s) = sorts {
        payload.insert("sorts".to_owned(), s.clone());
    }
    if let Some(c) = start_cursor {
        payload.insert("start_cursor".to_owned(), json!(c));
    }

    let body = serde_json::to_vec(&Value::Object(payload))
        .expect("serializing a serde_json::Value is infallible");

    Ok(HttpReq {
        method: "POST".to_owned(),
        url,
        headers: base_headers(token),
        body: Some(body),
    })
}

/// Build a `POST /pages` request that creates a new page in a database.
///
/// - `properties` is the Notion typed-property map (passed through verbatim).
/// - `children` (optional) is a Notion block array, included only when `Some`.
pub fn build_create_page_request(
    token: &str,
    parent_database_id: &str,
    properties: &serde_json::Map<String, Value>,
    children: Option<&Value>,
) -> Result<HttpReq, String> {
    validate_id(parent_database_id)?;

    let url = format!("{NOTION_BASE}/pages");
    let mut payload = serde_json::Map::new();
    payload.insert(
        "parent".to_owned(),
        json!({ "database_id": parent_database_id }),
    );
    payload.insert("properties".to_owned(), Value::Object(properties.clone()));
    if let Some(ch) = children {
        payload.insert("children".to_owned(), ch.clone());
    }

    let body = serde_json::to_vec(&Value::Object(payload))
        .expect("serializing a serde_json::Value is infallible");

    Ok(HttpReq {
        method: "POST".to_owned(),
        url,
        headers: base_headers(token),
        body: Some(body),
    })
}

// ─── Response parsers ─────────────────────────────────────────────────────────

/// Intermediate serde shape for a single result item in a Notion list response.
#[derive(Deserialize)]
struct RawPageHit {
    id: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    properties: Value,
}

/// Intermediate serde shape for the full Notion list/query response.
#[derive(Deserialize)]
struct RawQueryResponse {
    #[serde(default)]
    results: Vec<RawPageHit>,
    #[serde(default)]
    has_more: bool,
    next_cursor: Option<String>,
}

/// Parse a JSON string from the Notion `POST /databases/{id}/query` endpoint.
pub fn parse_query_response(json: &str) -> Result<QueryResult, String> {
    let raw: RawQueryResponse =
        serde_json::from_str(json).map_err(|e| format!("parse Notion query response: {e}"))?;
    Ok(QueryResult {
        results: raw
            .results
            .into_iter()
            .map(|h| PageHit {
                id: h.id,
                url: h.url,
                properties: h.properties,
            })
            .collect(),
        has_more: raw.has_more,
        next_cursor: raw.next_cursor,
    })
}

/// Intermediate serde shape for the Notion create-page response.
#[derive(Deserialize)]
struct RawPageResult {
    id: String,
    #[serde(default)]
    url: String,
}

/// Parse a JSON string from the Notion `POST /pages` endpoint.
pub fn parse_create_page_response(json: &str) -> Result<PageResult, String> {
    let raw: RawPageResult = serde_json::from_str(json)
        .map_err(|e| format!("parse Notion create-page response: {e}"))?;
    Ok(PageResult {
        id: raw.id,
        url: raw.url,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── validate_id ───────────────────────────────────────────────────────────

    #[test]
    fn validate_id_rejects_empty() {
        assert!(validate_id("").is_err());
    }

    #[test]
    fn validate_id_rejects_slash() {
        assert!(validate_id("bad/id").is_err());
    }

    #[test]
    fn validate_id_rejects_whitespace() {
        assert!(validate_id("bad id").is_err());
        assert!(validate_id("bad\tid").is_err());
        assert!(validate_id("bad\nid").is_err());
    }

    #[test]
    fn validate_id_rejects_control_char() {
        assert!(validate_id("bad\x00id").is_err());
    }

    #[test]
    fn validate_id_accepts_valid_notion_id() {
        assert!(validate_id("abc123-def456").is_ok());
        assert!(validate_id("8e7a3c2b1d4f5e6a").is_ok());
    }

    // ── build_query_database_request ─────────────────────────────────────────

    #[test]
    fn query_request_minimal() {
        let req = build_query_database_request("tok", "abc123", None, None, 100, None).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.url, "https://api.notion.com/v1/databases/abc123/query");
        assert!(
            req.headers
                .contains(&("authorization".to_owned(), "Bearer tok".to_owned()))
        );
        assert!(
            req.headers
                .contains(&("notion-version".to_owned(), "2022-06-28".to_owned()))
        );
        assert!(
            req.headers
                .contains(&("content-type".to_owned(), "application/json".to_owned()))
        );

        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["page_size"], 100);
        assert!(body.get("filter").is_none());
        assert!(body.get("sorts").is_none());
        assert!(body.get("start_cursor").is_none());
    }

    #[test]
    fn query_request_with_filter_sorts_cursor_and_oversized_page_size() {
        let filter = json!({"property": "Name", "title": {"is_not_empty": true}});
        let sorts = json!([{"timestamp": "created_time", "direction": "ascending"}]);
        let req = build_query_database_request(
            "tok",
            "db",
            Some(&filter),
            Some(&sorts),
            250,
            Some("CUR"),
        )
        .unwrap();

        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["page_size"], 100, "page_size must be capped at 100");
        assert_eq!(body["filter"], filter);
        assert_eq!(body["sorts"], sorts);
        assert_eq!(body["start_cursor"], "CUR");
    }

    #[test]
    fn query_request_rejects_slash_in_id() {
        assert!(build_query_database_request("tok", "bad/id", None, None, 100, None).is_err());
    }

    #[test]
    fn query_request_rejects_empty_id() {
        assert!(build_query_database_request("tok", "", None, None, 100, None).is_err());
    }

    #[test]
    fn query_headers_order() {
        let req = build_query_database_request("tok", "abc123", None, None, 10, None).unwrap();
        assert_eq!(req.headers[0].0, "authorization");
        assert_eq!(req.headers[1].0, "notion-version");
        assert_eq!(req.headers[2].0, "content-type");
    }

    // ── build_create_page_request ─────────────────────────────────────────────

    #[test]
    fn create_request_without_children() {
        let mut props = serde_json::Map::new();
        props.insert(
            "Name".to_owned(),
            json!({"title": [{"text": {"content": "Hi"}}]}),
        );
        let req = build_create_page_request("tok", "db", &props, None).unwrap();

        assert_eq!(req.method, "POST");
        assert_eq!(req.url, "https://api.notion.com/v1/pages");
        assert!(
            req.headers
                .contains(&("authorization".to_owned(), "Bearer tok".to_owned()))
        );
        assert!(
            req.headers
                .contains(&("notion-version".to_owned(), "2022-06-28".to_owned()))
        );

        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["parent"]["database_id"], "db");
        assert!(body["properties"]["Name"].is_object());
        assert!(body.get("children").is_none());
    }

    #[test]
    fn create_request_with_children() {
        let props = serde_json::Map::new();
        let children = json!([{"object": "block", "type": "paragraph"}]);
        let req = build_create_page_request("tok", "db", &props, Some(&children)).unwrap();

        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["children"], children);
    }

    #[test]
    fn create_request_rejects_slash_in_parent_id() {
        let props = serde_json::Map::new();
        assert!(build_create_page_request("tok", "bad/id", &props, None).is_err());
    }

    // ── parse_query_response ──────────────────────────────────────────────────

    #[test]
    fn parse_query_full_response() {
        let json = r#"{
            "object": "list",
            "results": [{"object": "page", "id": "p1", "url": "https://notion.so/p1", "properties": {"Name": {"id": "t"}}}],
            "has_more": true,
            "next_cursor": "NX"
        }"#;
        let result = parse_query_response(json).unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].id, "p1");
        assert_eq!(result.results[0].url, "https://notion.so/p1");
        assert!(result.has_more);
        assert_eq!(result.next_cursor, Some("NX".to_owned()));
    }

    #[test]
    fn parse_query_null_cursor_and_no_more() {
        let json = r#"{"results": [], "has_more": false, "next_cursor": null}"#;
        let result = parse_query_response(json).unwrap();
        assert!(!result.has_more);
        assert!(result.next_cursor.is_none());
    }

    #[test]
    fn parse_query_result_missing_url_defaults_to_empty() {
        let json = r#"{"results": [{"id": "p2"}], "has_more": false, "next_cursor": null}"#;
        let result = parse_query_response(json).unwrap();
        assert_eq!(result.results[0].url, "");
    }

    #[test]
    fn parse_query_malformed_json_returns_err() {
        assert!(parse_query_response("not json").is_err());
    }

    // ── parse_create_page_response ────────────────────────────────────────────

    #[test]
    fn parse_create_page_full_response() {
        let json = r#"{"object": "page", "id": "pg1", "url": "https://notion.so/pg1"}"#;
        let result = parse_create_page_response(json).unwrap();
        assert_eq!(result.id, "pg1");
        assert_eq!(result.url, "https://notion.so/pg1");
    }

    #[test]
    fn parse_create_page_malformed_json_returns_err() {
        assert!(parse_create_page_response("{bad}").is_err());
    }

    #[test]
    fn parse_create_page_missing_url_defaults_to_empty() {
        let json = r#"{"id": "pg2"}"#;
        let result = parse_create_page_response(json).unwrap();
        assert_eq!(result.id, "pg2");
        assert_eq!(result.url, "");
    }
}
