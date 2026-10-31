//! The two operations. Failure is a VALUE (`{ok:false,error:{code,message}}`),
//! never a trap and never a `NodeError`: the runner routes `on_error` only when
//! the flow draws that edge, and a trap would kill the whole run.
//!
//! The credential fields are read as `api_key` and `base_url` because that is
//! what the designer's `add_secret_refs` writes (`secret:done/api_key`,
//! `secret:done/base_url`, provider prefix dropped). Operator-facing fields
//! avoid the runner's reserved envelope keys (`operation`, `input`, `config`,
//! `payload`, `envelope`), which the runner would consume.

use serde_json::{Value, json};

use crate::env::{Env, HttpReq, Response, resolve};
use crate::uuid7;
use crate::validate;

/// The stable failure codes a flow can branch on.
pub const ERROR_CODES: &[&str] = &[
    "permission_denied",
    "blocked",
    "rate_limited",
    "invalid_recipient",
    "invalid_request",
    "unauthorised",
    "not_found",
    "unavailable",
    "bad_token",
    "bad_signature",
    "link_expired",
];

/// The Core's trigger id for `done.api.create`: served at `<base>/trigger/<id>`.
pub const CREATE_TRIGGER: &str = "done_api_create";

pub fn err(code: &str, message: impl std::fmt::Display) -> Value {
    json!({ "ok": false, "error": { "code": code, "message": message.to_string() } })
}

pub(crate) struct Creds {
    pub(crate) api_key: String,
    pub(crate) base: String,
}

pub(crate) fn creds(env: &dyn Env, input: &Value) -> Result<Creds, Value> {
    let key_raw = input
        .get("api_key")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            err(
                "unauthorised",
                "missing `api_key` (the node wires it to the secret done/api_key)",
            )
        })?;
    let api_key = resolve(env, key_raw).map_err(|e| err("unauthorised", e))?;
    let base_raw = input
        .get("base_url")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            err(
                "unavailable",
                "missing `base_url` (the node wires it to the secret done/base_url)",
            )
        })?;
    let base = resolve(env, base_raw)
        .and_then(|b| validate::base_url(&b))
        .map_err(|e| err("unavailable", e))?;
    Ok(Creds { api_key, base })
}

pub(crate) fn str_field<'a>(input: &'a Value, name: &str) -> Option<&'a str> {
    input
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Map a refused HTTP answer onto a stable code. The Core may name its own code
/// in the body (`{"error":{"code":..}}` or `{"code":..}`); a code outside
/// [`ERROR_CODES`] is ignored so a flow never branches on a string it was not
/// promised. The message never includes the request or the credential.
pub(crate) fn classify(resp: &Response) -> Value {
    let body: Value = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
    let named = body
        .pointer("/error/code")
        .or_else(|| body.get("code"))
        .and_then(Value::as_str)
        .filter(|c| ERROR_CODES.contains(c));
    let code = named.unwrap_or(match resp.status {
        401 | 403 => "unauthorised",
        404 => "not_found",
        429 => "rate_limited",
        400 | 422 => "invalid_request",
        _ => "unavailable",
    });
    err(code, format!("the Core answered {}", resp.status))
}

pub(crate) fn post(
    env: &dyn Env,
    url: String,
    api_key: &str,
    body: &Value,
) -> Result<Response, Value> {
    let bytes =
        serde_json::to_vec(body).map_err(|e| err("unavailable", format!("encode body: {e}")))?;
    env.send(HttpReq {
        method: "POST".into(),
        url,
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("authorization".into(), format!("Bearer {api_key}")),
        ],
        body: Some(bytes),
        timeout_ms: None,
    })
    .map_err(|e| err("unavailable", e))
}

