//! Reminder operations: `plan_reminder` (pure policy), `fanout_reminders` and
//! `deliver_reminder` (calls to the Core). Specs: `docs/done/flows-aux/README.md`
//! and decision D8.
//!
//! The policy is one [`Policy`] value. Changing the numbers is editing
//! [`Policy::DEFAULT`], nothing else.

use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};

use crate::env::Env;
use crate::ops::{classify, creds, err, post, str_field};

/// The Core trigger that handles ONE reminder (`done_remind_one` in the aux flows).
pub const REMIND_ONE_TRIGGER: &str = "done_remind_one";
/// The Core trigger that sends a reminder through the recipient's channel.
/// ASSUMPTION: the aux README leaves proactive send open; this is the name this
/// component posts to until the channel flow exists.
pub const DELIVER_TRIGGER: &str = "done_deliver_reminder";
/// A tick never fans out more than this many reminders (the SoR limit is 50).
pub const MAX_FANOUT: usize = 200;

/// Request states that must not be reminded: finished, not yet permitted, or
/// waiting on the requester rather than the recipient.
const NOT_REMINDABLE: &[&str] = &[
    "done",
    "rejected",
    "cant_do",
    "cancelled",
    "expired",
    "blocked",
    "permission_pending",
    "extension_requested",
];

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Quiet window start, recipient-local hour (inclusive).
    pub quiet_start_hour: u32,
    /// Quiet window end, recipient-local hour (exclusive).
    pub quiet_end_hour: u32,
    /// Interval after the first reminder; doubles each time.
    pub first_interval_hours: i64,
    pub max_interval_hours: i64,
    /// Reminders per request, in total.
    pub max_reminders: u32,
}

impl Policy {
    pub const DEFAULT: Policy = Policy {
        quiet_start_hour: 22,
        quiet_end_hour: 8,
        first_interval_hours: 24,
        max_interval_hours: 72,
        max_reminders: 5,
    };

    fn is_quiet(&self, hour: u32) -> bool {
        hour >= self.quiet_start_hour || hour < self.quiet_end_hour
    }

