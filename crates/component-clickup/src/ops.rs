//! One handler per clickup tool.
//!
//! Each `tools::*` module exposes the same triple — `build_call`,
//! `parse_operation`, `normalize` — all copied verbatim from the design
//! extension, so the handlers differ only in which module they call and come
//! from one macro.
//!
//! Every failure is a VALUE: a flow routes on `ok == false`, whereas a trap
//! takes the run down with a message no operator can act on.

use serde_json::Value;

use crate::auth;
use crate::client::{self, HttpCall};
use crate::transport::{HttpReq, check, resolve_secret, send};

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

/// ClickUp takes the token RAW — no `Bearer` prefix, which is why this goes
/// through the extension's own header builder rather than being assembled here.
///
/// The extension ALSO supports an `auth_mode` secret naming a long-lived OAuth
/// access token. That token is itself a bearer value with no refresh, so the
/// distinction collapses for a component: either way one secret is read and
/// sent. The broker path is not importable and is not offered.
fn auth_header(input: &Value) -> Result<String, Value> {
    let raw = input
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| err("missing required field `token` (a value, or `secret:NAME`)"))?;
    Ok(auth::token_auth_header(&resolve_secret(raw).map_err(err)?))
}

fn base_url(_input: &Value) -> Result<String, Value> {
    Ok(auth::BASE_URL.to_string())
}

fn dispatch_call(base: &str, call: &HttpCall, header: &str) -> Result<Vec<u8>, Value> {
    let body = match &call.body {
        Some(value) => {
            Some(serde_json::to_vec(value).map_err(|e| err(format!("encode body: {e}")))?)
        }
        None => None,
    };
    let resp = send(HttpReq {
        method: call.method.as_str().to_string(),
        url: format!("{base}{}{}", call.path, client::encode_query(&call.query)),
        headers: vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("authorization".to_string(), header.to_string()),
        ],
        body,
    })
    .map_err(err)?;
    check(resp).map_err(err)
}

macro_rules! tool {
    ($fn_name:ident, $module:ident) => {
        pub fn $fn_name(node_input: &Value) -> Value {
            use crate::tools::$module as m;

            let header = match auth_header(node_input) {
                Ok(h) => h,
                Err(e) => return e,
            };
            let base = match base_url(node_input) {
                Ok(b) => b,
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
            let raw = match dispatch_call(&base, &call, &header) {
                Ok(r) => r,
                Err(e) => return e,
            };
            match m::normalize(op, &raw) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }
    };
}

tool!(clickup_tasks, tasks);
tool!(clickup_spaces, spaces);
tool!(clickup_folders, folders);
tool!(clickup_lists, lists);
tool!(clickup_comments, comments);
tool!(clickup_time_entries, time_entries);
tool!(clickup_custom_fields, custom_fields);
tool!(clickup_members, members);
