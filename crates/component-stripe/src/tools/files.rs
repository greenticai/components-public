//! `stripe_files` tool domain — multipart body building, input parsing, and
//! response normalization for the Stripe Files API (`POST
//! https://files.stripe.com/v1/files`). No WIT imports — this module is fully
//! host-testable. The actual `http::fetch` call and `describe()` entry live in
//! `lib.rs` / `tool_meta.rs` (Task 2).

// Copied verbatim from the greentic.stripe design extension. The only edit is
// this attribute: the tool-metadata tables and several op enums exist for the
// TOOL surface and are unused by the node surface. Silencing it here keeps the
// rest of the file diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::Value;

/// Fixed multipart boundary. A static boundary is safe here: the file bytes
/// are arbitrary binary data, but the boundary is long and random-looking
/// enough that a collision is astronomically improbable. (Wasm has no
/// `Math.random` equivalent in a pure no-std context.)
pub const MULTIPART_BOUNDARY: &str = "----GreenticStripeFilesBoundary7MA4YWxkTrZu0gW";

/// Return the `Content-Type` header value for a multipart/form-data request
/// using [`MULTIPART_BOUNDARY`].
#[must_use]
pub fn content_type_header() -> String {
    format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}")
}

/// Sanitize a filename so it cannot inject control characters into the
/// `Content-Disposition` header. Maps `"`, `\`, `\r`, and `\n` to `_`;
/// falls back to `"upload"` if the result is empty.
fn sanitize_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\r' | '\n' => '_',
            other => other,
        })
        .collect();
    if sanitized.is_empty() {
        "upload".to_string()
    } else {
        sanitized
    }
}