    /// Wait after the `sent`-th reminder (1-based): 24h, 48h, 72h, 72h, ...
    fn interval_after(&self, sent: u32) -> Duration {
        let doublings = sent.saturating_sub(1).min(16);
        let hours = self
            .first_interval_hours
            .saturating_mul(1i64 << doublings)
            .min(self.max_interval_hours);
        Duration::hours(hours)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub send: bool,
    pub new_count: u32,
    /// `None`: nothing further is scheduled for this request.
    pub next_at: Option<DateTime<Utc>>,
    pub reason: &'static str,
}

/// Decide what to do with one due reminder. Pure.
///
/// `count` is how many reminders were already sent. A skipped tick (`send:false`)
/// keeps `count`, so the `(request, count)` pair stays unclaimed.
pub fn plan(
    policy: &Policy,
    now: DateTime<Utc>,
    tz: Tz,
    count: u32,
    state: &str,
) -> Result<Plan, String> {
    if NOT_REMINDABLE.contains(&state) {
        return Ok(Plan {
            send: false,
            new_count: count,
            next_at: None,
            reason: "not_remindable_state",
        });
    }
    if count >= policy.max_reminders {
        return Ok(Plan {
            send: false,
            new_count: count,
            next_at: None,
            reason: "max_reached",
        });
    }
    let local = now.with_timezone(&tz);
    if policy.is_quiet(local.hour()) {
        let date: NaiveDate = if local.hour() >= policy.quiet_start_hour {
            local
                .date_naive()
                .succ_opt()
                .ok_or_else(|| "date out of range".to_string())?
        } else {
            local.date_naive()
        };
        let wake = date
            .and_hms_opt(policy.quiet_end_hour, 0, 0)
            .ok_or_else(|| "invalid quiet hours".to_string())?;
        // `earliest` picks the first instant when a DST fold makes 08:00 occur
        // twice; a skipped (non-existent) local time falls to the next hour.
        let end = tz
            .from_local_datetime(&wake)
            .earliest()
            .or_else(|| {
                tz.from_local_datetime(&(wake + Duration::hours(1)))
                    .earliest()
            })
            .ok_or_else(|| "cannot resolve the end of quiet hours".to_string())?;
        return Ok(Plan {
            send: false,
            new_count: count,
            next_at: Some(end.with_timezone(&Utc)),
            reason: "quiet_hours",
        });
    }
    let new_count = count + 1;
    let next_at =
        (new_count < policy.max_reminders).then(|| now + policy.interval_after(new_count));
    Ok(Plan {
        send: true,
        new_count,
        next_at,
        reason: "send",
    })
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// A number from a template: the runner renders `{{x}}` as a string.
fn uint(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `plan_reminder`: `{now?, tz, count, state, due?}` ->
/// `{ok, send:"true"|"false", new_count, next_at|null, reason}`.
///
/// `send` is a string because the flow routes on it (`send == "false"`).
pub fn plan_reminder(env: &dyn Env, input: &Value) -> Value {
    let now = match str_field(input, "now") {
        Some(s) => match DateTime::parse_from_rfc3339(s) {
            Ok(t) => t.with_timezone(&Utc),
            Err(_) => return err("invalid_request", "`now` must be an RFC 3339 timestamp"),
        },
        None => match i64::try_from(env.now_ms())
            .ok()
            .and_then(DateTime::from_timestamp_millis)
        {
            Some(t) => t,
            None => return err("unavailable", "the host clock is out of range"),
        },
    };
    let Some(tz_name) = str_field(input, "tz") else {
        return err("invalid_request", "`tz` (an IANA time zone) is required");
    };
    let Ok(tz) = tz_name.parse::<Tz>() else {
        return err("invalid_request", "`tz` is not a known IANA time zone");
    };
    let Some(count) = input
        .get("count")
        .and_then(uint)
        .and_then(|c| u32::try_from(c).ok())
    else {
        return err("invalid_request", "`count` must be a non-negative integer");
    };
    let Some(state) = str_field(input, "state") else {
        return err("invalid_request", "`state` is required");
    };
    match plan(&Policy::DEFAULT, now, tz, count, state) {
        Ok(p) => json!({
            "ok": true,
            "send": if p.send { "true" } else { "false" },
            "new_count": p.new_count,
            "next_at": p.next_at.map(rfc3339),
            "reason": p.reason,
        }),
        Err(e) => err("unavailable", e),
    }
}

/// `fanout_reminders`: post one `done_remind_one` firing per due reminder.
///
/// The idempotency key is `<request id>:<count>` (the trigger sets
/// `idempotency.key: body.key`), so a tick that overlaps the previous one cannot
/// double-send. A failed post is counted and named, never fatal to the others.
pub fn fanout_reminders(env: &dyn Env, input: &Value) -> Value {
    let creds = match creds(env, input) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(items) = input.get("items").and_then(Value::as_array) else {
        return err("invalid_request", "`items` must be an array");
    };
    if items.len() > MAX_FANOUT {
        return err(
            "invalid_request",
            format!("`items` is over {MAX_FANOUT} reminders"),
        );
    }
    let tz = str_field(input, "tz").unwrap_or("UTC");
    let url = format!("{}/trigger/{REMIND_ONE_TRIGGER}", creds.base);
    let mut posted = 0u32;
    let mut failures = Vec::new();
    for item in items {
        let id = str_field(item, "request_id");
        let count = item.get("count").and_then(uint);
        let (Some(id), Some(count)) = (id, count) else {
            failures.push(json!({ "request_id": id, "code": "invalid_request" }));
            continue;
        };
        let body = json!({
            "request_id": id,
            "count": count,
            "key": format!("{id}:{count}"),
            "tz": tz,
        });
        match post(env, url.clone(), &creds.api_key, &body) {
            Ok(resp) if (200..300).contains(&resp.status) => posted += 1,
            Ok(resp) => failures.push(json!({
                "request_id": id,
                "code": classify(&resp)["error"]["code"],
            })),
            Err(e) => failures.push(json!({
                "request_id": id,
                "code": e["error"]["code"],
            })),
        }
    }
    json!({ "ok": true, "posted": posted, "failed": failures.len(), "failures": failures })
}

/// `deliver_reminder`: ask the Core to send the canonical card to the recipient.
pub fn deliver_reminder(env: &dyn Env, input: &Value) -> Value {
    let creds = match creds(env, input) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(request_id) = str_field(input, "request_id") else {
        return err("invalid_request", "`request_id` is required");
    };
    let Some(recipient) = str_field(input, "recipient") else {
        return err("invalid_recipient", "`recipient` is required");
    };
    let Some(text) = str_field(input, "text") else {
        return err("invalid_request", "`text` is required");
    };
    let mut body = json!({
        "kind": "reminder",
        "request_id": request_id,
        "recipient": recipient,
        "text": text,
    });
    if let Some(due) = str_field(input, "due") {
        body["due"] = json!(due);
    }
    let url = format!("{}/trigger/{DELIVER_TRIGGER}", creds.base);
    match post(env, url, &creds.api_key, &body) {
        Ok(resp) if (200..300).contains(&resp.status) => {
            json!({ "ok": true, "request_id": request_id })
        }
        Ok(resp) => classify(&resp),
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{HttpReq, Response};
    use std::cell::RefCell;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn run(now: &str, tz: &str, count: u32, state: &str) -> Plan {
        plan(&Policy::DEFAULT, at(now), tz.parse().unwrap(), count, state).unwrap()
    }

    #[test]
    fn midday_sends_and_the_interval_doubles_to_the_cap() {
        let now = "2026-11-02T12:00:00Z";
        let expected = [(0, 1, 24), (1, 2, 48), (2, 3, 72), (3, 4, 72)];
        for (count, new_count, hours) in expected {
            let p = run(now, "UTC", count, "submitted");
            assert!(p.send, "count {count}");
            assert_eq!(p.new_count, new_count);
            assert_eq!(
                p.next_at,
                Some(at(now) + Duration::hours(hours)),
                "count {count}"
            );
        }
    }

    #[test]
    fn the_fifth_reminder_is_the_last_and_schedules_nothing() {
        let p = run("2026-11-02T12:00:00Z", "UTC", 4, "accepted");
        assert!(p.send);
        assert_eq!(p.new_count, 5);
        assert_eq!(p.next_at, None);
        let p = run("2026-11-02T12:00:00Z", "UTC", 5, "accepted");
        assert!(!p.send);
        assert_eq!(p.reason, "max_reached");
        assert_eq!(p.new_count, 5);
    }

    #[test]
    fn quiet_hours_are_recipient_local_and_move_next_at_to_eight() {
        // 21:59 local: not quiet. 22:00 local: quiet until 08:00 next day.
        assert!(run("2026-11-02T21:59:00Z", "UTC", 0, "submitted").send);
        let p = run("2026-11-02T22:00:00Z", "UTC", 1, "submitted");
        assert!(!p.send);
        assert_eq!(p.new_count, 1, "a quiet tick does not advance the count");
        assert_eq!(p.next_at, Some(at("2026-11-03T08:00:00Z")));
        // 07:59 local: still quiet, wakes the same day at 08:00.
        let p = run("2026-11-03T07:59:00Z", "UTC", 0, "submitted");
        assert!(!p.send);
        assert_eq!(p.next_at, Some(at("2026-11-03T08:00:00Z")));
        assert!(run("2026-11-03T08:00:00Z", "UTC", 0, "submitted").send);

        // Jakarta is UTC+7: 16:00Z is 23:00 local, quiet until 08:00 WIB = 01:00Z.
        let p = run("2026-11-02T16:00:00Z", "Asia/Jakarta", 0, "submitted");
        assert!(!p.send);
        assert_eq!(p.next_at, Some(at("2026-11-03T01:00:00Z")));
        // The same instant is midday in UTC: sent.
        assert!(run("2026-11-02T16:00:00Z", "UTC", 0, "submitted").send);
    }

    #[test]
    fn quiet_hours_follow_daylight_saving() {
        // New York is UTC-5 after the 2026-11-01 change: 03:30Z on 11-02 is 22:30 EST.
        let p = run("2026-11-02T03:30:00Z", "America/New_York", 0, "submitted");
        assert!(!p.send);
        assert_eq!(p.next_at, Some(at("2026-11-02T13:00:00Z")));
        // Before the change (EDT, UTC-4) the same wall time is an hour earlier in UTC.
        let p = run("2026-10-20T02:30:00Z", "America/New_York", 0, "submitted");
        assert_eq!(p.next_at, Some(at("2026-10-20T12:00:00Z")));
    }

    #[test]
    fn finished_or_waiting_requests_are_never_reminded() {
        for state in [
            "done",
            "cancelled",
            "expired",
            "permission_pending",
            "extension_requested",
        ] {
            let p = run("2026-11-02T12:00:00Z", "UTC", 0, state);
            assert!(!p.send, "{state}");
            assert_eq!(p.next_at, None, "{state}");
        }
    }

    struct Quiet;
    impl Env for Quiet {
        fn secret(&self, _: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn send(&self, _: HttpReq) -> Result<Response, String> {
            Err("no network".into())
        }
        fn now_ms(&self) -> u64 {
            1_788_350_400_000 // 2026-09-02T12:00:00Z
        }
        fn random(&self) -> [u8; 10] {
            [0; 10]
        }
    }

    #[test]
    fn plan_reminder_speaks_the_flow_contract() {
        let out = plan_reminder(
            &Quiet,
            &json!({"now":"2026-11-02T12:00:00Z","tz":"UTC","count":"1","state":"submitted"}),
        );
        assert_eq!(
            out,
            json!({"ok":true,"send":"true","new_count":2,
                   "next_at":"2026-11-04T12:00:00Z","reason":"send"})
        );
        let out = plan_reminder(
            &Quiet,
            &json!({"now":"2026-11-02T23:00:00Z","tz":"UTC","count":1,"state":"submitted"}),
        );
        assert_eq!(out["send"], "false");
        assert_eq!(out["next_at"], "2026-11-03T08:00:00Z");
        let out = plan_reminder(&Quiet, &json!({"tz":"UTC","count":0,"state":"done"}));
        assert_eq!(out["next_at"], Value::Null);
    }

    #[test]
    fn plan_reminder_rejects_bad_input_as_values() {
        for input in [
            json!({}),
            json!({"tz":"Mars/Base","count":0,"state":"submitted"}),
            json!({"tz":"UTC","count":-1,"state":"submitted"}),
            json!({"tz":"UTC","count":"x","state":"submitted"}),
            json!({"tz":"UTC","count":0}),
            json!({"now":"yesterday","tz":"UTC","count":0,"state":"submitted"}),
        ] {
            let out = plan_reminder(&Quiet, &input);
            assert_eq!(out["ok"], false, "{input}");
            assert_eq!(out["error"]["code"], "invalid_request", "{input}");
        }
    }

    struct Core {
        statuses: RefCell<Vec<u16>>,
        sent: RefCell<Vec<HttpReq>>,
    }
    impl Core {
        fn new(statuses: Vec<u16>) -> Self {
            Self {
                statuses: RefCell::new(statuses),
                sent: RefCell::new(Vec::new()),
            }
        }
    }
    impl Env for Core {
        fn secret(&self, name: &str) -> Result<Option<String>, String> {
            Ok(match name {
                "done/api_key" => Some("k-123".into()),
                "done/base_url" => Some("https://core.example.com".into()),
                _ => None,
            })
        }
        fn send(&self, req: HttpReq) -> Result<Response, String> {
            self.sent.borrow_mut().push(req);
            let mut s = self.statuses.borrow_mut();
            let status = if s.is_empty() { 200 } else { s.remove(0) };
            Ok(Response {
                status,
                body: Vec::new(),
            })
        }
        fn now_ms(&self) -> u64 {
            0
        }
        fn random(&self) -> [u8; 10] {
            [0; 10]
        }
    }

    fn wired(extra: Value) -> Value {
        let mut v = json!({"api_key":"secret:done/api_key","base_url":"secret:done/base_url"});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    }

    #[test]
    fn fanout_posts_one_keyed_firing_per_item_and_counts_failures() {
        let env = Core::new(vec![202, 500, 202]);
        let out = fanout_reminders(
            &env,
            &wired(json!({"tz":"Asia/Jakarta","items":[
                {"request_id":"r1","count":0},
                {"request_id":"r2","count":"2"},
                {"request_id":"r3","count":1},
                {"count":1},
            ]})),
        );
        assert_eq!(out["ok"], true);
        assert_eq!(out["posted"], 2);
        assert_eq!(out["failed"], 2);
        assert_eq!(
            out["failures"][0],
            json!({"request_id":"r2","code":"unavailable"})
        );
        assert_eq!(out["failures"][1]["code"], "invalid_request");

        let sent = env.sent.borrow();
        assert_eq!(sent.len(), 3, "an invalid item sends nothing");
        assert_eq!(
            sent[0].url,
            "https://core.example.com/trigger/done_remind_one"
        );
        assert!(
            sent[0]
                .headers
                .contains(&("authorization".into(), "Bearer k-123".into()))
        );
        let body: Value = serde_json::from_slice(sent[1].body.as_ref().unwrap()).unwrap();
        assert_eq!(
            body,
            json!({"request_id":"r2","count":2,"key":"r2:2","tz":"Asia/Jakarta"})
        );
    }

    #[test]
    fn fanout_refuses_without_credentials_or_a_list() {
        assert_eq!(
            fanout_reminders(&Core::new(vec![]), &json!({"items":[]}))["error"]["code"],
            "unauthorised"
        );
        assert_eq!(
            fanout_reminders(&Core::new(vec![]), &wired(json!({})))["error"]["code"],
            "invalid_request"
        );
        let many: Vec<Value> = (0..=MAX_FANOUT)
            .map(|i| json!({"request_id":format!("r{i}"),"count":0}))
            .collect();
        let env = Core::new(vec![]);
        assert_eq!(
            fanout_reminders(&env, &wired(json!({"items":many})))["error"]["code"],
            "invalid_request"
        );
        assert!(env.sent.borrow().is_empty());
    }

    #[test]
    fn deliver_reminder_posts_and_maps_refusals() {
        let env = Core::new(vec![202]);
        let out = deliver_reminder(
            &env,
            &wired(
                json!({"request_id":"r1","recipient":"p1","text":"Approve the PO","due":"2026-11-05"}),
            ),
        );
        assert_eq!(out, json!({"ok":true,"request_id":"r1"}));
        let sent = env.sent.borrow();
        assert_eq!(
            sent[0].url,
            "https://core.example.com/trigger/done_deliver_reminder"
        );
        let body: Value = serde_json::from_slice(sent[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(body["kind"], "reminder");
        assert_eq!(body["due"], "2026-11-05");

        let out = deliver_reminder(
            &Core::new(vec![429]),
            &wired(json!({"request_id":"r","recipient":"p","text":"t"})),
        );
        assert_eq!(out["error"]["code"], "rate_limited");
        for input in [
            json!({"recipient":"p","text":"t"}),
            json!({"request_id":"r","text":"t"}),
            json!({"request_id":"r","recipient":"p"}),
        ] {
            let env = Core::new(vec![]);
            assert_eq!(deliver_reminder(&env, &wired(input))["ok"], false);
            assert!(env.sent.borrow().is_empty());
        }
    }
}