/// `done.create_request`.
///
/// The id is generated HERE (UUID v7) because a webhook trigger returns no body:
/// the caller must know the id without asking. The Core deduplicates on it, so
/// a retry must reuse the id. A flow that can be re-run passes its own stable
/// `id`; otherwise each run mints a new request.
pub fn create_request(env: &dyn Env, input: &Value) -> Value {
    let creds = match creds(env, input) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(to) = str_field(input, "to") else {
        return err("invalid_recipient", "`to` is required");
    };
    let Some(text) = str_field(input, "request") else {
        return err(
            "invalid_request",
            "`request` (what you are asking for) is required",
        );
    };
    if text.chars().count() > validate::MAX_TEXT_CHARS {
        return err(
            "invalid_request",
            format!("`request` is over {} characters", validate::MAX_TEXT_CHARS),
        );
    }
    let id = match str_field(input, "id") {
        Some(given) if uuid7::is_well_formed(given) => given.to_ascii_lowercase(),
        Some(_) => return err("invalid_request", "`id` must be a UUID"),
        None => uuid7::format(env.now_ms(), env.random()),
    };

    let mut body = serde_json::Map::new();
    body.insert("id".into(), json!(id));
    body.insert("to".into(), json!(to));
    body.insert("text".into(), json!(text));
    for (field, key) in [
        ("due", "due"),
        ("process_id", "process_id"),
        ("callback_trigger", "callback_trigger"),
    ] {
        if let Some(v) = str_field(input, field) {
            body.insert(key.into(), json!(v));
        }
    }
    if let Some(context) = input.get("context").filter(|c| !c.is_null()) {
        if let Some(problem) = validate::context_problem(context) {
            return err("invalid_request", problem);
        }
        body.insert("context".into(), context.clone());
    }

    let url = format!("{}/trigger/{CREATE_TRIGGER}", creds.base);
    let resp = match post(env, url, &creds.api_key, &Value::Object(body)) {
        Ok(r) => r,
        Err(e) => return e,
    };
    if (200..300).contains(&resp.status) {
        // 202: accepted and queued. The real state (including
        // permission_pending) arrives by read or as an event.
        json!({ "ok": true, "id": id, "state": "submitted" })
    } else {
        classify(&resp)
    }
}

/// The A2A `SendMessage` request asking the Core's `done-reader` agent.
pub fn read_message(id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "SendMessage",
        "params": { "message": {
            "messageId": id,
            "role": "ROLE_USER",
            "contextId": id,
            "parts": [{ "text": json!({ "lookup": { "id": id } }).to_string() }],
        }},
    })
}

/// The text the agent answered with, if the task completed. Anything else
/// (working, input required, a JSON-RPC error) is `None`: a read has no
/// business waiting on a task, and a half answer must not look like a state.
fn reply_text(body: &Value) -> Option<String> {
    if body.get("error").is_some() {
        return None;
    }
    let result = body.get("result")?;
    let message = if let Some(m) = result.get("message") {
        m
    } else {
        let task = result.get("task")?;
        if task.pointer("/status/state").and_then(Value::as_str) != Some("TASK_STATE_COMPLETED") {
            return None;
        }
        task.pointer("/status/message")?
    };
    let text: String = message
        .get("parts")?
        .as_array()?
        .iter()
        .filter_map(|p| p.get("text").and_then(Value::as_str))
        .collect();
    (!text.trim().is_empty()).then_some(text)
}

