//! Channel choice, card rendering and notice wording for `done.deliver` and
//! `done.notify` (`docs/done/flows/`, `docs/done/channels/`).
//!
//! - [`pick_channel`]: Teams, then Webex, then email; one place for "has a usable
//!   address" and for the preference order.
//! - [`render_card`]: the canonical card (`canonical-card.json`,
//!   `permission-prompt-card.json`) as a Teams / Webex Adaptive Card 1.4 or an
//!   email body. Every button carries an HMAC the inbound flow recomputes.
//! - [`notify_text`]: the plain-text wording a requester is told.
//!
//! Keys arrive as secret references only (`secret:done/action_key`,
//! `secret:done/link_key`); a literal is refused. Failure is a value.

use serde_json::{Value, json};

use crate::env::Env;
use crate::mac;
use crate::ops::{err, str_field};
use crate::token;
use crate::uuid7;

/// Preference order, first usable wins.
pub const CHANNELS: &[&str] = &["teams", "webex", "email"];
/// Optional secret holding the public base URL of the confirmation page host
/// (`https://<base>/done/confirm.html#t=<token>`), used when `public_base_url`
/// is not passed. A URL is not a secret, but the deployment keeps it beside the
/// keys. Rename here only.
pub const PUBLIC_BASE_SECRET: &str = "secret:done/public_base_url";
/// Path of the static confirmation page under the public base.
pub const CONFIRM_PATH: &str = "/done/confirm.html";
/// Longest request text a card shows; the rest is cut, never dropped silently.
pub const MAX_CARD_TEXT_CHARS: usize = 2000;

/// A usable address is non-blank; an email one also needs an `@` with text on
/// both sides, no whitespace.
fn usable(channel: &str, addr: &str) -> bool {
    let a = addr.trim();
    if a.is_empty() || a.chars().any(char::is_control) {
        return false;
    }
    match channel {
        "email" => {
            !a.contains(char::is_whitespace)
                && a.split_once('@')
                    .is_some_and(|(l, d)| !l.is_empty() && !d.is_empty() && !d.contains('@'))
        }
        _ => true,
    }
}

/// `pick_channel`. In: `teams_address`, `webex_address`, `email_address`,
/// `exclude` (a channel name, or empty). Out: `{ok, channel, address}` with
/// `channel` = `teams|webex|email|none` (`none` is a normal answer, not an error).
pub fn pick_channel(input: &Value) -> Value {
    let exclude = str_field(input, "exclude").unwrap_or("");
    if !exclude.is_empty() && !CHANNELS.contains(&exclude) {
        return err(
            "invalid_request",
            "`exclude` must be teams, webex, email or empty",
        );
    }
    for channel in CHANNELS {
        if *channel == exclude {
            continue;
        }
        let key = format!("{channel}_address");
        if let Some(addr) = str_field(input, &key).filter(|a| usable(channel, a)) {
            return json!({ "ok": true, "channel": channel, "address": addr });
        }
    }
    json!({ "ok": true, "channel": "none", "address": "" })
}

// ---------------------------------------------------------------- card model

struct Action {
    id: &'static str,
    label: &'static str,
    style: &'static str,
    from: &'static [&'static str],
    asks_date: bool,
}

/// Canonical request actions (`canonical-card.json`, transition table).
const REQUEST_ACTIONS: &[Action] = &[
    Action {
        id: "accept",
        label: "Accept",
        style: "positive",
        from: &["submitted"],
        asks_date: false,
    },
    Action {
        id: "done",
        label: "Done",
        style: "positive",
        from: &["accepted", "overdue", "escalated"],
        asks_date: false,
    },
    Action {
        id: "more_time",
        label: "Need more time",
        style: "default",
        from: &["accepted"],
        asks_date: true,
    },
    Action {
        id: "cant_do",
        label: "Can't do",
        style: "destructive",
        from: &["submitted"],
        asks_date: false,
    },
];

