//! The single Twilio operation: send an SMS.
//!
//! `build_send_sms_body` and `map_send_sms_response` are the design extension's
//! WIT-free modules, verbatim. Only marshalling, the URL, the Basic-auth header
//! and the `{ok, …}` envelope are new.

use serde_json::Value;

use crate::transport::{HttpReq, check, resolve_secret, send};
use crate::{input, output};

const BASE: &str = "https://api.twilio.com";

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

/// Twilio authenticates with HTTP Basic over `account_sid:auth_token`, so the
/// component needs base64 without pulling a dependency for sixteen lines.
fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn field(input: &Value, name: &str) -> Result<String, Value> {
    let raw = input
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| err(format!("missing required field `{name}`")))?;
    resolve_secret(raw).map_err(err)
}

pub fn send_sms(input: &Value) -> Value {
    // Three credentials, each resolvable as `secret:NAME` or a literal.
    let account_sid = match field(input, "account_sid") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let auth_token = match field(input, "auth_token") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let from_number = match field(input, "from_number") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let to = match input.get("to").and_then(Value::as_str) {
        Some(v) => v,
        None => return err("missing required field `to`"),
    };
    let message = match input.get("body").and_then(Value::as_str) {
        Some(v) => v,
        None => return err("missing required field `body`"),
    };

    let form = match input::build_send_sms_body(to, message, &from_number) {
        Ok(f) => f,
        Err(e) => return err(e),
    };

    let credentials = base64(format!("{account_sid}:{auth_token}").as_bytes());
    let req = HttpReq {
        method: "POST".into(),
        url: format!("{BASE}/2010-04-01/Accounts/{account_sid}/Messages.json"),
        headers: vec![
            (
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            ),
            ("authorization".into(), format!("Basic {credentials}")),
        ],
        body: Some(form.into_bytes()),
    };

    let resp = match send(req) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let raw = match check(resp) {
        Ok(b) => b,
        Err(e) => return err(e),
    };
    match output::map_send_sms_response(&raw) {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned against known vectors — a wrong pad byte produces a header Twilio
    /// rejects as a 401, which reads like a bad credential rather than a bug.
    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(base64(b"sid:token"), "c2lkOnRva2Vu");
    }

    #[test]
    fn a_missing_credential_is_named_and_does_not_panic() {
        let out = send_sms(&serde_json::json!({"to": "+1", "body": "hi"}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("account_sid"));
    }
}
