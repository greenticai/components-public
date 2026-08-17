//! The only part of this component that touches the host.
//!
//! `guard` and `protocol` are the design extension's WIT-free modules copied
//! verbatim; this file is what replaces its `lib.rs`. The extension reaches the
//! network through `greentic:extension-host/http` and credentials through
//! `greentic:extension-host/secrets`, neither of which a flow component can
//! import. A component uses the guest crate's `http_client_v1_1` and
//! `secrets_store` instead.
//!
//! Off-wasm both are stubs, so the guard and the protocol shaping stay
//! host-testable with plain `cargo test` — which is where the real coverage is.

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::http_client_v1_1 as client;
#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::secrets_store;

/// A gateway `/schema` introspection and an LLM completion are both slower than
/// an ordinary API call, so this sits at the host's effective ceiling rather
/// than at the 30s most components use.
#[cfg(target_arch = "wasm32")]
const TIMEOUT_MS: u32 = 60_000;

/// Off-wasm `send` is a stub that issues nothing, so nothing reads these
/// fields in a host build — the same idiom `lib.rs` uses for `COMPONENT_ID`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub struct HttpReq {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Resolve `secret:NAME` through the secret store; anything else is a literal.
///
/// The literal branch matters: an operator may paste a token directly while
/// testing, and refusing that would make the node unusable before a secret is
/// provisioned. It is also why no token ever appears in an error message below.
#[cfg(target_arch = "wasm32")]
pub fn resolve_secret(token: &str) -> Result<String, String> {
    match token.strip_prefix("secret:") {
        Some(name) => match secrets_store::get(name) {
            Ok(Some(bytes)) => {
                String::from_utf8(bytes).map_err(|_| "secret is not valid utf-8".to_string())
            }
            Ok(None) => Err(format!("secret not found: {name}")),
            Err(_) => Err(format!("failed to read secret: {name}")),
        },
        None => Ok(token.to_string()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn resolve_secret(token: &str) -> Result<String, String> {
    match token.strip_prefix("secret:") {
        Some(name) => std::env::var(name).map_err(|_| format!("secret not found: {name}")),
        None => Ok(token.to_string()),
    }
}

pub fn get(url: &str, bearer: &str) -> HttpReq {
    HttpReq {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: vec![("Authorization".to_string(), format!("Bearer {bearer}"))],
        body: None,
    }
}

pub fn post_json(url: &str, bearer: &str, body: &serde_json::Value) -> HttpReq {
    HttpReq {
        method: "POST".to_string(),
        url: url.to_string(),
        headers: vec![
            ("Authorization".to_string(), format!("Bearer {bearer}")),
            ("Content-Type".to_string(), "application/json".to_string()),
        ],
        body: Some(body.to_string().into_bytes()),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn send(req: HttpReq) -> Result<Response, String> {
    let wasm_req = client::Request {
        method: req.method,
        url: req.url,
        headers: req.headers,
        body: req.body,
    };
    let options = client::RequestOptions {
        timeout_ms: Some(TIMEOUT_MS),
        allow_insecure: Some(false),
        follow_redirects: Some(true),
    };
    match client::send(&wasm_req, Some(options), None) {
        Ok(resp) => Ok(Response {
            status: resp.status,
            body: resp.body.unwrap_or_default(),
        }),
        Err(err) => Err(format!("http send failed: {}", err.code)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn send(_req: HttpReq) -> Result<Response, String> {
    // Host builds have no network. Tests cover the guard and the protocol
    // shaping on either side of this call, which is where the logic lives.
    Err("http is unavailable off-wasm".to_string())
}

/// Classify a response. A non-2xx is a routable failure, not a crash.
///
/// The snippet is built from the response BODY only, capped at 256 chars — no
/// URL, no headers, no request body — so an error surfaced to an operator can
/// carry neither the gateway token nor the LLM key, both of which travel in an
/// `Authorization` header on the request this reports on.
pub fn check(what: &str, resp: Response) -> Result<Vec<u8>, String> {
    if (200..300).contains(&resp.status) {
        return Ok(resp.body);
    }
    let raw = String::from_utf8_lossy(&resp.body);
    let snippet: String = raw.chars().take(256).collect();
    Err(format!("{what} failed ({}): {snippet}", resp.status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_2xx_yields_the_body() {
        let out = check(
            "schema",
            Response {
                status: 200,
                body: b"{\"ok\":1}".to_vec(),
            },
        );
        assert_eq!(out.unwrap(), b"{\"ok\":1}");
    }

    /// The error text must be able to carry the gateway's message without ever
    /// carrying the request — that is what makes it safe to show an operator.
    #[test]
    fn a_non_2xx_reports_the_status_and_a_bounded_body_snippet() {
        let err = check(
            "query",
            Response {
                status: 401,
                body: "x".repeat(500).into_bytes(),
            },
        )
        .unwrap_err();
        assert!(err.starts_with("query failed (401):"));
        assert!(
            err.len() < 320,
            "snippet must stay bounded, got {}",
            err.len()
        );
    }

    /// The bearer is interpolated into a header, never into the URL or the
    /// error — pinned because `check` is the one place a response can surface.
    #[test]
    fn a_bearer_travels_in_a_header_and_never_in_the_url() {
        let req = post_json("https://gw/query", "tok-abc", &serde_json::json!({}));
        assert!(!req.url.contains("tok-abc"));
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer tok-abc")
        );
    }

    #[test]
    fn a_literal_token_is_passed_through_and_a_missing_secret_is_named() {
        assert_eq!(resolve_secret("gw_abc").unwrap(), "gw_abc");
        let err = resolve_secret("secret:SQL_TOKEN_THAT_IS_ABSENT").unwrap_err();
        assert!(err.contains("SQL_TOKEN_THAT_IS_ABSENT"));
    }
}
