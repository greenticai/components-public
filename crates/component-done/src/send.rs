//! `send_card` and `send_text`: hand a rendered card or a notice to the Done
//! Core, which sends it through the channel's provider (Core / channel trigger,
//! the same path as `deliver_reminder`).
//!
//! Unlike the other ops, a failed send still answers with `status: "failed"`
//! and an `outcomes` row, because `done.deliver` / `done.notify` route straight
//! to `record_deliveries` with `outcomes` and then test `status == "sent"`.
//! A failure therefore carries BOTH shapes: `{ok:false, error:{code,message}}`
//! and `{status:"failed", code, outcomes:[..]}`. A refused call that could not
//! even be addressed has an empty `outcomes`.

use serde_json::{Value, json};

use crate::card::CHANNELS;
use crate::env::Env;
use crate::ops::{classify, creds, err, post, str_field};

/// Core trigger that sends a rendered card. Rename here only.
pub const SEND_CARD_TRIGGER: &str = "done_send_card";
/// Core trigger that sends a plain-text notice. Rename here only.
pub const SEND_TEXT_TRIGGER: &str = "done_send_text";
/// Largest payload accepted, in bytes, before anything is sent.
pub const MAX_PAYLOAD_BYTES: usize = 100_000;
/// Longest notice text.
pub const MAX_TEXT_CHARS: usize = 4000;

fn outcome(subscriber: &str, ok: bool, code: Option<&str>) -> Value {
    let mut o = json!({
        "subscriber_id": subscriber,
        "status": if ok { "ok" } else { "failed" },
        "attempts_made": 1,
    });
    if let Some(c) = code {
        o["error_code"] = json!(c);
    }
    o
}

/// A failure in the shape the channel flows consume.
fn failure(error: &Value, subscriber: Option<&str>) -> Value {
    let code = error
        .pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap_or("unavailable");
    let outcomes: Vec<Value> = subscriber
        .map(|s| outcome(s, false, Some(code)))
        .into_iter()
        .collect();
    json!({
        "ok": false,
        "status": "failed",
        "code": code,
        "message_id": "",
        "error": error["error"],
        "outcomes": outcomes,
    })
}

struct Target<'a> {
    channel: &'a str,
    address: &'a str,
    recipient: &'a str,
    done_id: &'a str,
    subscriber: String,
}

fn target<'a>(input: &'a Value) -> Result<Target<'a>, Value> {
    let channel = str_field(input, "channel").unwrap_or("");
    if !CHANNELS.contains(&channel) {
        return Err(err(
            "invalid_request",
            "`channel` must be teams, webex or email",
        ));
    }
    let Some(address) = str_field(input, "address").filter(|a| !a.chars().any(char::is_control))
    else {
        return Err(err("invalid_recipient", "`address` is required"));
    };
    let Some(recipient) = str_field(input, "recipient") else {
        return Err(err("invalid_recipient", "`recipient` is required"));
    };
    let Some(done_id) = str_field(input, "done_id") else {
        return Err(err("invalid_request", "`done_id` is required"));
    };
    Ok(Target {
        channel,
        address,
        recipient,
        done_id,
        subscriber: format!("{recipient}:{channel}"),
    })
}

fn deliver(
    env: &dyn Env,
    input: &Value,
    trigger: &str,
    kind: &str,
    t: &Target<'_>,
    content: (&str, &Value),
) -> Value {
    let creds = match creds(env, input) {
        Ok(c) => c,
        Err(e) => return failure(&e, Some(&t.subscriber)),
    };
    let body = json!({
        "kind": kind,
        "channel": t.channel,
        "address": t.address,
        "recipient": t.recipient,
        "done_id": t.done_id,
        content.0: content.1,
    });
    let url = format!("{}/trigger/{trigger}", creds.base);
    match post(env, url, &creds.api_key, &body) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            let id = serde_json::from_slice::<Value>(&resp.body)
                .ok()
                .and_then(|v| {
                    v.get("message_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            json!({
                "ok": true,
                "status": "sent",
                "code": "",
                "message_id": id,
                "outcomes": [outcome(&t.subscriber, true, None)],
            })
        }
        Ok(resp) => failure(&classify(&resp), Some(&t.subscriber)),
        Err(e) => failure(&e, Some(&t.subscriber)),
    }
}

/// `send_card`. In: `channel`, `address`, `payload` (card object, or HTML for
/// email), `recipient` (Person id), `done_id`. Out: `status` = `sent|failed`,
/// `code`, `message_id`, `outcomes`.
pub fn send_card(env: &dyn Env, input: &Value) -> Value {
    let t = match target(input) {
        Ok(t) => t,
        Err(e) => return failure(&e, None),
    };
    let payload = match input.get("payload") {
        Some(p) if p.is_object() || p.as_str().is_some_and(|s| !s.trim().is_empty()) => p,
        _ => {
            return failure(
                &err(
                    "invalid_request",
                    "`payload` must be a card object or HTML text",
                ),
                Some(&t.subscriber),
            );
        }
    };
    if payload.to_string().len() > MAX_PAYLOAD_BYTES {
        return failure(
            &err("invalid_request", "`payload` is too large"),
            Some(&t.subscriber),
        );
    }
    deliver(
        env,
        input,
        SEND_CARD_TRIGGER,
        "card",
        &t,
        ("payload", payload),
    )
}

