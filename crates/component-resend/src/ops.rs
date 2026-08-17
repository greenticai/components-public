//! The single Resend operation. `build_send_email_body` and
//! `map_send_email_response` are the design extension's WIT-free modules,
//! verbatim; only marshalling, the URL and the `{ok, …}` envelope are new.

use serde_json::Value;

use crate::input::SendEmailInput;
use crate::transport::{HttpReq, check, resolve_secret, send};
use crate::{input, output};

const BASE: &str = "https://api.resend.com";

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

pub fn send_email(node_input: &Value) -> Value {
    let raw_key = match node_input.get("api_key").and_then(Value::as_str) {
        Some(k) => k,
        None => return err("missing required field `api_key` (a value, or `secret:NAME`)"),
    };
    let key = match resolve_secret(raw_key) {
        Ok(k) => k,
        Err(e) => return err(e),
    };

    let parsed: SendEmailInput = match serde_json::from_value(node_input.clone()) {
        Ok(p) => p,
        Err(e) => return err(format!("invalid input for send_email: {e}")),
    };
    let body = match input::build_send_email_body(&parsed) {
        Ok(b) => b,
        Err(e) => return err(e),
    };
    let bytes = match serde_json::to_vec(&body) {
        Ok(b) => b,
        Err(e) => return err(format!("encode body: {e}")),
    };

    let resp = match send(HttpReq {
        method: "POST".into(),
        url: format!("{BASE}/emails"),
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("authorization".into(), format!("Bearer {key}")),
        ],
        body: Some(bytes),
    }) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let raw = match check(resp) {
        Ok(b) => b,
        Err(e) => return err(e),
    };
    match output::map_send_email_response(&raw) {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}