/// Permission prompt actions (`permission-prompt-card.json`).
const PROMPT_ACTIONS: &[Action] = &[
    Action {
        id: "allow_sender",
        label: "Allow",
        style: "positive",
        from: &["permission_pending"],
        asks_date: false,
    },
    Action {
        id: "deny_sender",
        label: "Don't allow",
        style: "destructive",
        from: &["permission_pending"],
        asks_date: false,
    },
];

fn state_label(state: &str) -> String {
    match state {
        "submitted" => "Waiting for you".to_string(),
        "permission_pending" => "Waiting for your permission".to_string(),
        "accepted" => "Accepted".to_string(),
        "overdue" => "Overdue".to_string(),
        "escalated" => "Escalated".to_string(),
        other => other.replace('_', " "),
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

fn escape_html(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            other => o.push(other),
        }
    }
    o
}

/// `hex(HMAC-SHA256(action_key, "v1\n" + done_id + "\n" + action + "\n" + recipient))`.
pub fn action_sig(key: &[u8], done_id: &str, action: &str, recipient: &str) -> String {
    let msg = format!("v1\n{done_id}\n{action}\n{recipient}");
    mac::hex(&mac::hmac_sha256(key, msg.as_bytes()))
}

struct Ctx<'a> {
    kind: &'a str,
    done_id: &'a str,
    state: &'a str,
    requester: &'a str,
    text: String,
    due_label: String,
    recipient: &'a str,
}

fn title(c: &Ctx<'_>) -> String {
    if c.kind == "permission_prompt" {
        format!("{} wants to send you requests", c.requester)
    } else {
        format!("{} asked you for something", c.requester)
    }
}

fn body_text(c: &Ctx<'_>) -> String {
    if c.kind == "permission_prompt" {
        format!("Allow {} to send you requests through Done?", c.requester)
    } else {
        c.text.clone()
    }
}

fn actions_for<'a>(c: &Ctx<'_>) -> Vec<&'a Action> {
    let table: &'a [Action] = if c.kind == "permission_prompt" {
        PROMPT_ACTIONS
    } else {
        REQUEST_ACTIONS
    };
    table.iter().filter(|a| a.from.contains(&c.state)).collect()
}

/// Teams / Webex Adaptive Card 1.4 (`teams-adaptive-card.json`,
/// `teams-renderer.md`): one `Action.Submit` per legal action, `data` exactly
/// `{cardId, done_id, action, sig}`, `Input.Date` only when more_time is offered.
fn adaptive_card(c: &Ctx<'_>, action_key: &[u8]) -> Value {
    let acts = actions_for(c);
    let mut body = vec![
        json!({ "type": "TextBlock", "text": title(c), "weight": "Bolder", "size": "Medium", "wrap": true }),
        json!({ "type": "TextBlock", "text": body_text(c), "wrap": true }),
    ];
    if c.kind != "permission_prompt" {
        body.insert(
            1,
            json!({ "type": "TextBlock", "text": format!("From {}", c.requester),
                     "isSubtle": true, "spacing": "None", "wrap": true }),
        );
        body.push(json!({ "type": "FactSet", "facts": [
            { "title": "Due", "value": c.due_label },
            { "title": "Status", "value": state_label(c.state) },
        ]}));
    }
    if acts.iter().any(|a| a.asks_date) {
        body.push(
            json!({ "type": "Input.Date", "id": "new_due", "isRequired": false,
                          "label": "Proposed new deadline (only for Need more time)" }),
        );
    }
    let actions: Vec<Value> = acts
        .iter()
        .map(|a| {
            let mut v = json!({
                "type": "Action.Submit",
                "title": a.label,
                "data": {
                    "cardId": "done.action",
                    "done_id": c.done_id,
                    "action": a.id,
                    "sig": action_sig(action_key, c.done_id, a.id, c.recipient),
                },
            });
            if a.style != "default" {
                v["style"] = json!(a.style);
            }
            v
        })
        .collect();
    json!({
        "type": "AdaptiveCard",
        "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
        "version": "1.4",
        "body": body,
        "actions": actions,
    })
}

