//! `deliver_events`: POST one Done event to every subscriber, HMAC-signed.
//! Spec: `docs/done/flows-aux/README.md` and `trigger.done-event.json`.
//!
//! Delivery is at least once. The signed bytes are exactly the bytes sent. A
//! subscriber's secret is resolved here from its NAME (`secret_name`), so no
//! secret value passes through the flow. This op reports per-target outcomes
//! (`ok` | `failed`); the flow owns attempt counting, the backoff ladder and
//! dead-lettering.

use serde_json::{Value, json};

use crate::env::{Env, HttpReq};
use crate::mac;
use crate::ops::{err, str_field};

pub const MAX_TARGETS: usize = 20;
pub const DEFAULT_MAX_BODY: usize = 65_536;
pub const HARD_MAX_BODY: usize = 262_144;
const DEFAULT_BACKOFF_S: [u64; 3] = [1, 5, 25];
const MAX_BACKOFF_STEPS: usize = 5;
const MAX_BACKOFF_S: u64 = 60;

fn uint(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// An HTTP header name: RFC 9110 token characters only.
fn header_name(input: &Value, key: &str, default: &str) -> Result<String, Value> {
    let name = str_field(input, key).unwrap_or(default);
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
    if ok {
        Ok(name.to_string())
    } else {
        Err(err(
            "invalid_request",
            format!("`{key}` is not a valid header name"),
        ))
    }
}

fn backoff(input: &Value) -> Result<Vec<u64>, Value> {
    let Some(raw) = input.get("in_run_backoff_s") else {
        return Ok(DEFAULT_BACKOFF_S.to_vec());
    };
    let bad = || {
        err(
            "invalid_request",
            "`in_run_backoff_s` must be up to 5 whole seconds, each at most 60",
        )
    };
    let list = raw.as_array().ok_or_else(bad)?;
    if list.len() > MAX_BACKOFF_STEPS {
        return Err(bad());
    }
    list.iter()
        .map(|v| uint(v).filter(|s| *s <= MAX_BACKOFF_S).ok_or_else(bad))
        .collect()
}

/// Worth another try inside this run: the subscriber may recover.
fn transient(status: u16) -> bool {
    status >= 500 || status == 408 || status == 429
}

fn outcome(id: &str, status: &str, attempts: u64) -> Value {
    json!({ "subscriber_id": id, "status": status, "attempts_made": attempts })
}

fn failed(id: &str, code: &str, attempts: u64, http_status: Option<u16>) -> Value {
    let mut o = outcome(id, "failed", attempts);
    o["error_code"] = json!(code);
    if let Some(s) = http_status {
        o["http_status"] = json!(s);
    }
    o
}

struct Delivery<'a> {
    body: &'a [u8],
    event_id: &'a str,
    signature_header: &'a str,
    signature_prefix: &'a str,
    event_id_header: &'a str,
    timeout_ms: u64,
    backoff: &'a [u64],
}

fn deliver_one(env: &dyn Env, d: &Delivery<'_>, target: &Value) -> Value {
    let Some(id) = str_field(target, "subscriber_id") else {
        return failed("", "invalid_request", 0, None);
    };
    let Some(url) =
        str_field(target, "url").filter(|u| u.starts_with("https://") && !u.contains(' '))
    else {
        return failed(id, "invalid_request", 0, None);
    };
    // The secret is read by NAME. `secret_name` is never a value.
    let Some(name) = str_field(target, "secret_name") else {
        return failed(id, "unauthorised", 0, None);
    };
    let name = name.strip_prefix("secret:").unwrap_or(name);
    let secret = match crate::env::resolve(env, &format!("secret:{name}")) {
        Ok(s) => s,
        Err(_) => return failed(id, "unauthorised", 0, None),
    };
    let signature = format!(
        "{}{}",
        d.signature_prefix,
        mac::hex(&mac::hmac_sha256(secret.as_bytes(), d.body))
    );
    let already = target.get("attempts").and_then(uint).unwrap_or(0);

    let mut made = 0u64;
    for step in 0..=d.backoff.len() {
        made += 1;
        let req = HttpReq {
            method: "POST".into(),
            url: url.to_string(),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                (d.event_id_header.into(), d.event_id.into()),
                (d.signature_header.into(), signature.clone()),
                ("x-done-attempt".into(), (already + made).to_string()),
            ],
            body: Some(d.body.to_vec()),
            timeout_ms: Some(d.timeout_ms),
        };
        let (code, http_status, retry) = match env.send(req) {
            Ok(r) if (200..300).contains(&r.status) => {
                let mut o = outcome(id, "ok", made);
                o["http_status"] = json!(r.status);
                return o;
            }
            Ok(r) => ("unavailable", Some(r.status), transient(r.status)),
            Err(_) => ("unavailable", None, true),
        };
        match d.backoff.get(step) {
            Some(wait) if retry => env.sleep_ms(wait * 1000),
            _ => return failed(id, code, made, http_status),
        }
    }
    failed(id, "unavailable", made, None)
}

