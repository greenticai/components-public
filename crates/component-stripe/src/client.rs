//! Pure shared HTTP call type, query-string encoding, and Stripe's
//! `application/x-www-form-urlencoded` body encoding for the Stripe
//! extension. No WIT imports — this module is fully host-testable; the
//! actual `extension-host/http` invocation happens in `tools/*`.

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
/// HTTP method for a Stripe REST API request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
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
            Method::Delete => "DELETE",
        }
    }
}

/// A single Stripe REST API request, decoupled from the WIT host call so it
/// can be constructed and asserted on in pure unit tests.
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

/// Encode a JSON object into Stripe's `application/x-www-form-urlencoded`
/// request body format.
///
/// Stripe's REST API takes POST parameters as a form body using bracket
/// notation for nesting, not JSON:
/// - a scalar (string/number/bool) becomes `key=percent_encoded_value`;
/// - a nested object becomes `parent[child]=...`, recursively;
/// - an array (of scalars or objects) is indexed: `key[0]=...`, `key[1]=...`;
/// - `null` values are omitted entirely (Stripe treats an omitted parameter
///   the same as an explicit empty one for most endpoints, and including
///   `key=null` would send the literal string `"null"`).
///
/// Pairs are joined with `&`. A non-object `value` (or an empty object)
/// encodes to `""`.
///
/// Ordering: `serde_json::Value::Object` is backed by a `BTreeMap` unless
/// the `preserve_order` feature is enabled (it is not, in this crate), so
/// object keys are already yielded in sorted order — the output is
/// deterministic without any extra sorting step here.
#[must_use]
pub fn form_encode_value(value: &serde_json::Value) -> String {
    let mut pairs = Vec::new();
    form_encode_into(None, value, &mut pairs);
    pairs.join("&")
}

/// Recursive worker for [`form_encode_value`]. `prefix` is the bracket-
/// notation key built so far (`None` only at the top level, where object
/// keys become bare top-level keys).
fn form_encode_into(prefix: Option<&str>, value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                let next = match prefix {
                    Some(parent) => format!("{parent}[{key}]"),
                    None => key.clone(),
                };
                form_encode_into(Some(&next), val, out);
            }
        }
        serde_json::Value::Array(items) => {
            let base = prefix.unwrap_or_default();
            for (index, item) in items.iter().enumerate() {
                let next = format!("{base}[{index}]");
                form_encode_into(Some(&next), item, out);
            }
        }
        serde_json::Value::Null => {
            // Omit nulls entirely rather than sending the literal "null".
        }
        scalar => {
            let key = prefix.unwrap_or_default();
            out.push(format!(
                "{key}={}",
                percent_encode(&scalar_to_string(scalar))
            ));
        }
    }
}

/// Render a JSON scalar (string/number/bool) as the raw string Stripe
/// expects before percent-encoding. Non-scalars never reach here (handled
/// by the `Object`/`Array`/`Null` arms in [`form_encode_into`]).
fn scalar_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null | serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
                ("query".into(), "amount > 100".into()),
                ("limit".into(), "10".into())
            ]),
            "?query=amount%20%3E%20100&limit=10"
        );
    }

    // ===== form_encode_value =====

    #[test]
    fn form_encode_empty_object_is_empty_string() {
        assert_eq!(form_encode_value(&json!({})), "");
    }

    #[test]
    fn form_encode_flat_scalars() {
        let value = json!({
            "amount": 2000,
            "currency": "usd",
            "capture": true
        });
        // serde_json (without the `preserve_order` feature) backs `Map` with a
        // `BTreeMap`, so key iteration is already sorted — deterministic
        // without any extra bookkeeping here.
        assert_eq!(
            form_encode_value(&value),
            "amount=2000&capture=true&currency=usd"
        );
    }

    #[test]
    fn form_encode_nested_object_uses_bracket_notation() {
        let value = json!({
            "metadata": { "order": "42" }
        });
        assert_eq!(form_encode_value(&value), "metadata[order]=42");
    }

    #[test]
    fn form_encode_deeply_nested_object() {
        let value = json!({
            "shipping": { "address": { "city": "SF", "postal_code": "94107" } }
        });
        assert_eq!(
            form_encode_value(&value),
            "shipping[address][city]=SF&shipping[address][postal_code]=94107"
        );
    }

    #[test]
    fn form_encode_array_of_objects_is_indexed() {
        let value = json!({
            "line_items": [
                { "price": "p1", "quantity": 1 }
            ]
        });
        assert_eq!(
            form_encode_value(&value),
            "line_items[0][price]=p1&line_items[0][quantity]=1"
        );
    }

    #[test]
    fn form_encode_array_of_objects_multiple_entries() {
        let value = json!({
            "line_items": [
                { "price": "p1", "quantity": 1 },
                { "price": "p2", "quantity": 3 }
            ]
        });
        assert_eq!(
            form_encode_value(&value),
            "line_items[0][price]=p1&line_items[0][quantity]=1&line_items[1][price]=p2&line_items[1][quantity]=3"
        );
    }

    #[test]
    fn form_encode_array_of_scalars_is_indexed() {
        let value = json!({ "expand": ["customer", "invoice"] });
        assert_eq!(
            form_encode_value(&value),
            "expand[0]=customer&expand[1]=invoice"
        );
    }

    #[test]
    fn form_encode_percent_encodes_special_characters() {
        let value = json!({ "description": "50% off & free!" });
        assert_eq!(
            form_encode_value(&value),
            "description=50%25%20off%20%26%20free%21"
        );
    }

    #[test]
    fn form_encode_null_values_are_omitted() {
        let value = json!({ "amount": 100, "coupon": null });
        assert_eq!(form_encode_value(&value), "amount=100");
    }

    #[test]
    fn form_encode_combined_scalar_nested_and_array() {
        let value = json!({
            "amount": 2000,
            "currency": "usd",
            "metadata": { "order": "42" },
            "expand": ["customer"]
        });
        assert_eq!(
            form_encode_value(&value),
            "amount=2000&currency=usd&expand[0]=customer&metadata[order]=42"
        );
    }
}