/// Email body: one link per legal action to the static confirmation page. The
/// token is in the fragment (`email-link-format.md`). All third-party text is
/// escaped; the links are built from base64url and a configured base only.
fn email_html(c: &Ctx<'_>, links: &[(&Action, String)]) -> String {
    let mut h = String::new();
    h.push_str("<div style=\"font-family:sans-serif;max-width:560px\">");
    h.push_str(&format!("<h2>{}</h2>", escape_html(&title(c))));
    if c.kind != "permission_prompt" {
        h.push_str(&format!(
            "<p><small>From {}</small></p>",
            escape_html(c.requester)
        ));
    }
    h.push_str(&format!(
        "<p>{}</p>",
        escape_html(&body_text(c)).replace('\n', "<br>")
    ));
    if c.kind != "permission_prompt" {
        h.push_str(&format!(
            "<p><b>Due:</b> {}<br><b>Status:</b> {}</p>",
            escape_html(&c.due_label),
            escape_html(&state_label(c.state))
        ));
    }
    h.push_str("<p>");
    for (a, url) in links {
        h.push_str(&format!(
            "<a href=\"{}\" style=\"display:inline-block;margin:4px 8px 4px 0;padding:10px 16px;border:1px solid #888;border-radius:4px;text-decoration:none\">{}</a>",
            escape_html(url),
            escape_html(a.label)
        ));
    }
    h.push_str("</p><p><small>Each button opens a page where you confirm. The links expire in 72 hours and work once.</small></p></div>");
    h
}

fn secret_key(env: &dyn Env, input: &Value, field: &str, name: &str) -> Result<Vec<u8>, Value> {
    let Some(raw) = str_field(input, field) else {
        return Err(err(
            "unauthorised",
            format!("missing `{field}` (wire it to the secret {name})"),
        ));
    };
    mac::secret_by_name(env, raw)
        .map(String::into_bytes)
        .map_err(|e| err("unauthorised", e))
}

fn public_base(env: &dyn Env, input: &Value) -> Result<String, Value> {
    let raw = str_field(input, "public_base_url").unwrap_or(PUBLIC_BASE_SECRET);
    let url = crate::env::resolve(env, raw).map_err(|e| err("invalid_request", e))?;
    let url = url.trim().trim_end_matches('/').to_string();
    if !url.starts_with("https://") || url.contains(char::is_whitespace) || url.contains(['#', '?'])
    {
        return Err(err(
            "invalid_request",
            "`public_base_url` must be an https URL without query or fragment",
        ));
    }
    Ok(url)
}