/// `done.read_request`: `id` -> `{ok, id, state, outcome?}`.
pub fn read_request(env: &dyn Env, input: &Value) -> Value {
    let creds = match creds(env, input) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(id) = str_field(input, "id") else {
        return err("invalid_request", "`id` is required");
    };
    if !uuid7::is_well_formed(id) {
        return err("invalid_request", "`id` must be a UUID");
    }
    let id = id.to_ascii_lowercase();
    let url = format!("{}/a2a", creds.base);
    let resp = match post(env, url, &creds.api_key, &read_message(&id)) {
        Ok(r) => r,
        Err(e) => return e,
    };
    if !(200..300).contains(&resp.status) {
        return classify(&resp);
    }
    let body: Value = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
    let Some(text) = reply_text(&body) else {
        return err("unavailable", "the Core's reader did not return an answer");
    };
    // The reader replies with a JSON string. Anything else is not a state.
    let Ok(answer) = serde_json::from_str::<Value>(&text) else {
        return err(
            "unavailable",
            "the Core's reader answered with something that is not JSON",
        );
    };
    if answer.get("found") == Some(&Value::Bool(false)) {
        return err("not_found", "no request with that id");
    }
    let Some(state) = answer.get("state").and_then(Value::as_str) else {
        return err("unavailable", "the Core's reader answered without a state");
    };
    let mut out = json!({ "ok": true, "id": id, "state": state });
    if let Some(outcome) = answer.get("outcome").filter(|o| !o.is_null()) {
        out["outcome"] = outcome.clone();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        secrets: Vec<(&'static str, &'static str)>,
        reply: RefCell<Option<Result<Response, String>>>,
        sent: RefCell<Vec<HttpReq>>,
    }

    impl Fake {
        fn new(reply: Result<Response, String>) -> Self {
            Self {
                secrets: vec![
                    ("done/api_key", "k-123"),
                    ("done/base_url", "https://core.example.com/"),
                ],
                reply: RefCell::new(Some(reply)),
                sent: RefCell::new(Vec::new()),
            }
        }
        fn answering(status: u16, body: &str) -> Self {
            Self::new(Ok(Response {
                status,
                body: body.as_bytes().to_vec(),
            }))
        }
    }

    impl Env for Fake {
        fn secret(&self, name: &str) -> Result<Option<String>, String> {
            Ok(self
                .secrets
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_string()))
        }
        fn send(&self, req: HttpReq) -> Result<Response, String> {
            self.sent.borrow_mut().push(req);
            self.reply
                .borrow_mut()
                .take()
                .unwrap_or(Err("no reply scripted".into()))
        }
        fn now_ms(&self) -> u64 {
            1_700_000_000_000
        }
        fn random(&self) -> [u8; 10] {
            [7; 10]
        }
    }

    fn wired(extra: Value) -> Value {
        let mut base =
            json!({ "api_key": "secret:done/api_key", "base_url": "secret:done/base_url" });
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    }

    #[test]
    fn create_sends_the_generated_id_with_the_key_and_reports_submitted() {
        let env = Fake::answering(202, "{}");
        let out = create_request(
            &env,
            &wired(
                json!({ "to": "ana@acme.test", "request": "Approve the PO", "due": "2026-11-05" }),
            ),
        );
        assert_eq!(out["ok"], true);
        assert_eq!(out["state"], "submitted");
        let id = out["id"].as_str().unwrap();
        assert!(
            uuid7::is_well_formed(id) && id.as_bytes()[14] == b'7',
            "got {id}"
        );

        let sent = env.sent.borrow();
        assert_eq!(
            sent[0].url,
            "https://core.example.com/trigger/done_api_create"
        );
        assert!(
            sent[0]
                .headers
                .contains(&("authorization".into(), "Bearer k-123".into()))
        );
        let body: Value = serde_json::from_slice(sent[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(body["id"], id);
        assert_eq!(body["text"], "Approve the PO");
        assert!(!String::from_utf8_lossy(sent[0].body.as_ref().unwrap()).contains("k-123"));
    }

    #[test]
    fn a_caller_supplied_id_is_reused_so_a_retry_is_idempotent() {
        let id = "01890a5d-ac96-774b-bcce-b302099a8057";
        let env = Fake::answering(200, "{}");
        let out = create_request(
            &env,
            &wired(json!({ "to": "a@b.c", "request": "x", "id": id })),
        );
        assert_eq!(out["id"], id);
    }

    #[test]
    fn invalid_input_is_a_value_and_sends_nothing() {
        let env = Fake::answering(202, "{}");
        for (extra, code) in [
            (json!({ "request": "x" }), "invalid_recipient"),
            (json!({ "to": "a@b.c" }), "invalid_request"),
            (
                json!({ "to": "a@b.c", "request": "x", "id": "nope" }),
                "invalid_request",
            ),
            (
                json!({ "to": "a@b.c", "request": "x", "context": { "token": "t" } }),
                "invalid_request",
            ),
        ] {
            let out = create_request(&env, &wired(extra));
            assert_eq!(out["ok"], false);
            assert_eq!(out["error"]["code"], code);
        }
        assert!(env.sent.borrow().is_empty());
    }

    #[test]
    fn a_missing_credential_is_named_and_sends_nothing() {
        let mut env = Fake::answering(202, "{}");
        env.secrets.clear();
        let out = create_request(&env, &wired(json!({ "to": "a@b.c", "request": "x" })));
        assert_eq!(out["error"]["code"], "unauthorised");
        assert!(
            out["error"]["message"]
                .as_str()
                .unwrap()
                .contains("done/api_key")
        );
        assert!(env.sent.borrow().is_empty());
    }

    #[test]
    fn refusals_map_to_stable_codes_and_never_echo_the_body() {
        for (status, body, code) in [
            (401, "", "unauthorised"),
            (429, "", "rate_limited"),
            (500, "boom secret-text", "unavailable"),
            (403, r#"{"error":{"code":"blocked"}}"#, "blocked"),
            (400, r#"{"code":"invalid_recipient"}"#, "invalid_recipient"),
            (403, r#"{"error":{"code":"made_up"}}"#, "unauthorised"),
        ] {
            let env = Fake::answering(status, body);
            let out = create_request(&env, &wired(json!({ "to": "a@b.c", "request": "x" })));
            assert_eq!(out["ok"], false);
            assert_eq!(out["error"]["code"], code, "status {status}");
            assert!(!out.to_string().contains("secret-text"));
        }
    }

    #[test]
    fn a_transport_failure_is_unavailable() {
        let env = Fake::new(Err("http send failed: timeout".into()));
        let out = create_request(&env, &wired(json!({ "to": "a@b.c", "request": "x" })));
        assert_eq!(out["error"]["code"], "unavailable");
    }

    fn a2a_reply(text: &str) -> String {
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "message": { "parts": [{ "text": text }] } } }).to_string()
    }

    const ID: &str = "01890a5d-ac96-774b-bcce-b302099a8057";

    #[test]
    fn read_returns_state_and_outcome() {
        let env = Fake::answering(200, &a2a_reply(r#"{"state":"done","outcome":"done"}"#));
        let out = read_request(&env, &wired(json!({ "id": ID })));
        assert_eq!(
            out,
            json!({ "ok": true, "id": ID, "state": "done", "outcome": "done" })
        );
        let sent = env.sent.borrow();
        assert_eq!(sent[0].url, "https://core.example.com/a2a");
        let body: Value = serde_json::from_slice(sent[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(body["method"], "SendMessage");
    }

    #[test]
    fn read_reports_not_found_and_unusable_answers_without_inventing_a_state() {
        for (reply, code) in [
            (a2a_reply(r#"{"found":false}"#), "not_found"),
            (a2a_reply("not json"), "unavailable"),
            (a2a_reply(r#"{"hello":1}"#), "unavailable"),
            (
                json!({ "result": { "task": { "status": { "state": "TASK_STATE_WORKING" } } } })
                    .to_string(),
                "unavailable",
            ),
            (
                json!({ "error": { "code": -32001 } }).to_string(),
                "unavailable",
            ),
        ] {
            let env = Fake::answering(200, &reply);
            let out = read_request(&env, &wired(json!({ "id": ID })));
            assert_eq!(out["ok"], false);
            assert_eq!(out["error"]["code"], code, "{reply}");
        }
    }

    #[test]
    fn read_validates_the_id_before_any_call() {
        let env = Fake::answering(200, "{}");
        assert_eq!(
            read_request(&env, &wired(json!({})))["error"]["code"],
            "invalid_request"
        );
        assert_eq!(
            read_request(&env, &wired(json!({ "id": "x" })))["error"]["code"],
            "invalid_request"
        );
        assert!(env.sent.borrow().is_empty());
    }
}
