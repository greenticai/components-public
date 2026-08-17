//! The host dependencies — secrets and HTTP — behind traits so tool logic is
//! testable on the host.
//!
//! Copied from the design extension, minus two things it has and this does not:
//! its `wasm_host` module (which forwards to `greentic:extension-host/*`, which
//! a flow component cannot import — `transport` replaces it), and its `Logger`
//! trait, whose one caller was the audit line the extension emits after a
//! successful WRITE. No write handler is ported here, so there is nothing to
//! audit.

/// Resolve a `secret://...` URI to its plaintext value.
pub trait SecretStore {
    fn get(&self, uri: &str) -> Result<String, String>;
}

/// Minimal HTTP transport (the host `http` capability shape).
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

pub trait HttpTransport {
    fn send(
        &self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, String>;
}