pub fn deliver_events(env: &dyn Env, input: &Value) -> Value {
    let Some(event) = input.get("event").filter(|e| e.is_object()) else {
        return err("invalid_request", "`event` must be an object");
    };
    let Some(event_id) =
        str_field(event, "id").filter(|i| i.len() <= 128 && !i.contains(char::is_control))
    else {
        return err("invalid_request", "`event.id` is required");
    };
    let Some(targets) = input.get("targets").and_then(Value::as_array) else {
        return err("invalid_request", "`targets` must be an array");
    };
    if targets.len() > MAX_TARGETS {
        return err(
            "invalid_request",
            format!("`targets` is over {MAX_TARGETS}"),
        );
    }
    let (signature_header, event_id_header) = match (
        header_name(input, "signature_header", "X-Done-Signature"),
        header_name(input, "event_id_header", "X-Done-Event-Id"),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let backoff = match backoff(input) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let max_body = input
        .get("max_body_bytes")
        .and_then(uint)
        .and_then(|n| usize::try_from(n).ok())
        .map_or(DEFAULT_MAX_BODY, |n| n.min(HARD_MAX_BODY));
    let body = match serde_json::to_vec(&json!({ "event": event })) {
        Ok(b) => b,
        Err(e) => return err("invalid_request", format!("encode event: {e}")),
    };
    if body.len() > max_body {
        return err(
            "invalid_request",
            format!(
                "the event body is {} bytes; the limit is {max_body}",
                body.len()
            ),
        );
    }
    let timeout_s = input
        .get("attempt_timeout_s")
        .and_then(uint)
        .unwrap_or(10)
        .clamp(1, 30);
    let delivery = Delivery {
        body: &body,
        event_id,
        signature_header: &signature_header,
        signature_prefix: input
            .get("signature_prefix")
            .and_then(Value::as_str)
            .unwrap_or("sha256="),
        event_id_header: &event_id_header,
        timeout_ms: timeout_s * 1000,
        backoff: &backoff,
    };
    let outcomes: Vec<Value> = targets
        .iter()
        .map(|t| deliver_one(env, &delivery, t))
        .collect();
    json!({ "ok": true, "outcomes": outcomes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Response;
    use std::cell::RefCell;

    struct Net {
        script: RefCell<Vec<Result<u16, String>>>,
        sent: RefCell<Vec<HttpReq>>,
        slept: RefCell<Vec<u64>>,
    }
    impl Net {
        fn new(script: Vec<Result<u16, String>>) -> Self {
            Self {
                script: RefCell::new(script),
                sent: RefCell::new(Vec::new()),
                slept: RefCell::new(Vec::new()),
            }
        }
    }
    impl Env for Net {
        fn secret(&self, name: &str) -> Result<Option<String>, String> {
            Ok((name == "done/sub_a").then(|| "sub-secret".to_string()))
        }
        fn send(&self, req: HttpReq) -> Result<Response, String> {
            self.sent.borrow_mut().push(req);
            let mut s = self.script.borrow_mut();
            let next = if s.is_empty() { Ok(200) } else { s.remove(0) };
            next.map(|status| Response {
                status,
                body: b"secret response body".to_vec(),
            })
        }
        fn now_ms(&self) -> u64 {
            0
        }
        fn random(&self) -> [u8; 10] {
            [0; 10]
        }
        fn sleep_ms(&self, ms: u64) {
            self.slept.borrow_mut().push(ms);
        }
    }

    fn input(targets: Value) -> Value {
        json!({
            "event": {"id":"e-1","type":"done.accepted","request_id":"r-1"},
            "targets": targets,
        })
    }

    fn sub() -> Value {
        json!({"subscriber_id":"s1","url":"https://hooks.example.com/done","secret_name":"done/sub_a"})
    }

    #[test]
    fn the_signature_is_hmac_of_the_exact_bytes_sent() {
        let net = Net::new(vec![Ok(200)]);
        let out = deliver_events(&net, &input(json!([sub()])));
        assert_eq!(
            out["outcomes"][0],
            json!({"subscriber_id":"s1","status":"ok","attempts_made":1,"http_status":200})
        );
        let sent = net.sent.borrow();
        let body = sent[0].body.as_ref().unwrap();
        let want = format!(
            "sha256={}",
            mac::hex(&mac::hmac_sha256(b"sub-secret", body))
        );
        let header = |n: &str| {
            sent[0]
                .headers
                .iter()
                .find(|(k, _)| k == n)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(header("X-Done-Signature"), Some(want));
        assert_eq!(header("X-Done-Event-Id"), Some("e-1".into()));
        assert_eq!(header("x-done-attempt"), Some("1".into()));
        let parsed: Value = serde_json::from_slice(body).unwrap();
        assert_eq!(parsed["event"]["id"], "e-1");
        assert_eq!(sent[0].timeout_ms, Some(10_000));
    }

    #[test]
    fn transient_failures_retry_with_the_ladder_and_then_report_failed() {
        let net = Net::new(vec![Ok(503), Err("timeout".into()), Ok(500), Ok(502)]);
        let out = deliver_events(&net, &input(json!([sub()])));
        let o = &out["outcomes"][0];
        assert_eq!(o["status"], "failed");
        assert_eq!(o["attempts_made"], 4);
        assert_eq!(o["http_status"], 502);
        assert_eq!(*net.slept.borrow(), vec![1000, 5000, 25000]);
        // Every attempt carries the same signature.
        let sent = net.sent.borrow();
        let sig = |i: usize| {
            sent[i]
                .headers
                .iter()
                .find(|(k, _)| k == "X-Done-Signature")
                .cloned()
        };
        assert_eq!(sig(0), sig(3));
        assert!(!out.to_string().contains("secret response body"));
    }

    #[test]
    fn a_recovery_stops_retrying_and_a_permanent_refusal_does_not_retry() {
        let net = Net::new(vec![Ok(500), Ok(204)]);
        let out = deliver_events(&net, &input(json!([sub()])));
        assert_eq!(out["outcomes"][0]["status"], "ok");
        assert_eq!(out["outcomes"][0]["attempts_made"], 2);

        let net = Net::new(vec![Ok(404)]);
        let out = deliver_events(&net, &input(json!([sub()])));
        assert_eq!(out["outcomes"][0]["status"], "failed");
        assert_eq!(out["outcomes"][0]["attempts_made"], 1);
        assert!(net.slept.borrow().is_empty());
    }

    #[test]
    fn prior_attempts_continue_the_attempt_header() {
        let net = Net::new(vec![Ok(200)]);
        let mut t = sub();
        t["attempts"] = json!("3");
        deliver_events(&net, &input(json!([t])));
        let sent = net.sent.borrow();
        assert!(
            sent[0]
                .headers
                .contains(&("x-done-attempt".into(), "4".into()))
        );
    }

    #[test]
    fn bad_targets_fail_individually_without_sending() {
        let net = Net::new(vec![Ok(200)]);
        let out = deliver_events(
            &net,
            &input(json!([
                {"subscriber_id":"plain","url":"http://x.test","secret_name":"done/sub_a"},
                {"subscriber_id":"nosecret","url":"https://x.test","secret_name":"done/missing"},
                {"subscriber_id":"noname","url":"https://x.test"},
                sub(),
            ])),
        );
        let codes: Vec<&str> = out["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["error_code"].as_str().unwrap_or("ok"))
            .collect();
        assert_eq!(
            codes,
            ["invalid_request", "unauthorised", "unauthorised", "ok"]
        );
        assert_eq!(net.sent.borrow().len(), 1);
        assert!(!out.to_string().contains("sub-secret"));
    }

    #[test]
    fn a_bad_event_body_or_option_is_refused_before_any_send() {
        let net = Net::new(vec![]);
        let mut big = input(json!([sub()]));
        big["max_body_bytes"] = json!(10);
        for bad in [
            json!({}),
            json!({"event":"x","targets":[]}),
            json!({"event":{"type":"x"},"targets":[]}),
            json!({"event":{"id":"e"}}),
            big,
            {
                let mut v = input(json!([]));
                v["signature_header"] = json!("Bad Header");
                v
            },
            {
                let mut v = input(json!([]));
                v["in_run_backoff_s"] = json!([1, 2, 3, 4, 5, 6]);
                v
            },
            input(json!(vec![sub(); MAX_TARGETS + 1])),
        ] {
            let out = deliver_events(&net, &bad);
            assert_eq!(out["ok"], false, "{bad}");
            assert_eq!(out["error"]["code"], "invalid_request", "{bad}");
        }
        assert!(net.sent.borrow().is_empty());
    }
}