/// Build the raw `multipart/form-data` body for a Stripe file upload.
///
/// Assembles the two form parts (`purpose` and `file`) plus the closing
/// boundary using CRLF line endings, as required by RFC 2046. The binary
/// `file_bytes` are appended verbatim without any base64 re-encoding.
#[must_use]
pub fn build_multipart_body(purpose: &str, filename: &str, file_bytes: &[u8]) -> Vec<u8> {
    let b = MULTIPART_BOUNDARY;
    let safe_filename = sanitize_filename(filename);

    let mut body: Vec<u8> = Vec::new();

    // Part 1: purpose
    let part1 =
        format!("--{b}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\n{purpose}\r\n");
    body.extend_from_slice(part1.as_bytes());

    // Part 2: file header
    let part2_header = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{safe_filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    body.extend_from_slice(part2_header.as_bytes());

    // Part 2: file bytes (binary, not re-encoded)
    body.extend_from_slice(file_bytes);

    // Part 2: trailing CRLF + closing boundary
    let closing = format!("\r\n--{b}--\r\n");
    body.extend_from_slice(closing.as_bytes());

    body
}

/// Raw `stripe_files` tool input, deserialized from the model-supplied
/// `args_json`. `filename` and `purpose` are optional — the dispatch layer
/// defaults `purpose` to `"dispute_evidence"` and `filename` to `"upload"`.
#[derive(Debug, Deserialize)]
pub struct FilesInput {
    /// The file bytes, base64-encoded. Required.
    pub file_base64: String,
    /// Suggested filename for the uploaded file. Optional.
    #[serde(default)]
    pub filename: Option<String>,
    /// Stripe file purpose (e.g. `"dispute_evidence"`). Optional.
    #[serde(default)]
    pub purpose: Option<String>,
}

/// Parse `args_json` into a [`FilesInput`].
///
/// Returns `Err` with a human-readable message on any JSON parse failure.
pub fn parse_input(args_json: &str) -> Result<FilesInput, String> {
    serde_json::from_str(args_json).map_err(|e| format!("invalid input: {e}"))
}

/// Map a raw Stripe Files API response body to the compact shape returned to
/// the model: `{id, purpose, size, type, url}`. Every field falls back to
/// `null` rather than failing if the Stripe response omits it.
pub fn normalize(raw: &[u8]) -> Result<Value, String> {
    let v: Value =
        serde_json::from_slice(raw).map_err(|e| format!("invalid file response: {e}"))?;
    Ok(serde_json::json!({
        "id":      v.get("id").cloned().unwrap_or(Value::Null),
        "purpose": v.get("purpose").cloned().unwrap_or(Value::Null),
        "size":    v.get("size").cloned().unwrap_or(Value::Null),
        "type":    v.get("type").cloned().unwrap_or(Value::Null),
        "url":     v.get("url").cloned().unwrap_or(Value::Null),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_multipart_body ──────────────────────────────────────────────────

    #[test]
    fn multipart_body_starts_with_boundary() {
        let body = build_multipart_body("dispute_evidence", "ev.pdf", b"PDFDATA");
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.starts_with(&format!("--{MULTIPART_BOUNDARY}")),
            "body must start with the opening boundary"
        );
    }

    #[test]
    fn multipart_body_contains_purpose_part() {
        let body = build_multipart_body("dispute_evidence", "ev.pdf", b"PDFDATA");
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("name=\"purpose\""),
            "missing purpose field name"
        );
        assert!(text.contains("dispute_evidence"), "missing purpose value");
    }

    #[test]
    fn multipart_body_contains_file_part() {
        let body = build_multipart_body("dispute_evidence", "ev.pdf", b"PDFDATA");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("filename=\"ev.pdf\""), "missing filename");
        assert!(text.contains("PDFDATA"), "missing file bytes");
    }

    #[test]
    fn multipart_body_ends_with_closing_boundary() {
        let body = build_multipart_body("dispute_evidence", "ev.pdf", b"PDFDATA");
        let closing = format!("--{MULTIPART_BOUNDARY}--\r\n");
        assert!(
            body.ends_with(closing.as_bytes()),
            "body must end with the closing boundary"
        );
    }

    // ── sanitize_filename ────────────────────────────────────────────────────

    #[test]
    fn sanitize_filename_replaces_injection_chars() {
        assert_eq!(sanitize_filename("a\"b\nc\\d"), "a_b_c_d");
    }

    #[test]
    fn sanitize_filename_empty_returns_upload() {
        assert_eq!(sanitize_filename(""), "upload");
    }

    #[test]
    fn sanitize_filename_cr_replaced() {
        assert_eq!(sanitize_filename("a\rb"), "a_b");
    }

    // ── normalize ────────────────────────────────────────────────────────────

    #[test]
    fn normalize_extracts_all_fields() {
        let raw = br#"{"id":"file_1","purpose":"dispute_evidence","size":10,"type":"pdf","url":"https://x"}"#;
        let out = normalize(raw).unwrap();
        assert_eq!(out["id"], "file_1");
        assert_eq!(out["purpose"], "dispute_evidence");
        assert_eq!(out["size"], 10);
        assert_eq!(out["type"], "pdf");
        assert_eq!(out["url"], "https://x");
    }

    #[test]
    fn normalize_returns_null_for_missing_fields() {
        let raw = br#"{"id":"file_2"}"#;
        let out = normalize(raw).unwrap();
        assert_eq!(out["id"], "file_2");
        assert_eq!(out["purpose"], serde_json::Value::Null);
        assert_eq!(out["size"], serde_json::Value::Null);
        assert_eq!(out["type"], serde_json::Value::Null);
        assert_eq!(out["url"], serde_json::Value::Null);
    }

    #[test]
    fn normalize_errors_on_invalid_json() {
        assert!(normalize(b"not json").is_err());
    }

    // ── parse_input ──────────────────────────────────────────────────────────

    #[test]
    fn parse_input_minimal_input_succeeds() {
        let input = parse_input(r#"{"file_base64":"AAAA"}"#).unwrap();
        assert_eq!(input.file_base64, "AAAA");
        assert!(input.filename.is_none());
        assert!(input.purpose.is_none());
    }

    #[test]
    fn parse_input_full_input_succeeds() {
        let input = parse_input(
            r#"{"file_base64":"AAAA","filename":"ev.pdf","purpose":"dispute_evidence"}"#,
        )
        .unwrap();
        assert_eq!(input.file_base64, "AAAA");
        assert_eq!(input.filename.as_deref(), Some("ev.pdf"));
        assert_eq!(input.purpose.as_deref(), Some("dispute_evidence"));
    }

    #[test]
    fn parse_input_errors_on_invalid_json() {
        assert!(parse_input("{bad json").is_err());
    }

    #[test]
    fn parse_input_errors_on_missing_required_field() {
        assert!(parse_input(r#"{"filename":"ev.pdf"}"#).is_err());
    }
}
