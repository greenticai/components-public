//! Pure request-shaping for the Resend send_email tool. No WIT/network imports.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
pub struct SendEmailInput {
    pub from: String,
    pub to: Value, // string or array of strings, passed through to Resend
    pub subject: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub cc: Option<Value>,
    #[serde(default)]
    pub bcc: Option<Value>,
    #[serde(default)]
    pub reply_to: Option<Value>,
}

fn recipients_ok(v: &Value) -> bool {
    match v {
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(a) => !a.is_empty(),
        _ => false,
    }
}

fn non_empty(opt: Option<&String>) -> bool {
    opt.is_some_and(|s| !s.trim().is_empty())
}

/// Build the `POST /emails` body. Validates and omits absent optionals.
pub fn build_send_email_body(input: &SendEmailInput) -> Result<Value, String> {
    if input.from.trim().is_empty() {
        return Err("from must not be empty".to_string());
    }
    if input.subject.trim().is_empty() {
        return Err("subject must not be empty".to_string());
    }
    if !recipients_ok(&input.to) {
        return Err("to must be a non-empty email or array of emails".to_string());
    }
    if !non_empty(input.text.as_ref()) && !non_empty(input.html.as_ref()) {
        return Err("at least one of text or html must be provided".to_string());
    }
    let mut body = serde_json::Map::new();
    body.insert("from".to_string(), json!(input.from));
    body.insert("to".to_string(), input.to.clone());
    body.insert("subject".to_string(), json!(input.subject));
    if let Some(text) = &input.text
        && !text.trim().is_empty()
    {
        body.insert("text".to_string(), json!(text));
    }
    if let Some(html) = &input.html
        && !html.trim().is_empty()
    {
        body.insert("html".to_string(), json!(html));
    }
    if let Some(cc) = &input.cc {
        body.insert("cc".to_string(), cc.clone());
    }
    if let Some(bcc) = &input.bcc {
        body.insert("bcc".to_string(), bcc.clone());
    }
    if let Some(reply_to) = &input.reply_to {
        body.insert("reply_to".to_string(), reply_to.clone());
    }
    Ok(Value::Object(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_minimal_text_email() {
        let input: SendEmailInput = serde_json::from_str(
            r#"{"from":"Bot <b@x.com>","to":"u@x.com","subject":"Hi","text":"hello"}"#,
        )
        .unwrap();
        let body = build_send_email_body(&input).unwrap();
        assert_eq!(body["from"], "Bot <b@x.com>");
        assert_eq!(body["to"], "u@x.com");
        assert_eq!(body["subject"], "Hi");
        assert_eq!(body["text"], "hello");
        assert!(body.get("html").is_none());
    }

    #[test]
    fn accepts_array_recipients_and_html() {
        let input: SendEmailInput = serde_json::from_str(
            r#"{"from":"b@x.com","to":["a@x.com","b@x.com"],"subject":"H","html":"<p>hi</p>","cc":"c@x.com"}"#,
        )
        .unwrap();
        let body = build_send_email_body(&input).unwrap();
        assert_eq!(body["to"], json!(["a@x.com", "b@x.com"]));
        assert_eq!(body["html"], "<p>hi</p>");
        assert_eq!(body["cc"], "c@x.com");
        assert!(body.get("text").is_none());
    }

    #[test]
    fn rejects_missing_body() {
        let input: SendEmailInput =
            serde_json::from_str(r#"{"from":"b@x.com","to":"u@x.com","subject":"H"}"#).unwrap();
        assert!(build_send_email_body(&input).is_err());
    }

    #[test]
    fn rejects_empty_from_subject_or_recipients() {
        let bad_from: SendEmailInput =
            serde_json::from_str(r#"{"from":"  ","to":"u@x.com","subject":"H","text":"x"}"#)
                .unwrap();
        assert!(build_send_email_body(&bad_from).is_err());
        let bad_subject: SendEmailInput =
            serde_json::from_str(r#"{"from":"b@x.com","to":"u@x.com","subject":"  ","text":"x"}"#)
                .unwrap();
        assert!(build_send_email_body(&bad_subject).is_err());
        let empty_to: SendEmailInput =
            serde_json::from_str(r#"{"from":"b@x.com","to":[],"subject":"H","text":"x"}"#).unwrap();
        assert!(build_send_email_body(&empty_to).is_err());
    }

    #[test]
    fn blank_text_with_html_is_ok_and_omits_text() {
        let input: SendEmailInput = serde_json::from_str(
            r#"{"from":"b@x.com","to":"u@x.com","subject":"H","text":"   ","html":"<p>x</p>"}"#,
        )
        .unwrap();
        let body = build_send_email_body(&input).unwrap();
        assert!(body.get("text").is_none());
        assert_eq!(body["html"], "<p>x</p>");
    }
}
