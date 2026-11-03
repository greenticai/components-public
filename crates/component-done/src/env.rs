//! Everything in this component that touches the host: secrets, HTTP, the
//! clock and entropy. Behind a trait so `ops` is testable off-wasm with a
//! scripted host, which is where the real coverage is.

/// A self-contained request.
#[derive(Debug, Clone)]
pub struct HttpReq {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// Per-attempt timeout; `None` means the host default.
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

pub trait Env {
    /// Read a secret by name (`done/api_key`). `Ok(None)` means it is not set.
    fn secret(&self, name: &str) -> Result<Option<String>, String>;
    /// One HTTPS round trip. `Err` is a transport failure, never an HTTP status.
    fn send(&self, req: HttpReq) -> Result<Response, String>;
    fn now_ms(&self) -> u64;
    fn random(&self) -> [u8; 10];
    /// Wait between in-run retries. The default does not wait, so a scripted
    /// host in a test never sleeps; the real host overrides it.
    fn sleep_ms(&self, _ms: u64) {}
}

/// Resolve `secret:NAME` through the host; anything else is a literal.
///
/// The error text names the secret, never a value, so it is safe to show an
/// operator. An unset secret is an error here: both credentials this component
/// reads are required, and treating "absent" as "empty" would send an
/// unauthenticated call that the Core then refuses with a less useful message.
pub fn resolve(env: &dyn Env, raw: &str) -> Result<String, String> {
    match raw.strip_prefix("secret:") {
        Some(name) => match env.secret(name) {
            Ok(Some(v)) if !v.trim().is_empty() => Ok(v),
            Ok(_) => Err(format!("secret not set: {name}")),
            Err(_) => Err(format!("failed to read secret: {name}")),
        },
        None => Ok(raw.to_string()),
    }
}

#[cfg(all(target_arch = "wasm32", feature = "host"))]
pub struct HostEnv;

#[cfg(all(target_arch = "wasm32", feature = "host"))]
impl Env for HostEnv {
    fn secret(&self, name: &str) -> Result<Option<String>, String> {
        use greentic_interfaces_guest::secrets_store;
        match secrets_store::get(name) {
            Ok(Some(bytes)) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "secret is not valid utf-8".to_string()),
            Ok(None) => Ok(None),
            Err(_) => Err("secret read failed".to_string()),
        }
    }

    fn send(&self, req: HttpReq) -> Result<Response, String> {
        use greentic_interfaces_guest::http_client_v1_1 as client;
        let wasm_req = client::Request {
            method: req.method,
            url: req.url,
            headers: req.headers,
            body: req.body,
        };
        let options = client::RequestOptions {
            timeout_ms: Some(u32::try_from(req.timeout_ms.unwrap_or(30_000)).unwrap_or(30_000)),
            allow_insecure: Some(false),
            follow_redirects: Some(false),
        };
        match client::send(&wasm_req, Some(options), None) {
            Ok(resp) => Ok(Response {
                status: resp.status,
                body: resp.body.unwrap_or_default(),
            }),
            Err(err) => Err(format!("http send failed: {}", err.code)),
        }
    }

    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
            .unwrap_or(0)
    }

    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    fn random(&self) -> [u8; 10] {
        let mut buf = [0u8; 10];
        // A failed entropy read must not fall back to zeros: two requests would
        // then share a random suffix inside one millisecond. Mix the clock in
        // as a last resort so ids still differ, and let the Core's conflict
        // check be the backstop.
        if getrandom::fill(&mut buf).is_err() {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            buf.copy_from_slice(&nanos.to_be_bytes()[6..16]);
        }
        buf
    }
}
