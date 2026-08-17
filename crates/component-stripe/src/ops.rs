//! One handler per Stripe tool.
//!
//! Each `tools::*` module exposes the same triple — `build_call`,
//! `parse_operation`, `normalize` — and all of them are the design extension's
//! WIT-free modules, verbatim. The thirteen handlers therefore differ only in
//! which module they call, so they come from one macro rather than thirteen
//! near-identical functions.
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

/// Resolve the secret key per call rather than caching it: a component instance
/// may outlive a credential rotation, and a stale key fails as an opaque 401.
fn auth_header(input: &Value) -> Result<String, Value> {
    let raw = input
        .get("secret_key")
        .and_then(Value::as_str)
        .ok_or_else(|| err("missing required field `secret_key` (a value, or `secret:NAME`)"))?;
    let key = resolve_secret(raw).map_err(err)?;
    Ok(auth::bearer_header(&key))
}

/// Stripe takes FORM-encoded bodies, not JSON — `form_encode_value` is the
/// extension's own encoder, kept so nested `params` flatten identically.
fn dispatch_call(call: &HttpCall, header: &str) -> Result<Vec<u8>, Value> {
    let mut headers = vec![("authorization".to_string(), header.to_string())];
    let body = match &call.body {
        Some(value) => {
            headers.push((
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ));
            Some(client::form_encode_value(value).into_bytes())
        }
        None => None,
    };

    let resp = send(HttpReq {
        method: call.method.as_str().to_string(),
        url: format!(
            "{}{}{}",
            auth::BASE_URL,
            call.path,
            client::encode_query(&call.query)
        ),
        headers,
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
            let raw = match dispatch_call(&call, &header) {
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

tool!(stripe_customers, customers);
tool!(stripe_products, products);
tool!(stripe_prices, prices);
tool!(stripe_payment_links, payment_links);
tool!(stripe_invoices, invoices);
tool!(stripe_subscriptions, subscriptions);
tool!(stripe_checkout_sessions, checkout_sessions);
tool!(stripe_refunds, refunds);
tool!(stripe_payment_intents, payment_intents);
tool!(stripe_disputes, disputes);
tool!(stripe_coupons, coupons);
tool!(stripe_promotion_codes, promotion_codes);
/// Files is the one operation that does NOT follow the triple: it uploads
/// multipart to a DIFFERENT host (`files.stripe.com`, not `api.stripe.com`),
/// so it is written out rather than macro-generated.
///
/// The base64 decode happens here because the node input carries the bytes as a
/// string; a malformed value is reported rather than panicking, since a flow
/// can route on it.
pub fn stripe_files(node_input: &Value) -> Value {
    use crate::tools::files;

    let header = match auth_header(node_input) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let args = match serde_json::to_string(node_input) {
        Ok(s) => s,
        Err(e) => return err(format!("encode request: {e}")),
    };
    let input = match files::parse_input(&args) {
        Ok(i) => i,
        Err(e) => return err(e),
    };
    let bytes = match base64_decode(&input.file_base64) {
        Ok(b) => b,
        Err(e) => return err(format!("file_base64 decode: {e}")),
    };
    let purpose = input.purpose.as_deref().unwrap_or("dispute_evidence");
    let filename = input.filename.as_deref().unwrap_or("upload");

    let resp = match send(HttpReq {
        method: "POST".into(),
        url: "https://files.stripe.com/v1/files".into(),
        headers: vec![
            ("authorization".into(), header),
            ("content-type".into(), files::content_type_header()),
        ],
        body: Some(files::build_multipart_body(purpose, filename, &bytes)),
    }) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let raw = match check(resp) {
        Ok(b) => b,
        Err(e) => return err(e),
    };
    match files::normalize(&raw) {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

/// Standard base64 decode. Sixteen lines rather than a dependency, and it
/// REJECTS invalid input instead of silently skipping bytes — a truncated
/// upload that Stripe accepts as a valid-but-wrong file is worse than an error.
fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("invalid base64 character: {}", c as char)),
        }
    }
    let cleaned: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let body: Vec<u8> = cleaned.iter().copied().take_while(|&b| b != b'=').collect();
    if !cleaned.len().is_multiple_of(4) {
        return Err("length is not a multiple of 4".to_string());
    }
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for chunk in body.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= u32::from(val(c)?) << (18 - 6 * i);
        }
        let take = chunk.len() * 6 / 8;
        for i in 0..take {
            out.push(((acc >> (16 - 8 * i)) & 0xff) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned against known vectors, and against REJECTING bad input — a
    /// decoder that silently drops characters uploads a corrupt file that
    /// Stripe stores successfully.
    #[test]
    fn base64_decode_matches_known_vectors_and_rejects_junk() {
        assert_eq!(base64_decode("YQ==").unwrap(), b"a");
        assert_eq!(base64_decode("YWI=").unwrap(), b"ab");
        assert_eq!(base64_decode("YWJj").unwrap(), b"abc");
        assert!(
            base64_decode("YWJ").is_err(),
            "length must be a multiple of 4"
        );
        assert!(
            base64_decode("YW!j").is_err(),
            "invalid character must be rejected"
        );
    }
}
