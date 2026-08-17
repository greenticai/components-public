//! One handler per operation: parse the node input, build the request, send it,
//! parse the response.
//!
//! The building and parsing are the design extension's WIT-free modules,
//! verbatim. Only the marshalling and the `{ok, …}` envelope are new — so a
//! Notion call means the same thing whether an agentic worker makes it as a
//! TOOL or a flow runs it as a NODE.
//!
//! Every failure is a VALUE. A flow routes on `ok == false`; a trap would take
//! the run down with a message no operator can act on. That includes a missing
//! token, a malformed id, a non-2xx from Notion, and an unparseable response.

use serde_json::{Map, Value};

use crate::transport::{check, resolve_secret, send};
use crate::{notion, notion_read, notion_users, notion_write};

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(response) => return response,
        }
    };
}

/// Notion's default page size. Kept here rather than defaulted per call site so
/// eight operations cannot disagree about it.
const DEFAULT_PAGE_SIZE: u32 = 100;

fn token(input: &Value) -> Result<String, Value> {
    let raw = input
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| err("missing required field `token` (a value, or `secret:NAME`)"))?;
    resolve_secret(raw).map_err(err)
}

fn req_str<'a>(input: &'a Value, name: &str) -> Result<&'a str, Value> {
    input
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| err(format!("missing required field `{name}`")))
}

fn opt_str<'a>(input: &'a Value, name: &str) -> Option<&'a str> {
    input.get(name).and_then(Value::as_str)
}

fn req_object<'a>(input: &'a Value, name: &str) -> Result<&'a Map<String, Value>, Value> {
    match input.get(name) {
        Some(Value::Object(map)) => Ok(map),
        Some(_) => Err(err(format!("`{name}` must be a JSON object"))),
        None => Err(err(format!("missing required field `{name}`"))),
    }
}

fn page_size(input: &Value) -> u32 {
    input
        .get("page_size")
        .and_then(Value::as_u64)
        .map(|n| n.min(u64::from(u32::MAX)) as u32)
        .unwrap_or(DEFAULT_PAGE_SIZE)
}

/// Send a built request and hand back the raw body. Shared so every operation
/// classifies transport and non-2xx failures identically.
fn round_trip(req: notion::HttpReq) -> Result<Vec<u8>, Value> {
    let resp = send(req).map_err(err)?;
    check(resp).map_err(err)
}

fn as_json(body: &[u8]) -> Result<Value, Value> {
    serde_json::from_slice(body).map_err(|e| err(format!("Notion returned unparseable JSON: {e}")))
}

pub fn query_database(input: &Value) -> Value {
    let token = tri!(token(input));
    let db = tri!(req_str(input, "database_id"));
    let req = match notion::build_query_database_request(
        &token,
        db,
        input.get("filter"),
        input.get("sorts"),
        page_size(input),
        opt_str(input, "start_cursor"),
    ) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let body = tri!(round_trip(req));
    // Parsed to VALIDATE the shape, then the raw document is returned. The
    // typed structs are `Deserialize`-only in the extension, and deriving
    // `Serialize` onto them here would break the verbatim copy for no gain — a
    // flow reads fields by name off the document either way.
    if let Err(e) = notion::parse_query_response(&String::from_utf8_lossy(&body)) {
        return err(e);
    }
    ok(tri!(as_json(&body)))
}

pub fn create_page(input: &Value) -> Value {
    let token = tri!(token(input));
    let parent = tri!(req_str(input, "parent_database_id"));
    let props = tri!(req_object(input, "properties"));
    let req = match notion::build_create_page_request(&token, parent, props, input.get("children"))
    {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let body = tri!(round_trip(req));
    if let Err(e) = notion::parse_create_page_response(&String::from_utf8_lossy(&body)) {
        return err(e);
    }
    ok(tri!(as_json(&body)))
}

pub fn search(input: &Value) -> Value {
    let token = tri!(token(input));
    // Infallible: `search` has no id to validate.
    let req = notion_read::build_search_request(
        &token,
        opt_str(input, "query"),
        input.get("filter"),
        input.get("sort"),
        page_size(input),
        opt_str(input, "start_cursor"),
    );
    let body = tri!(round_trip(req));
    ok(tri!(as_json(&body)))
}

pub fn retrieve_block_children(input: &Value) -> Value {
    let token = tri!(token(input));
    let block = tri!(req_str(input, "block_id"));
    let req = match notion_read::build_retrieve_children_request(
        &token,
        block,
        page_size(input),
        opt_str(input, "start_cursor"),
    ) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let body = tri!(round_trip(req));
    ok(tri!(as_json(&body)))
}

pub fn list_users(input: &Value) -> Value {
    let token = tri!(token(input));
    // Infallible: `list_users` takes no id.
    let req = notion_users::build_list_users_request(
        &token,
        page_size(input),
        opt_str(input, "start_cursor"),
    );
    let body = tri!(round_trip(req));
    ok(tri!(as_json(&body)))
}

pub fn create_comment(input: &Value) -> Value {
    let token = tri!(token(input));
    let rich_text = match input.get("rich_text") {
        Some(v) => v,
        None => return err("missing required field `rich_text`"),
    };
    let req = match notion_users::build_create_comment_request(
        &token,
        opt_str(input, "page_id"),
        opt_str(input, "discussion_id"),
        rich_text,
    ) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let body = tri!(round_trip(req));
    ok(tri!(as_json(&body)))
}

pub fn update_page(input: &Value) -> Value {
    let token = tri!(token(input));
    let page = tri!(req_str(input, "page_id"));
    let props = tri!(req_object(input, "properties"));
    let req = match notion_write::build_update_page_request(
        &token,
        page,
        props,
        input.get("archived").and_then(Value::as_bool),
    ) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let body = tri!(round_trip(req));
    ok(tri!(as_json(&body)))
}

pub fn append_block(input: &Value) -> Value {
    let token = tri!(token(input));
    let block = tri!(req_str(input, "block_id"));
    let children = match input.get("children") {
        Some(v) => v,
        None => return err("missing required field `children`"),
    };
    let req = match notion_write::build_append_block_request(&token, block, children) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let body = tri!(round_trip(req));
    ok(tri!(as_json(&body)))
}
