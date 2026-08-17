//! One handler per operation. The body builders (`input`) and response mappers
//! (`output`) are the design extension's WIT-free modules, verbatim; only
//! marshalling, the URL and the `{ok, …}` envelope are new.

use serde_json::Value;

use crate::input::{ExtractInput, SearchInput};
use crate::transport::{HttpReq, check, resolve_secret, send};
use crate::{input, output};

const BASE: &str = "https://api.tavily.com";

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

/// Resolve the key per call rather than caching it: a component instance may
/// outlive a credential rotation, and a stale key fails as an opaque 401.
fn headers(input: &Value) -> Result<Vec<(String, String)>, Value> {
    let raw = input
        .get("api_key")
        .and_then(Value::as_str)
        .ok_or_else(|| err("missing required field `api_key` (a value, or `secret:NAME`)"))?;
    let key = resolve_secret(raw).map_err(err)?;
    Ok(vec![
        ("content-type".to_string(), "application/json".to_string()),
        ("authorization".to_string(), format!("Bearer {key}")),
    ])
}

fn post(path: &str, headers: Vec<(String, String)>, body: &Value) -> Result<Vec<u8>, Value> {
    let bytes = serde_json::to_vec(body).map_err(|e| err(format!("encode body: {e}")))?;
    let resp = send(HttpReq {
        method: "POST".into(),
        url: format!("{BASE}{path}"),
        headers,
        body: Some(bytes),
    })
    .map_err(err)?;
    check(resp).map_err(err)
}

macro_rules! op {
    ($fn_name:ident, $ty:ty, $what:literal, $build:path, $map:path, $path:literal) => {
        pub fn $fn_name(node_input: &Value) -> Value {
            let headers = match headers(node_input) {
                Ok(h) => h,
                Err(e) => return e,
            };
            let parsed: $ty = match serde_json::from_value(node_input.clone()) {
                Ok(p) => p,
                Err(e) => return err(format!("invalid input for {}: {e}", $what)),
            };
            let body = match $build(&parsed) {
                Ok(b) => b,
                Err(e) => return err(e),
            };
            let raw = match post($path, headers, &body) {
                Ok(r) => r,
                Err(e) => return e,
            };
            match $map(&raw) {
                Ok(v) => ok(serde_json::to_value(v).unwrap_or(Value::Null)),
                Err(e) => err(e),
            }
        }
    };
}

op!(
    tavily_search,
    SearchInput,
    "tavily_search",
    input::build_search_body,
    output::map_search_response,
    "/search"
);
op!(
    tavily_extract,
    ExtractInput,
    "tavily_extract",
    input::build_extract_body,
    output::map_extract_response,
    "/extract"
);