/// `send_text`. In: `channel`, `address`, `text`, `recipient`, `done_id`.
/// Out: as `send_card`.
pub fn send_text(env: &dyn Env, input: &Value) -> Value {
    let t = match target(input) {
        Ok(t) => t,
        Err(e) => return failure(&e, None),
    };
    let Some(text) = str_field(input, "text") else {
        return failure(
            &err("invalid_request", "`text` is required"),
            Some(&t.subscriber),
        );
    };
    if text.chars().count() > MAX_TEXT_CHARS {
        return failure(
            &err(
                "invalid_request",
                format!("`text` is over {MAX_TEXT_CHARS} characters"),
            ),
            Some(&t.subscriber),
        );
    }
    deliver(
        env,
        input,
        SEND_TEXT_TRIGGER,
        "text",
        &t,
        ("text", &json!(text)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{HttpReq, Response};
    use std::cell::RefCell;

    struct Core {
        status: u16,
        body: &'static str,
        sent: RefCell<Vec<HttpReq>>,
    }
    impl Core {
        fn new(status: u16, body: &'static str) -> Self {
            Self {
                status,
                body,
                sent: RefCell::new(Vec::new()),
            }
        }
    }
    impl Env for Core {
        fn secret(&self, name: &str) -> Result<Option<String>, String> {
            Ok(match name {
                "done/api_key" => Some("KEY".into()),
                "done/base_url" => Some("https://core.example.com".into()),
                _ => None,
            })
        }
        fn send(&self, req: HttpReq) -> Result<Response, String> {
            self.sent.borrow_mut().push(req);
            if self.status == 0 {
                return Err("down".into());
            }
            Ok(Response {
                status: self.status,
                body: self.body.as_bytes().to_vec(),
            })
        }
        fn now_ms(&self) -> u64 {
            0
        }
        fn random(&self) -> [u8; 10] {
            [0; 10]
        }
    }

    fn card_in() -> Value {
        json!({
            "channel": "teams", "address": "19:abc", "payload": {"type":"AdaptiveCard"},
            "recipient": "person-1", "done_id": "id-1",
            "api_key": "secret:done/api_key", "base_url": "secret:done/base_url",
        })
    }

    #[test]
    fn a_sent_card_posts_to_the_core_and_reports_an_outcome_row() {
        let core = Core::new(200, r#"{"message_id":"m-1"}"#);
        let out = send_card(&core, &card_in());
        assert_eq!(out["status"], "sent");
        assert_eq!(out["message_id"], "m-1");
        assert_eq!(out["outcomes"][0]["subscriber_id"], "person-1:teams");
        assert_eq!(out["outcomes"][0]["status"], "ok");
        let sent = core.sent.borrow();
        assert_eq!(
            sent[0].url,
            format!("https://core.example.com/trigger/{SEND_CARD_TRIGGER}")
        );
        let body: Value = serde_json::from_slice(sent[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(body["payload"]["type"], "AdaptiveCard");
        assert_eq!(body["kind"], "card");
        assert!(!body.to_string().contains("KEY"));
    }

    #[test]
    fn a_refused_send_is_a_failed_status_with_a_failed_outcome() {
        let core = Core::new(429, "{}");
        let out = send_card(&core, &card_in());
        assert_eq!(out["ok"], false);
        assert_eq!(out["status"], "failed");
        assert_eq!(out["code"], "rate_limited");
        assert_eq!(out["error"]["code"], "rate_limited");
        assert_eq!(out["outcomes"][0]["status"], "failed");
        assert_eq!(out["outcomes"][0]["error_code"], "rate_limited");
    }

    #[test]
    fn a_transport_failure_is_unavailable_and_still_records() {
        let out = send_text(
            &Core::new(0, ""),
            &json!({
                "channel": "email", "address": "a@b.c", "text": "hi", "recipient": "p", "done_id": "i",
                "api_key": "secret:done/api_key", "base_url": "secret:done/base_url",
            }),
        );
        assert_eq!(out["status"], "failed");
        assert_eq!(out["code"], "unavailable");
        assert_eq!(out["outcomes"][0]["subscriber_id"], "p:email");
    }

    #[test]
    fn bad_input_sends_nothing() {
        let core = Core::new(200, "{}");
        for (k, v) in [
            ("channel", json!("sms")),
            ("address", json!("")),
            ("recipient", json!("")),
            ("done_id", json!("")),
            ("payload", json!("  ")),
            ("payload", json!(5)),
        ] {
            let mut i = card_in();
            i[k] = v;
            let out = send_card(&core, &i);
            assert_eq!(
                (out["ok"].clone(), out["status"].clone()),
                (json!(false), json!("failed")),
                "{k}"
            );
        }
        assert!(core.sent.borrow().is_empty());
        let mut no_creds = card_in();
        no_creds.as_object_mut().unwrap().remove("api_key");
        let out = send_card(&core, &no_creds);
        assert_eq!(out["code"], "unauthorised");
        assert!(core.sent.borrow().is_empty());
    }

    #[test]
    fn send_text_posts_the_text_to_its_own_trigger() {
        let core = Core::new(202, "");
        let out = send_text(
            &core,
            &json!({
                "channel": "webex", "address": "room", "text": "Alice accepted.", "recipient": "p", "done_id": "i",
                "api_key": "secret:done/api_key", "base_url": "secret:done/base_url",
            }),
        );
        assert_eq!(out["status"], "sent");
        assert_eq!(out["message_id"], "");
        let sent = core.sent.borrow();
        assert!(sent[0].url.ends_with(SEND_TEXT_TRIGGER));
        let body: Value = serde_json::from_slice(sent[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(body["text"], "Alice accepted.");
        let long = send_text(
            &core,
            &json!({
                "channel": "webex", "address": "room", "text": "x".repeat(MAX_TEXT_CHARS + 1),
                "recipient": "p", "done_id": "i",
                "api_key": "secret:done/api_key", "base_url": "secret:done/base_url",
            }),
        );
        assert_eq!(long["status"], "failed");
    }
}
