//! Pure response-mapping for the Resend send_email tool. No WIT imports.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
struct ResendResponse {
    #[serde(default)]
    id: Option<String>,
    // Resend error envelope fields (permissive):
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// Parse Resend's `POST /emails` response into `{ message_id }`.
pub fn map_send_email_response(body: &[u8]) -> Result<Value, String> {
    let parsed: ResendResponse =
        serde_json::from_slice(body).map_err(|error| format!("decode resend response: {error}"))?;
    match parsed.id {
        Some(id) if !id.trim().is_empty() => Ok(json!({ "message_id": id })),
        _ => Err(parsed
            .message
            .or(parsed.name)
            .unwrap_or_else(|| "resend returned no message id".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_id_to_message_id() {
        let raw = br#"{"id":"49a3999c-0ce1-4ea6-ab68-afcd6dc2e794"}"#;
        let out = map_send_email_response(raw).unwrap();
        assert_eq!(out["message_id"], "49a3999c-0ce1-4ea6-ab68-afcd6dc2e794");
    }

    #[test]
    fn surfaces_resend_error_message() {
        let raw = br#"{"statusCode":422,"message":"The from address is not verified","name":"validation_error"}"#;
        assert_eq!(
            map_send_email_response(raw).unwrap_err(),
            "The from address is not verified"
        );
    }

    #[test]
    fn missing_id_is_err() {
        assert!(map_send_email_response(br"{}").is_err());
    }

    #[test]
    fn malformed_json_is_err() {
        assert!(map_send_email_response(b"{bad").is_err());
    }
}
