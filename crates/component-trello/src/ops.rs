//! One handler per Trello tool.
//!
//! Each `tools::*` module exposes the same triple — `build_call`,
//! `parse_operation`, `normalize` — and all three are the design extension's
//! WIT-free modules, verbatim. The eight handlers below therefore differ only
//! in which module they call and which request field backfills a missing id, so
//! they are generated from one macro rather than written out eight times.
//!
//! Every failure is a VALUE: a flow routes on `ok == false`, whereas a trap
//! takes the run down with a message no operator can act on.

use serde_json::Value;

use crate::auth;
use crate::client::{HttpCall, Method, encode_query};
use crate::transport::{HttpReq, check, resolve_secret, send};

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

/// Trello authenticates with two values carried as QUERY PARAMETERS, not
/// headers — which is why they join `call.query` rather than the header list.
fn auth_pairs(input: &Value) -> Result<Vec<(String, String)>, Value> {
    let key_raw = input
        .get("api_key")
        .and_then(Value::as_str)
        .ok_or_else(|| err("missing required field `api_key` (a value, or `secret:NAME`)"))?;
    let token_raw = input
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| err("missing required field `token` (a value, or `secret:NAME`)"))?;
    let key = resolve_secret(key_raw).map_err(err)?;
    let token = resolve_secret(token_raw).map_err(err)?;
    Ok(auth::auth_query(&key, &token))
}

fn dispatch_call(call: &HttpCall, pairs: &[(String, String)]) -> Result<Vec<u8>, Value> {
    let body = match &call.body {
        Some(value) => {
            Some(serde_json::to_vec(value).map_err(|e| err(format!("encode body: {e}")))?)
        }
        None => None,
    };
    let mut query = call.query.clone();
    query.extend_from_slice(pairs);

    let url = format!("{}{}?{}", auth::BASE_URL, call.path, encode_query(&query));
    let method = match call.method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
    };

    let resp = send(HttpReq {
        method: method.to_string(),
        url,
        headers: vec![("content-type".into(), "application/json".into())],
        body,
    })
    .map_err(err)?;
    check(resp).map_err(err)
}

/// Trello answers some writes with an acknowledgement carrying a null `id`.
/// The extension backfills it from the request so a downstream node still has
/// something to reference; dropping that would make those operations look
/// successful and return nothing usable.
fn backfill_id(normalized: &mut Value, node_input: &Value, request_field: &str) {
    if !normalized.get("id").is_some_and(Value::is_null) {
        return;
    }
    if let Some(id) = node_input.get(request_field).and_then(Value::as_str) {
        normalized["id"] = Value::String(id.to_string());
    }
}

macro_rules! tool {
    ($fn_name:ident, $module:ident, $id_field:literal) => {
        pub fn $fn_name(node_input: &Value) -> Value {
            use crate::tools::$module as m;

            let pairs = match auth_pairs(node_input) {
                Ok(p) => p,
                Err(e) => return e,
            };
            // The pure modules parse the raw request JSON, so the node input is
            // handed back as a string rather than re-modelled here.
            let args = match serde_json::to_string(node_input) {
                Ok(s) => s,
                Err(e) => return err(format!("encode request: {e}")),
            };
            let call = match m::build_call(&args) {
                Ok(c) => c,
                Err(e) => return err(e),
            };
            let op = match m::parse_operation(&args) {
                Ok(o) => o,
                Err(e) => return err(e),
            };
            let raw = match dispatch_call(&call, &pairs) {
                Ok(r) => r,
                Err(e) => return e,
            };
            match m::normalize(op, &raw) {
                Ok(mut v) => {
                    backfill_id(&mut v, node_input, $id_field);
                    ok(v)
                }
                Err(e) => err(e),
            }
        }
    };
}

tool!(trello_cards, cards, "card_id");
tool!(trello_lists, lists, "list_id");
tool!(trello_boards, boards, "board_id");
tool!(trello_checklists, checklists, "checklist_id");
tool!(trello_labels, labels, "label_id");
tool!(trello_comments, comments, "comment_id");
tool!(trello_attachments, attachments, "attachment_id");
tool!(trello_members, members, "member_id");
