//! Pure response-mapping for the Twilio send_sms tool. No WIT imports.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several input structs exist for the TOOL surface
// and are unused by the node surface, and `HttpReq`'s fields are read only on
// the wasm target. Silencing it here keeps the rest of the file diffable
// against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
struct TwilioResponse {
    #[serde(default)]
    sid: Option<String>,
    // `status` is a string on a 2xx response ("queued") but a NUMBER in Twilio's
    // error envelope ({"status":400}). Typing it `Option<Value>` tolerates both so
    // an error body still parses (and takes the no-sid → Err(message) arm).
    #[serde(default)]
    status: Option<Value>,
    #[serde(default)]
    message: Option<String>,
}

/// Parse Twilio's Messages response into `{ message_sid, status }`.
pub fn map_send_sms_response(body: &[u8]) -> Result<Value, String> {
    let parsed: TwilioResponse =
        serde_json::from_slice(body).map_err(|error| format!("decode twilio response: {error}"))?;
    match parsed.sid {
        Some(sid) if !sid.trim().is_empty() => {
            let status = parsed
                .status
                .as_ref()
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(json!({ "message_sid": sid, "status": status }))
        }
        _ => Err(parsed
            .message
            .unwrap_or_else(|| "twilio returned no message sid".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_sid_and_status() {
        let raw = br#"{"sid":"SM1234567890abcdef","status":"queued","to":"+1555"}"#;
        let out = map_send_sms_response(raw).unwrap();
        assert_eq!(out["message_sid"], "SM1234567890abcdef");
        assert_eq!(out["status"], "queued");
    }

    #[test]
    fn missing_status_defaults_empty() {
        let raw = br#"{"sid":"SMabc"}"#;
        let out = map_send_sms_response(raw).unwrap();
        assert_eq!(out["message_sid"], "SMabc");
        assert_eq!(out["status"], "");
    }

    #[test]
    fn surfaces_twilio_error_message() {
        let raw = br#"{"code":21211,"message":"The 'To' number is not a valid phone number.","status":400}"#;
        assert_eq!(
            map_send_sms_response(raw).unwrap_err(),
            "The 'To' number is not a valid phone number."
        );
    }

    #[test]
    fn missing_sid_is_err() {
        assert!(map_send_sms_response(br"{}").is_err());
    }

    #[test]
    fn malformed_json_is_err() {
        assert!(map_send_sms_response(b"{bad").is_err());
    }
}
