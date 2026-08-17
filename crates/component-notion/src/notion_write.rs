//! Pure request-building and response-parsing layer for Notion write operations.
//!
//! Covers `notion_update_page` (`PATCH /pages/{page_id}`) and
//! `notion_append_block` (`PATCH /blocks/{block_id}/children`).  Both use
//! `PATCH`; neither introduces new dependencies.  This module is free of any
//! WIT/bindings imports and can be tested on the host with plain `cargo test`.
//! Every request produced here carries the mandatory `Notion-Version` header
//! because both builders delegate to [`crate::notion::base_headers`].

// Copied verbatim from the greentic.notion design extension. The only edit is
// this attribute: several structs are consumed through `Deserialize` to VALIDATE
// a response rather than to read fields, and `HttpReq`'s fields are read only on
// the wasm target, so an off-wasm build sees them as dead. Silencing it here
// keeps the rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::Value;

use crate::notion::{HttpReq, NOTION_BASE, base_headers, validate_id};

// ─── Request builders ─────────────────────────────────────────────────────────

/// Build a `PATCH /pages/{page_id}` request that updates a page's properties.
///
/// - `properties` is the Notion typed-property map (passed through verbatim,
///   e.g. `{"Status": {"select": {"name": "Done"}}}`).
/// - `archived` is included in the body only when `Some`; `None` leaves the
///   archived state unchanged.
///
/// Returns `Err` when `page_id` fails [`validate_id`].
pub fn build_update_page_request(
    token: &str,
    page_id: &str,
    properties: &serde_json::Map<String, Value>,
    archived: Option<bool>,
) -> Result<HttpReq, String> {
    validate_id(page_id)?;

    let url = format!("{NOTION_BASE}/pages/{page_id}");
    let mut payload = serde_json::Map::new();
    payload.insert("properties".to_owned(), Value::Object(properties.clone()));
    if let Some(b) = archived {
        payload.insert("archived".to_owned(), serde_json::json!(b));
    }

    let body = serde_json::to_vec(&Value::Object(payload))
        .expect("serializing a serde_json::Value is infallible");

    Ok(HttpReq {
        method: "PATCH".to_owned(),
        url,
        headers: base_headers(token),
        body: Some(body),
    })
}

/// Build a `PATCH /blocks/{block_id}/children` request that appends block
/// children to a page or block.
///
/// - `children` is a Notion block-object array, passed through verbatim.
/// - A page ID is a valid block ID in Notion; no distinction is made here.
///
/// Returns `Err` when `block_id` fails [`validate_id`].
pub fn build_append_block_request(
    token: &str,
    block_id: &str,
    children: &Value,
) -> Result<HttpReq, String> {
    validate_id(block_id)?;

    let url = format!("{NOTION_BASE}/blocks/{block_id}/children");
    let payload = serde_json::json!({ "children": children.clone() });

    let body = serde_json::to_vec(&payload).expect("serializing a serde_json::Value is infallible");

    Ok(HttpReq {
        method: "PATCH".to_owned(),
        url,
        headers: base_headers(token),
        body: Some(body),
    })
}

// ─── Response types ───────────────────────────────────────────────────────────

/// Parsed `notion_append_block` response.
#[derive(Debug, PartialEq)]
pub struct AppendResult {
    /// IDs of the block objects that were appended, in order.
    pub block_ids: Vec<String>,
}

// ─── Response parsers ─────────────────────────────────────────────────────────

/// Intermediate serde shape for one block in the Notion list response.
#[derive(Deserialize)]
struct RawBlock {
    id: String,
}

/// Intermediate serde shape for the full Notion append-blocks response.
#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    results: Vec<RawBlock>,
}