/// `render_card`. In: `kind` (`request|permission_prompt`), `channel`, `done_id`,
/// `state`, `requester_name`, `text`, `due`, `recipient` (Person.id),
/// `action_key`, `link_key` (secret refs), optional `public_base_url` (email).
/// Out: `{ok, payload, subject, content_type, actions}`; `payload` is an
/// Adaptive Card object for teams/webex, an HTML string for email.
pub fn render_card(env: &dyn Env, input: &Value) -> Value {
    let kind = str_field(input, "kind").unwrap_or("");
    if kind != "request" && kind != "permission_prompt" {
        return err(
            "invalid_request",
            "`kind` must be request or permission_prompt",
        );
    }
    let channel = str_field(input, "channel").unwrap_or("");
    if !CHANNELS.contains(&channel) {
        return err("invalid_request", "`channel` must be teams, webex or email");
    }
    let Some(done_id) = str_field(input, "done_id").filter(|d| uuid7::is_well_formed(d)) else {
        return err("invalid_request", "`done_id` must be a request id");
    };
    let Some(recipient) = str_field(input, "recipient").filter(|r| !r.contains('\n')) else {
        return err("invalid_recipient", "`recipient` (a Person id) is required");
    };
    let Some(state) = str_field(input, "state") else {
        return err("invalid_request", "`state` is required");
    };
    let requester = str_field(input, "requester_name").unwrap_or("Someone");
    let c = Ctx {
        kind,
        done_id,
        state,
        requester,
        text: clip(str_field(input, "text").unwrap_or(""), MAX_CARD_TEXT_CHARS),
        due_label: str_field(input, "due").unwrap_or("No due date").to_string(),
        recipient,
    };
    let acts = actions_for(&c);
    if acts.is_empty() {
        return err(
            "invalid_request",
            format!("no action is offered in state `{state}`"),
        );
    }
    let action_key = match secret_key(env, input, "action_key", "done/action_key") {
        Ok(k) => k,
        Err(e) => return e,
    };
    let names: Vec<&str> = acts.iter().map(|a| a.id).collect();
    if channel != "email" {
        return json!({ "ok": true, "payload": adaptive_card(&c, &action_key),
                       "subject": title(&c), "content_type": "application/vnd.microsoft.card.adaptive",
                       "actions": names });
    }
    let link_key = match secret_key(env, input, "link_key", "done/link_key") {
        Ok(k) => k,
        Err(e) => return e,
    };
    let base = match public_base(env, input) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let expires = (env.now_ms() / 1000).saturating_add(token::DEFAULT_TTL_S);
    let mut links = Vec::new();
    for a in &acts {
        // One fresh nonce per link: 16 bytes from two entropy reads.
        let (r1, r2) = (env.random(), env.random());
        let mut nonce = [0u8; 16];
        nonce[..10].copy_from_slice(&r1);
        nonce[10..].copy_from_slice(&r2[..6]);
        let n = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, nonce);
        let Some(t) = token::issue(&link_key, done_id, a.id, recipient, expires, &n) else {
            return err("invalid_request", "could not sign a link for these fields");
        };
        links.push((*a, format!("{base}{CONFIRM_PATH}#t={t}")));
    }
    json!({ "ok": true, "payload": email_html(&c, &links), "subject": title(&c),
            "content_type": "text/html", "actions": names })
}

// ------------------------------------------------------------------- notices

