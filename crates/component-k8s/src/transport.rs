//! The only part of this component that touches the host.
//!
//! The extension already put its host dependencies behind traits
//! (`host::SecretStore` / `HttpTransport` / `Logger`) so its tool logic would be
//! host-testable. That is what makes this port cheap: `catalog`, `clusters`,
//! `k8s`, `json` and both read-only tool modules copy across untouched, and only
//! the trait implementations are new. The extension backs them with
//! `greentic:extension-host/{http,secrets,logging}`, none of which a flow
//! component can import; these back them with the guest crate instead.

#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::http_client_v1_1 as client;
#[cfg(target_arch = "wasm32")]
use greentic_interfaces_guest::secrets_store;

use crate::host::{HttpResponse, HttpTransport, SecretStore};

/// A cluster API can be slow to answer a `describe` over a large namespace;
/// this matches the extension's own effective ceiling.
#[cfg(target_arch = "wasm32")]
const TIMEOUT_MS: u32 = 30_000;

/// Resolve `secret:NAME` through the secret store; anything else is a literal.
///
/// The literal branch matters: an operator may paste a token directly while
/// testing, and refusing that would make the node unusable before a secret is
/// provisioned. It is also why the token never appears in an error below.
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

/// A `SecretStore` backed by the NODE's own config rather than by a worker's
/// `secret://k8s/*` namespace.
///
/// `clusters::resolve_cluster` is copied verbatim, so it still asks for
/// `secret://k8s/<name>/api_url` and `.../token`; this answers those two from
/// the fields an operator authored on the node. Keeping the extension's
/// resolver unchanged is deliberate — it brings its own tests, including the
/// cluster-name charset check that stops a name reshaping a secret URI.
///
/// `allow_write` is answered `Err` unconditionally, which resolves to
/// `allow_write: false`. That is not a policy this component enforces so much as
/// a fact it reports: no write handler was ported, so there is nothing here for
/// a `true` to enable.
pub struct NodeSecrets {
    pub api_url: String,
    pub token: String,
}

impl SecretStore for NodeSecrets {
    fn get(&self, uri: &str) -> Result<String, String> {
        match uri.rsplit('/').next() {
            Some("api_url") => Ok(self.api_url.clone()),
            Some("token") => Ok(self.token.clone()),
            _ => Err(format!("not configured on this node: {uri}")),
        }
    }
}

pub struct HostHttp;

impl HttpTransport for HostHttp {
    #[cfg(target_arch = "wasm32")]
    fn send(
        &self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, String> {
        let req = client::Request {
            method: method.to_string(),
            url: url.to_string(),
            headers: headers.to_vec(),
            body,
        };
        let options = client::RequestOptions {
            timeout_ms: Some(TIMEOUT_MS),
            allow_insecure: Some(false),
            follow_redirects: Some(true),
        };
        match client::send(&req, Some(options), None) {
            Ok(resp) => Ok(HttpResponse {
                status: resp.status,
                body: resp.body.unwrap_or_default(),
            }),
            Err(err) => Err(format!("http send failed: {}", err.code)),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn send(
        &self,
        _method: &str,
        _url: &str,
        _headers: &[(String, String)],
        _body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, String> {
        // Host builds have no network. The copied tool modules carry their own
        // tests against a fake transport, which is where the logic is covered.
        Err("http is unavailable off-wasm".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_config_answers_the_two_uris_the_resolver_asks_for() {
        let store = NodeSecrets {
            api_url: "https://api.example".into(),
            token: "tok".into(),
        };
        assert_eq!(
            store.get("secret://k8s/prod/api_url").unwrap(),
            "https://api.example"
        );
        assert_eq!(store.get("secret://k8s/token").unwrap(), "tok");
    }

    /// `allow_write` must not be answerable. A component with no write handler
    /// that reported `allow_write: true` would be claiming a capability it does
    /// not have.
    #[test]
    fn allow_write_is_never_answered() {
        let store = NodeSecrets {
            api_url: "https://api.example".into(),
            token: "tok".into(),
        };
        assert!(store.get("secret://k8s/prod/allow_write").is_err());
        let creds = crate::clusters::resolve_cluster(&store, "prod").unwrap();
        assert!(!creds.allow_write);
    }

    #[test]
    fn a_literal_token_is_passed_through_and_a_missing_secret_is_named() {
        assert_eq!(resolve_secret("tok").unwrap(), "tok");
        let err = resolve_secret("secret:K8S_TOKEN_THAT_IS_ABSENT").unwrap_err();
        assert!(err.contains("K8S_TOKEN_THAT_IS_ABSENT"));
    }
}
