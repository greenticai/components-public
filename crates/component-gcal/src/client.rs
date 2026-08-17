//! Pure shared HTTP call type and query-string encoding for the Google
//! Calendar extension. No WIT imports — this module is fully host-testable;
//! the actual `extension-host/http` invocation happens in `tools/*`.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
/// HTTP method for a Google Calendar REST API request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    /// The uppercase HTTP verb, as expected by `extension-host/http`.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// A single Google Calendar REST API request, decoupled from the WIT host
/// call so it can be constructed and asserted on in pure unit tests.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpCall {
    pub method: Method,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Option<serde_json::Value>,
}

/// Percent-encode a single value for a URL query string or an
/// `application/x-www-form-urlencoded` body. Encodes every byte that is not
/// in the unreserved set (`A-Z a-z 0-9 - _ . ~`) as `%XX`.
pub(crate) fn percent_encode(value: &str) -> String {
    const UPPER_HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push('%');
                encoded.push(UPPER_HEX[(byte >> 4) as usize] as char);
                encoded.push(UPPER_HEX[(byte & 0x0f) as usize] as char);
            }
        }
    }
    encoded
}

/// Build a URL query string from key/value pairs, percent-encoding each
/// key and value. Returns `""` for an empty slice; otherwise
/// `"?k=v&k2=v2"`.
#[must_use]
pub fn encode_query(pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let mut out = String::from("?");
    for (index, (key, value)) in pairs.iter().enumerate() {
        if index > 0 {
            out.push('&');
        }
        out.push_str(&percent_encode(key));
        out.push('=');
        out.push_str(&percent_encode(value));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn method_str() {
        assert_eq!(Method::Get.as_str(), "GET");
        assert_eq!(Method::Delete.as_str(), "DELETE");
    }
    #[test]
    fn query_empty_and_encoded() {
        assert_eq!(encode_query(&[]), "");
        assert_eq!(
            encode_query(&[
                ("jql".into(), "project = AB".into()),
                ("max".into(), "10".into())
            ]),
            "?jql=project%20%3D%20AB&max=10"
        );
    }
}
