//! Pure request-shaping for the Twilio send_sms tool. No WIT/network imports.
//! Builds the `application/x-www-form-urlencoded` body via `form_urlencoded`.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several input structs exist for the TOOL surface
// and are unused by the node surface, and `HttpReq`'s fields are read only on
// the wasm target. Silencing it here keeps the rest of the file diffable
// against its source.
#![allow(dead_code)]
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SendSmsInput {
    pub to: String,
    pub body: String,
}

/// Build the form-encoded body `To=..&From=..&Body=..`. `from_number` comes from
/// the `twilio/from_number` secret (resolved by the caller), not from tool input.
pub fn build_send_sms_body(to: &str, body: &str, from_number: &str) -> Result<String, String> {
    if to.trim().is_empty() {
        return Err("to must not be empty".to_string());
    }
    if body.trim().is_empty() {
        return Err("body must not be empty".to_string());
    }
    if from_number.trim().is_empty() {
        return Err("twilio/from_number secret must not be empty".to_string());
    }
    Ok(form_urlencoded::Serializer::new(String::new())
        .append_pair("To", to)
        .append_pair("From", from_number)
        .append_pair("Body", body)
        .finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_form_body_with_all_three_pairs() {
        let form = build_send_sms_body("+15551234567", "Hello world", "+15557654321").unwrap();
        // form_urlencoded encodes '+' as %2B and space as '+'
        assert!(form.contains("To=%2B15551234567"), "form was: {form}");
        assert!(form.contains("From=%2B15557654321"), "form was: {form}");
        assert!(form.contains("Body=Hello+world"), "form was: {form}");
    }

    #[test]
    fn percent_encodes_special_chars_in_body() {
        let form = build_send_sms_body("+1555", "50% off & more!", "+1999").unwrap();
        assert!(
            form.contains("Body=50%25+off+%26+more%21"),
            "form was: {form}"
        );
    }

    #[test]
    fn rejects_empty_to() {
        assert!(build_send_sms_body("   ", "hi", "+1999").is_err());
    }

    #[test]
    fn rejects_empty_body() {
        assert!(build_send_sms_body("+1555", "  ", "+1999").is_err());
    }

    #[test]
    fn rejects_empty_from_number() {
        assert!(build_send_sms_body("+1555", "hi", "").is_err());
    }

    #[test]
    fn input_decodes_to_and_body() {
        let input: SendSmsInput =
            serde_json::from_str(r#"{"to":"+15551234567","body":"hi"}"#).unwrap();
        assert_eq!(input.to, "+15551234567");
        assert_eq!(input.body, "hi");
    }
}
