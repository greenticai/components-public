//! Pure request-building and response-parsing layer for Notion users and comments.
//!
//! Covers `notion_list_users` (`GET /v1/users`) and
//! `notion_create_comment` (`POST /v1/comments`).
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
use serde_json::json;

use crate::notion::{HttpReq, NOTION_BASE, base_headers, validate_id};
use crate::notion_read::percent_encode;

// ─── Request builders ─────────────────────────────────────────────────────────

/// Build a `GET /users` request to list workspace members.
///
/// - `page_size` is capped at 100 (Notion's maximum).
/// - `start_cursor`, when present, is percent-encoded before appending.
#[must_use]
pub fn build_list_users_request(
    token: &str,
    page_size: u32,
    start_cursor: Option<&str>,
) -> HttpReq {
    let path = format!("{NOTION_BASE}/users");
    let mut query = format!("page_size={}", page_size.min(100));
    if let Some(cursor) = start_cursor {
        write!(query, "&start_cursor={}", percent_encode(cursor))
            .expect("write to String is infallible");
    }
    let url = format!("{path}?{query}");

    HttpReq {
        method: "GET".to_owned(),
        url,
        headers: base_headers(token),
        body: None,
    }
}

/// Build a `POST /comments` request to create a comment on a page or discussion.
///
/// Exactly one of `page_id` or `discussion_id` must be `Some`; supplying both
/// or neither returns an `Err`.  The provided id is validated by
/// [`validate_id`] (path-injection guard).
pub fn build_create_comment_request(
    token: &str,
    page_id: Option<&str>,
    discussion_id: Option<&str>,
    rich_text: &serde_json::Value,
) -> Result<HttpReq, String> {
    let body = match (page_id, discussion_id) {
        (Some(pid), None) => {
            validate_id(pid)?;
            json!({
                "parent": { "page_id": pid },
                "rich_text": rich_text
            })
        }
        (None, Some(did)) => {
            validate_id(did)?;
            json!({
                "discussion_id": did,
                "rich_text": rich_text
            })
        }
        _ => {
            return Err(
                "create_comment requires exactly one of page_id or discussion_id".to_owned(),
            );
        }
    };

    let url = format!("{NOTION_BASE}/comments");
    let body_bytes =
        serde_json::to_vec(&body).expect("serializing a serde_json::Value is infallible");

    Ok(HttpReq {
        method: "POST".to_owned(),
        url,
        headers: base_headers(token),
        body: Some(body_bytes),
    })
}

// ─── Response types ───────────────────────────────────────────────────────────

/// One user returned by `notion_list_users`.
#[derive(Debug, PartialEq)]
pub struct UserHit {
    pub id: String,
    pub name: String,
    /// User object kind, typically `"person"` or `"bot"`.
    pub kind: String,
}