/// `notify_text`. In: `event_type`, `request_text`, `due`, `recipient_name`.
/// Out: `{ok, send: "true"|"false", text}`. An event with no wording sends nothing.
pub fn notify_text(input: &Value) -> Value {
    let who = str_field(input, "recipient_name").unwrap_or("The person you asked");
    let what = clip(str_field(input, "request_text").unwrap_or(""), 200);
    let line = match str_field(input, "event_type").unwrap_or("") {
        "done.accepted" => format!("{who} accepted your request: \"{what}\"."),
        "done.completed" => format!("{who} marked your request as done: \"{what}\"."),
        "done.extension_requested" => {
            let due = str_field(input, "due")
                .map(|d| format!(" The current deadline is {d}."))
                .unwrap_or_default();
            format!("{who} asked for more time on your request: \"{what}\".{due}")
        }
        "done.rejected" => format!("{who} can't do your request: \"{what}\"."),
        _ => return json!({ "ok": true, "send": "false", "text": "" }),
    };
    json!({ "ok": true, "send": "true", "text": line })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{HttpReq, Response};

    const ID: &str = "01890a5d-ac96-774b-bcce-b302099a8057";

    struct Keys;
    impl Env for Keys {
        fn secret(&self, name: &str) -> Result<Option<String>, String> {
            Ok(match name {
                "done/action_key" => Some("action-key".into()),
                "done/link_key" => Some("link-key".into()),
                "done/public_base_url" => Some("https://done.example.com/".into()),
                _ => None,
            })
        }
        fn send(&self, _: HttpReq) -> Result<Response, String> {
            Err("no network".into())
        }
        fn now_ms(&self) -> u64 {
            1_000_000
        }
        fn random(&self) -> [u8; 10] {
            [7; 10]
        }
    }

    fn input(kind: &str, channel: &str, state: &str) -> Value {
        json!({
            "kind": kind, "channel": channel, "done_id": ID, "state": state,
            "requester_name": "Maarten", "text": "Send the <b>report</b>", "due": "2026-11-05",
            "recipient": "person-1",
            "action_key": "secret:done/action_key", "link_key": "secret:done/link_key",
        })
    }

    #[test]
    fn channel_order_is_teams_then_webex_then_email() {
        let all =
            json!({"teams_address":"t","webex_address":"w","email_address":"a@b.c","exclude":""});
        assert_eq!(pick_channel(&all)["channel"], "teams");
        let no_teams = json!({"teams_address":" ","webex_address":"w","email_address":"a@b.c"});
        assert_eq!(pick_channel(&no_teams)["channel"], "webex");
        let only_mail = json!({"email_address":"a@b.c"});
        let out = pick_channel(&only_mail);
        assert_eq!(
            (out["channel"].as_str(), out["address"].as_str()),
            (Some("email"), Some("a@b.c"))
        );
    }

    #[test]
    fn exclude_skips_a_channel_and_none_is_a_value() {
        let all = json!({"teams_address":"t","webex_address":"w","email_address":"a@b.c","exclude":"teams"});
        assert_eq!(pick_channel(&all)["channel"], "webex");
        let one = json!({"teams_address":"t","exclude":"teams"});
        let out = pick_channel(&one);
        assert_eq!(out["ok"], true);
        assert_eq!(out["channel"], "none");
        assert_eq!(pick_channel(&json!({}))["channel"], "none");
        assert_eq!(pick_channel(&json!({"exclude":"sms"}))["ok"], false);
    }

    #[test]
    fn a_malformed_email_is_not_usable() {
        for bad in ["nope", "@b.c", "a@", "a b@c.d", "a@b@c"] {
            assert_eq!(
                pick_channel(&json!({"email_address": bad}))["channel"],
                "none",
                "{bad}"
            );
        }
    }

    #[test]
    fn the_sig_formula_is_pinned() {
        // hex(HMAC-SHA256(key, "v1\n<id>\n<action>\n<recipient>")).
        let msg = format!("v1\n{ID}\naccept\nperson-1");
        let expect = mac::hex(&mac::hmac_sha256(b"action-key", msg.as_bytes()));
        assert_eq!(action_sig(b"action-key", ID, "accept", "person-1"), expect);
        assert_eq!(expect.len(), 64);
        assert_ne!(expect, action_sig(b"action-key", ID, "accept", "person-2"));
    }

    #[test]
    fn a_submitted_request_card_offers_accept_and_cant_do_with_signed_data() {
        let out = render_card(&Keys, &input("request", "teams", "submitted"));
        assert_eq!(out["ok"], true);
        let card = &out["payload"];
        assert_eq!(card["version"], "1.4");
        let actions = card["actions"].as_array().unwrap();
        let ids: Vec<&str> = actions
            .iter()
            .map(|a| a["data"]["action"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["accept", "cant_do"]);
        for a in actions {
            let d = a["data"].as_object().unwrap();
            assert_eq!(d.len(), 4);
            assert_eq!(d["cardId"], "done.action");
            assert_eq!(d["done_id"], ID);
            assert_eq!(
                d["sig"],
                action_sig(b"action-key", ID, d["action"].as_str().unwrap(), "person-1")
            );
        }
        assert!(!card.to_string().contains("Input.Date"));
    }

    #[test]
    fn more_time_appears_only_from_accepted_and_brings_the_date_input() {
        let card = &render_card(&Keys, &input("request", "webex", "accepted"))["payload"];
        let ids: Vec<&str> = card["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["data"]["action"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["done", "more_time"]);
        assert!(card.to_string().contains("\"Input.Date\""));
    }

    #[test]
    fn the_permission_prompt_has_allow_and_deny_and_no_request_facts() {
        let card = &render_card(
            &Keys,
            &input("permission_prompt", "teams", "permission_pending"),
        )["payload"];
        let ids: Vec<&str> = card["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["data"]["action"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["allow_sender", "deny_sender"]);
        assert!(!card.to_string().contains("FactSet"));
        assert!(
            card.to_string()
                .contains("Maarten wants to send you requests")
        );
    }

    #[test]
    fn a_terminal_state_or_wrong_pair_renders_nothing() {
        assert_eq!(
            render_card(&Keys, &input("request", "teams", "done"))["error"]["code"],
            "invalid_request"
        );
        assert_eq!(
            render_card(&Keys, &input("permission_prompt", "teams", "submitted"))["ok"],
            false
        );
    }

    #[test]
    fn the_email_body_links_verify_and_escape_third_party_text() {
        let out = render_card(&Keys, &input("request", "email", "submitted"));
        assert_eq!(out["ok"], true);
        let html = out["payload"].as_str().unwrap();
        assert!(html.contains("&lt;b&gt;report&lt;/b&gt;"));
        assert!(!html.contains("<b>report"));
        let marker = "https://done.example.com/done/confirm.html#t=";
        let mut seen = Vec::new();
        for part in html.split("href=\"").skip(1) {
            let url = part.split('"').next().unwrap();
            let tok = url.strip_prefix(marker).expect("base + path + fragment");
            let link = token::check(b"link-key", tok, 1_000).expect("verifies");
            assert_eq!(
                (link.done_id.as_str(), link.recipient.as_str()),
                (ID, "person-1")
            );
            assert_eq!(link.expires_at, 1_000 + token::DEFAULT_TTL_S);
            seen.push(link.action);
        }
        assert_eq!(seen, ["accept", "cant_do"]);
        assert_eq!(out["subject"], "Maarten asked you for something");
    }

    #[test]
    fn keys_must_be_secret_references_and_never_leak() {
        let mut i = input("request", "teams", "submitted");
        i["action_key"] = json!("action-key");
        let out = render_card(&Keys, &i);
        assert_eq!(out["error"]["code"], "unauthorised");
        assert!(!out.to_string().contains("action-key\""));
        let mut e = input("request", "email", "submitted");
        e["link_key"] = json!("link-key");
        assert_eq!(render_card(&Keys, &e)["error"]["code"], "unauthorised");
        let mut missing = input("request", "teams", "submitted");
        missing.as_object_mut().unwrap().remove("action_key");
        assert_eq!(
            render_card(&Keys, &missing)["error"]["code"],
            "unauthorised"
        );
    }

    #[test]
    fn bad_input_is_a_value() {
        for (k, v) in [
            ("kind", "x"),
            ("channel", "sms"),
            ("done_id", "nope"),
            ("state", ""),
        ] {
            let mut i = input("request", "teams", "submitted");
            i[k] = json!(v);
            assert_eq!(render_card(&Keys, &i)["ok"], false, "{k}");
        }
        let mut http = input("request", "email", "submitted");
        http["public_base_url"] = json!("http://insecure.example");
        assert_eq!(
            render_card(&Keys, &http)["error"]["code"],
            "invalid_request"
        );
    }

    #[test]
    fn notice_wording_per_event_and_silence_for_the_rest() {
        let base = json!({"request_text":"Pay it","due":"2026-11-05","recipient_name":"Alice"});
        let mk = |e: &str| {
            let mut i = base.clone();
            i["event_type"] = json!(e);
            notify_text(&i)
        };
        assert_eq!(
            mk("done.accepted")["text"],
            "Alice accepted your request: \"Pay it\"."
        );
        assert!(
            mk("done.extension_requested")["text"]
                .as_str()
                .unwrap()
                .contains("2026-11-05")
        );
        assert_eq!(mk("done.completed")["send"], "true");
        assert_eq!(mk("done.rejected")["send"], "true");
        let none = mk("done.something_else");
        assert_eq!(
            (none["send"].as_str(), none["text"].as_str()),
            (Some("false"), Some(""))
        );
        assert_eq!(notify_text(&json!({}))["send"], "false");
    }
}
