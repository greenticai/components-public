//! Auth header builders. Pure in-process logic; no I/O.
//!
//! Supported auth types: none, bearer, api_key, basic.
//! OAuth2 flows are handled by a separate extension (out of scope).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    None,
    Bearer,
    ApiKey,
    Basic,
}

impl AuthType {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "bearer" => Some(Self::Bearer),
            "api_key" => Some(Self::ApiKey),
            "basic" => Some(Self::Basic),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bearer => "bearer",
            Self::ApiKey => "api_key",
            Self::Basic => "basic",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthHeader {
    pub name: String,
    pub value: String,
}

pub fn build_auth_header(
    t: AuthType,
    token: Option<&str>,
    api_key_header: Option<&str>,
) -> Option<AuthHeader> {
    match t {
        AuthType::None => None,
        AuthType::Bearer => token.map(|tok| AuthHeader {
            name: "Authorization".into(),
            value: format!("Bearer {tok}"),
        }),
        AuthType::ApiKey => token.map(|tok| AuthHeader {
            name: api_key_header.unwrap_or("X-API-Key").to_string(),
            value: tok.to_string(),
        }),
        AuthType::Basic => token.map(|user_pass| AuthHeader {
            name: "Authorization".into(),
            value: format!("Basic {}", base64_encode(user_pass.as_bytes())),
        }),
    }
}

/// Minimal base64 encoder for the tiny auth-header use case (~30 lines, avoids a dep).
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(4 * input.len().div_ceil(3));
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0b11) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(((b1 & 0b1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(b2 & 0b111111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_empty() {
        assert_eq!(base64_encode(b""), "");
    }
    #[test]
    fn base64_f() {
        assert_eq!(base64_encode(b"f"), "Zg==");
    }
    #[test]
    fn base64_foobar() {
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
    #[test]
    fn base64_alice() {
        assert_eq!(base64_encode(b"alice:s3cret"), "YWxpY2U6czNjcmV0");
    }
}