/// Parsed `notion_list_users` response.
#[derive(Debug, PartialEq)]
pub struct UsersResult {
    pub users: Vec<UserHit>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// Parsed `notion_create_comment` response.
#[derive(Debug, PartialEq)]
pub struct CommentResult {
    pub id: String,
}

// ─── Response parsers ─────────────────────────────────────────────────────────

/// Intermediate serde shape for a single user in the list-users response.
#[derive(Deserialize)]
struct RawUser {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    kind: String,
}

/// Intermediate serde shape for the full list-users response.
#[derive(Deserialize)]
struct RawUsersResponse {
    #[serde(default)]
    results: Vec<RawUser>,
    #[serde(default)]
    has_more: bool,
    next_cursor: Option<String>,
}

/// Parse a JSON string from the Notion `GET /users` endpoint.
pub fn parse_users_response(json: &str) -> Result<UsersResult, String> {
    let raw: RawUsersResponse =
        serde_json::from_str(json).map_err(|e| format!("parse Notion list-users response: {e}"))?;
    Ok(UsersResult {
        users: raw
            .results
            .into_iter()
            .map(|u| UserHit {
                id: u.id,
                name: u.name,
                kind: u.kind,
            })
            .collect(),
        has_more: raw.has_more,
        next_cursor: raw.next_cursor,
    })
}

/// Intermediate serde shape for the create-comment response.
#[derive(Deserialize)]
struct RawCommentResult {
    id: String,
}

/// Parse a JSON string from the Notion `POST /comments` endpoint.
pub fn parse_comment_response(json: &str) -> Result<CommentResult, String> {
    let raw: RawCommentResult = serde_json::from_str(json)
        .map_err(|e| format!("parse Notion create-comment response: {e}"))?;
    Ok(CommentResult { id: raw.id })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── build_list_users_request ──────────────────────────────────────────────

    #[test]
    fn list_users_request_shape_with_cursor() {
        let req = build_list_users_request("tok", 50, Some("CUR/1"));
        assert_eq!(req.method, "GET");
        assert_eq!(
            req.url,
            "https://api.notion.com/v1/users?page_size=50&start_cursor=CUR%2F1"
        );
        assert!(
            req.headers
                .contains(&("authorization".to_owned(), "Bearer tok".to_owned()))
        );
        assert!(
            req.headers
                .contains(&("notion-version".to_owned(), "2022-06-28".to_owned()))
        );
        assert!(req.body.is_none());
    }

    #[test]
    fn list_users_request_no_cursor() {
        let req = build_list_users_request("tok", 100, None);
        assert_eq!(req.url, "https://api.notion.com/v1/users?page_size=100");
    }

    #[test]
    fn list_users_request_caps_page_size() {
        let req = build_list_users_request("tok", 999, None);
        assert!(req.url.contains("page_size=100"));
    }

    // ── build_create_comment_request ──────────────────────────────────────────

    #[test]
    fn create_comment_with_page_id_produces_parent_body() {
        let rich_text = json!([{"type": "text", "text": {"content": "Hello"}}]);
        let req = build_create_comment_request("tok", Some("page1"), None, &rich_text).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.url, "https://api.notion.com/v1/comments");
        assert!(
            req.headers
                .contains(&("authorization".to_owned(), "Bearer tok".to_owned()))
        );

        let body: serde_json::Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["parent"]["page_id"], "page1");
        assert!(body.get("discussion_id").is_none());
        assert_eq!(body["rich_text"], rich_text);
    }

    #[test]
    fn create_comment_with_discussion_id() {
        let rich_text = json!([{"type": "text", "text": {"content": "Hi"}}]);
        let req = build_create_comment_request("tok", None, Some("disc1"), &rich_text).unwrap();

        let body: serde_json::Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["discussion_id"], "disc1");
        assert!(body.get("parent").is_none());
        assert_eq!(body["rich_text"], rich_text);
    }

    #[test]
    fn create_comment_both_ids_returns_err() {
        let rich_text = json!([]);
        let result = build_create_comment_request("tok", Some("pid"), Some("did"), &rich_text);
        match result {
            Err(e) => assert!(
                e.contains("exactly one of page_id or discussion_id"),
                "unexpected error: {e}"
            ),
            Ok(_) => panic!("expected Err, got Ok"),
        }
    }

    #[test]
    fn create_comment_neither_id_returns_err() {
        let rich_text = json!([]);
        let result = build_create_comment_request("tok", None, None, &rich_text);
        match result {
            Err(e) => assert!(
                e.contains("exactly one of page_id or discussion_id"),
                "unexpected error: {e}"
            ),
            Ok(_) => panic!("expected Err, got Ok"),
        }
    }

    #[test]
    fn create_comment_invalid_page_id_returns_err() {
        let rich_text = json!([]);
        let result = build_create_comment_request("tok", Some("a/b"), None, &rich_text);
        // validate_id Err, not the mutual-exclusion Err
        match result {
            Err(e) => assert!(e.contains("invalid Notion id"), "unexpected error: {e}"),
            Ok(_) => panic!("expected Err, got Ok"),
        }
    }

    // ── parse_users_response ──────────────────────────────────────────────────

    #[test]
    fn parse_users_full_response() {
        let json_str = r#"{
            "results": [
                {"id": "u1", "name": "Alice", "type": "person"},
                {"id": "u2"}
            ],
            "has_more": true,
            "next_cursor": "NX"
        }"#;
        let result = parse_users_response(json_str).unwrap();
        assert_eq!(result.users.len(), 2);
        assert_eq!(result.users[0].id, "u1");
        assert_eq!(result.users[0].name, "Alice");
        assert_eq!(result.users[0].kind, "person");
        // Missing name and type default to empty string
        assert_eq!(result.users[1].id, "u2");
        assert_eq!(result.users[1].name, "");
        assert_eq!(result.users[1].kind, "");
        assert!(result.has_more);
        assert_eq!(result.next_cursor, Some("NX".to_owned()));
    }

    #[test]
    fn parse_users_empty_response() {
        let json_str = r#"{"results":[],"has_more":false}"#;
        let result = parse_users_response(json_str).unwrap();
        assert!(result.users.is_empty());
        assert!(!result.has_more);
        assert!(result.next_cursor.is_none());
    }

    #[test]
    fn parse_users_malformed_json_returns_err() {
        assert!(parse_users_response("not json").is_err());
    }

    // ── parse_comment_response ────────────────────────────────────────────────

    #[test]
    fn parse_comment_response_returns_id() {
        let result = parse_comment_response(r#"{"id":"cmt1","object":"comment"}"#).unwrap();
        assert_eq!(result.id, "cmt1");
    }

    #[test]
    fn parse_comment_malformed_json_returns_err() {
        assert!(parse_comment_response("{bad}").is_err());
    }
}