/// Parse a JSON string from the Notion `PATCH /blocks/{id}/children` endpoint.
pub fn parse_append_response(json: &str) -> Result<AppendResult, String> {
    let raw: Raw =
        serde_json::from_str(json).map_err(|e| format!("parse Notion append response: {e}"))?;
    Ok(AppendResult {
        block_ids: raw.results.into_iter().map(|b| b.id).collect(),
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Helper: parse the body bytes back to a JSON Value.
    fn body_json(req: &HttpReq) -> Value {
        serde_json::from_slice(req.body.as_deref().unwrap()).unwrap()
    }

    // Helper: check that a header (name, value) pair is present.
    fn has_header(req: &HttpReq, name: &str, value: &str) -> bool {
        req.headers.iter().any(|(n, v)| n == name && v == value)
    }

    // ── build_update_page_request ─────────────────────────────────────────────

    #[test]
    fn update_request_shape_no_archived() {
        let mut props = serde_json::Map::new();
        props.insert("Status".to_owned(), json!({"select": {"name": "Done"}}));
        let req = build_update_page_request("tok", "pg1", &props, None).unwrap();

        assert_eq!(req.method, "PATCH");
        assert_eq!(req.url, "https://api.notion.com/v1/pages/pg1");
        assert!(has_header(&req, "authorization", "Bearer tok"));
        assert!(has_header(&req, "notion-version", "2022-06-28"));
        assert!(has_header(&req, "content-type", "application/json"));

        let body = body_json(&req);
        assert!(body["properties"]["Status"].is_object());
        assert!(
            body.get("archived").is_none(),
            "archived must be absent when None"
        );
    }

    #[test]
    fn update_request_with_archived_true() {
        let mut props = serde_json::Map::new();
        props.insert("X".to_owned(), json!(1));
        let req = build_update_page_request("tok", "pg1", &props, Some(true)).unwrap();

        let body = body_json(&req);
        assert_eq!(body["archived"], true);
    }

    #[test]
    fn update_request_with_archived_false() {
        let mut props = serde_json::Map::new();
        props.insert("X".to_owned(), json!(1));
        let req = build_update_page_request("tok", "pg1", &props, Some(false)).unwrap();

        let body = body_json(&req);
        assert_eq!(body["archived"], false);
    }

    #[test]
    fn update_request_rejects_invalid_id() {
        let mut props = serde_json::Map::new();
        props.insert("X".to_owned(), json!(1));
        assert!(build_update_page_request("tok", "bad/id", &props, None).is_err());
    }

    #[test]
    fn update_request_rejects_empty_id() {
        let props = serde_json::Map::new();
        assert!(build_update_page_request("tok", "", &props, None).is_err());
    }

    // ── build_append_block_request ────────────────────────────────────────────

    #[test]
    fn append_request_shape() {
        let children = json!([{
            "type": "paragraph",
            "paragraph": {"rich_text": []}
        }]);
        let req = build_append_block_request("tok", "blk1", &children).unwrap();

        assert_eq!(req.method, "PATCH");
        assert_eq!(req.url, "https://api.notion.com/v1/blocks/blk1/children");
        assert!(has_header(&req, "authorization", "Bearer tok"));
        assert!(has_header(&req, "notion-version", "2022-06-28"));

        let body = body_json(&req);
        assert_eq!(body["children"], children);
    }

    #[test]
    fn append_request_rejects_invalid_id() {
        assert!(build_append_block_request("tok", "a/b", &json!([])).is_err());
    }

    #[test]
    fn append_request_rejects_empty_id() {
        assert!(build_append_block_request("tok", "", &json!([])).is_err());
    }

    // ── parse_append_response ────────────────────────────────────────────────

    #[test]
    fn parse_append_two_ids() {
        let json = r#"{"object":"list","results":[{"object":"block","id":"b1"},{"id":"b2"}]}"#;
        let result = parse_append_response(json).unwrap();
        assert_eq!(result.block_ids, vec!["b1", "b2"]);
    }

    #[test]
    fn parse_append_empty_results() {
        let json = r#"{"results":[]}"#;
        let result = parse_append_response(json).unwrap();
        assert_eq!(result.block_ids, Vec::<String>::new());
    }

    #[test]
    fn parse_append_missing_results_defaults_to_empty() {
        let json = r#"{"object":"list"}"#;
        let result = parse_append_response(json).unwrap();
        assert_eq!(result.block_ids, Vec::<String>::new());
    }

    #[test]
    fn parse_append_malformed_json_returns_err() {
        assert!(parse_append_response("not json").is_err());
    }
}
