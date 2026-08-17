//! One handler per Google Calendar tool.
//!
//! gcal is the only integration in this set with NO static-token path — its own
//! description says Google Calendar has no static API key for user calendars.
//! So unlike calendly / jira / clickup, a component here cannot simply read one
//! secret and send it: it has to perform the refresh-token grant itself.
//!
//! That is possible without the OAuth broker because both halves of the
//! exchange are pure functions in the extension's `auth` module —
//! `build_refresh_form` and `extract_refreshed_token` — and the token host is
//! already on the network allowlist. Only the POST between them is new.

use serde_json::Value;

use crate::auth;
use crate::client::{self, HttpCall};
use crate::transport::{HttpReq, check, resolve_secret, send};

const BASE: &str = "https://www.googleapis.com/calendar/v3";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

fn field(input: &Value, name: &str) -> Result<String, Value> {
    let raw = input.get(name).and_then(Value::as_str).ok_or_else(|| {
        err(format!(
            "missing required field `{name}` (a value, or `secret:NAME`)"
        ))
    })?;
    resolve_secret(raw).map_err(err)
}

/// Exchange the stored refresh token for an access token.
///
/// This runs on EVERY call rather than caching: a component instance has no
/// lifetime an access token could safely be scoped to, and Google's tokens are
/// short-lived. One extra round trip is the honest cost of not having a broker
/// to hold state.
///
/// None of the three secrets ever leaves this function — only the resulting
/// access token reaches a header, and the error text names the failure, not the
/// credential.
fn access_token(input: &Value) -> Result<String, Value> {
    let client_id = field(input, "oauth_client_id")?;
    let client_secret = field(input, "oauth_client_secret")?;
    let refresh_token = field(input, "oauth_refresh_token")?;

    let form = auth::build_refresh_form(&client_id, &client_secret, &refresh_token);
    let resp = send(HttpReq {
        method: "POST".into(),
        url: TOKEN_URL.into(),
        headers: vec![(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        )],
        body: Some(form.into_bytes()),
    })
    .map_err(err)?;
    let raw = check(resp).map_err(|e| err(format!("token refresh failed: {e}")))?;

    auth::extract_refreshed_token(&String::from_utf8_lossy(&raw))
        .map_err(|e| err(format!("token refresh returned no access token: {e:?}")))
}

fn dispatch_call(call: &HttpCall, token: &str) -> Result<Vec<u8>, Value> {
    let body = match &call.body {
        Some(value) => {
            Some(serde_json::to_vec(value).map_err(|e| err(format!("encode body: {e}")))?)
        }
        None => None,
    };
    let resp = send(HttpReq {
        method: call.method.as_str().to_string(),
        url: format!("{BASE}{}{}", call.path, client::encode_query(&call.query)),
        headers: vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("authorization".to_string(), format!("Bearer {token}")),
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

            let token = match access_token(node_input) {
                Ok(t) => t,
                Err(e) => return e,
            };
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
            let raw = match dispatch_call(&call, &token) {
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

tool!(gcal_events, events);
tool!(gcal_calendars, calendars);
tool!(gcal_freebusy, freebusy);
