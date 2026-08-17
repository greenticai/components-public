//! `gcal_events` tool domain — pure HTTP-call building and response
//! normalization for Google Calendar event operations (create/search/get/
//! update/delete/quick_add/respond/move). No WIT imports — this module is
//! fully host-testable; the actual `extension-host/http` invocation and
//! `describe()` tool metadata live in `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext::tools::issues` template: `EventOp`
//! (input enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::client::{HttpCall, Method};

/// Google Calendar event operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOp {
    Create,
    Search,
    Get,
    Update,
    Delete,
    QuickAdd,
    Respond,
    Move,
}

/// Raw `gcal_events` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct EventsInput {
    operation: EventOp,
    #[serde(default)]
    calendar_id: Option<String>,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    destination_calendar_id: Option<String>,
    #[serde(default)]
    attendee_email: Option<String>,
    #[serde(default)]
    response_status: Option<String>,
    #[serde(default)]
    time_min: Option<String>,
    #[serde(default)]
    time_max: Option<String>,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    max_results: Option<u32>,
    #[serde(default)]
    single_events: Option<bool>,
    #[serde(default)]
    order_by: Option<String>,
    #[serde(default)]
    event: Option<Value>,
}

/// Calendar id to address, defaulting to `"primary"` when omitted.
fn calendar_id(input: &EventsInput) -> String {
    input
        .calendar_id
        .clone()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| "primary".to_string())
}

/// Build the Google Calendar REST v3 [`HttpCall`] for a `gcal_events`
/// invocation.
///
/// Parses `args_json` into an [`EventsInput`], validates the fields required
/// by the selected [`EventOp`], and returns the resulting request. On
/// missing input or a missing required field, returns `Err` naming the
/// field.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: EventsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        EventOp::Create => build_create(&input),
        EventOp::Search => build_search(&input),
        EventOp::Get => build_get(&input),
        EventOp::Update => build_update(&input),
        EventOp::Delete => build_delete(&input),
        EventOp::QuickAdd => build_quick_add(&input),
        // `respond` cannot be a single `HttpCall`: Google's `events.patch`
        // replaces the `attendees` array wholesale, so a naive single-PATCH
        // build would wipe every other attendee on a shared event. It is
        // handled as a two-step read-modify-write in the dispatch layer
        // (`lib.rs::invoke_respond`), which never calls this arm — see
        // [`parse_respond_request`], [`get_call`], [`merge_attendee_response`],
        // and [`patch_attendees_call`].
        EventOp::Respond => Err(
            "respond is handled as a read-modify-write in the dispatch layer, not build_call"
                .to_string(),
        ),
        EventOp::Move => build_move(&input),
    }
}

/// Fields required to process a `respond` (RSVP) operation, validated
/// without touching the network. Mirrors the calendar-id default and
/// required-field checks the other `build_*` helpers use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RespondRequest {
    pub calendar_id: String,
    pub event_id: String,
    pub attendee_email: String,
    pub response_status: String,
}

/// Parse and validate a `respond` request from `args_json`, without
/// building any [`HttpCall`]. Returns `Err` naming the first missing
/// required field (`event_id`, `attendee_email`, or `response_status`) —
/// the dispatch layer calls this before doing the GET, so an invalid
/// request never reaches the network.
pub fn parse_respond_request(args_json: &str) -> Result<RespondRequest, String> {
    let input: EventsInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    let event_id = super::require_field(input.event_id.as_deref(), "event_id")?.to_string();
    let attendee_email =
        super::require_field(input.attendee_email.as_deref(), "attendee_email")?.to_string();
    let response_status =
        super::require_field(input.response_status.as_deref(), "response_status")?.to_string();
    Ok(RespondRequest {
        calendar_id: calendar_id(&input),
        event_id,
        attendee_email,
        response_status,
    })
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<EventOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: EventOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

fn build_create(input: &EventsInput) -> Result<HttpCall, String> {
    let event = input
        .event
        .clone()
        .ok_or_else(|| "missing required field: event".to_string())?;
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/calendars/{}/events", calendar_id(input)),
        query: Vec::new(),
        body: Some(event),
    })
}

// `Result` return kept for uniformity with the other `build_*` helpers this
// module's `build_call` dispatch matches on (some of which do fail); every
// `search` filter is optional, so this one never does.
#[allow(clippy::unnecessary_wraps)]
fn build_search(input: &EventsInput) -> Result<HttpCall, String> {
    let mut query = Vec::new();
    if let Some(q) = input.q.as_deref().filter(|v| !v.is_empty()) {
        query.push(("q".to_string(), q.to_string()));
    }
    if let Some(time_min) = input.time_min.as_deref().filter(|v| !v.is_empty()) {
        query.push(("timeMin".to_string(), time_min.to_string()));
    }
    if let Some(time_max) = input.time_max.as_deref().filter(|v| !v.is_empty()) {
        query.push(("timeMax".to_string(), time_max.to_string()));
    }
    if let Some(max_results) = input.max_results {
        query.push(("maxResults".to_string(), max_results.to_string()));
    }
    if let Some(single_events) = input.single_events {
        query.push(("singleEvents".to_string(), single_events.to_string()));
    }
    if let Some(order_by) = input.order_by.as_deref().filter(|v| !v.is_empty()) {
        query.push(("orderBy".to_string(), order_by.to_string()));
    }
    Ok(HttpCall {
        method: Method::Get,
        path: format!("/calendars/{}/events", calendar_id(input)),
        query,
        body: None,
    })
}

fn build_get(input: &EventsInput) -> Result<HttpCall, String> {
    let event_id = super::require_field(input.event_id.as_deref(), "event_id")?;
    Ok(get_call(&calendar_id(input), event_id))
}

/// Build a GET request for a single event. Shared by the `get` operation and
/// by `respond`'s read-modify-write flow in the dispatch layer, which needs
/// the event's current `attendees` array before it can PATCH a merged one.
#[must_use]
pub fn get_call(calendar_id: &str, event_id: &str) -> HttpCall {
    HttpCall {
        method: Method::Get,
        path: format!("/calendars/{calendar_id}/events/{event_id}"),
        query: Vec::new(),
        body: None,
    }
}

fn build_update(input: &EventsInput) -> Result<HttpCall, String> {
    let event_id = super::require_field(input.event_id.as_deref(), "event_id")?;
    let event = input
        .event
        .clone()
        .ok_or_else(|| "missing required field: event".to_string())?;
    Ok(HttpCall {
        method: Method::Put,
        path: format!("/calendars/{}/events/{event_id}", calendar_id(input)),
        query: Vec::new(),
        body: Some(event),
    })
}

fn build_delete(input: &EventsInput) -> Result<HttpCall, String> {
    let event_id = super::require_field(input.event_id.as_deref(), "event_id")?;
    Ok(HttpCall {
        method: Method::Delete,
        path: format!("/calendars/{}/events/{event_id}", calendar_id(input)),
        query: Vec::new(),
        body: None,
    })
}

fn build_quick_add(input: &EventsInput) -> Result<HttpCall, String> {
    let text = super::require_field(input.text.as_deref(), "text")?.to_string();
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/calendars/{}/events/quickAdd", calendar_id(input)),
        query: vec![("text".to_string(), text)],
        body: None,
    })
}

/// Build a PATCH request that replaces the event's `attendees` array with
/// the given, already-merged value. Google's `events.patch` replaces
/// `attendees` wholesale (arrays are not merged server-side), so the caller
/// — `respond`'s read-modify-write flow in the dispatch layer — must supply
/// the *full* merged array, not just the responding attendee.
#[must_use]
pub fn patch_attendees_call(calendar_id: &str, event_id: &str, attendees: &Value) -> HttpCall {
    HttpCall {
        method: Method::Patch,
        path: format!("/calendars/{calendar_id}/events/{event_id}"),
        query: Vec::new(),
        body: Some(json!({ "attendees": attendees })),
    }
}

/// Merge an RSVP response into an event's existing `attendees` array
/// (pure, no I/O). `existing` is the `attendees` field read back from the
/// event (defaults to `[]` when absent/not an array). The attendee whose
/// `email` case-insensitively matches `email` has its `responseStatus` set
/// to `status`, with every other field and every other attendee left
/// untouched; if no attendee matches, `{email, responseStatus}` is appended
/// (self-adding).
#[must_use]
pub fn merge_attendee_response(existing: &Value, email: &str, status: &str) -> Value {
    let mut attendees: Vec<Value> = existing.as_array().cloned().unwrap_or_default();

    let matched = attendees.iter_mut().find(|attendee| {
        attendee
            .get("email")
            .and_then(Value::as_str)
            .is_some_and(|existing_email| existing_email.eq_ignore_ascii_case(email))
    });

    match matched {
        Some(attendee) => {
            attendee["responseStatus"] = Value::String(status.to_string());
        }
        None => attendees.push(json!({ "email": email, "responseStatus": status })),
    }

    Value::Array(attendees)
}

fn build_move(input: &EventsInput) -> Result<HttpCall, String> {
    let event_id = super::require_field(input.event_id.as_deref(), "event_id")?;
    let destination = super::require_field(
        input.destination_calendar_id.as_deref(),
        "destination_calendar_id",
    )?
    .to_string();
    Ok(HttpCall {
        method: Method::Post,
        path: format!("/calendars/{}/events/{event_id}/move", calendar_id(input)),
        query: vec![("destination".to_string(), destination)],
        body: None,
    })
}

/// Map a raw Google Calendar Events response body to the compact shape
/// returned to the model, based on the [`EventOp`] that produced it.
pub fn normalize(op: EventOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        EventOp::Search => normalize_search(raw),
        EventOp::Create
        | EventOp::Get
        | EventOp::Update
        | EventOp::QuickAdd
        | EventOp::Respond
        | EventOp::Move => normalize_record(raw),
        EventOp::Delete => Ok(normalize_ack(raw)),
    }
}

/// Map a single Events resource to `{id,summary,start,end,status,htmlLink}`,
/// without panicking on missing/absent fields.
fn map_event_record(value: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "id".to_string(),
        value.get("id").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "summary".to_string(),
        value.get("summary").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "start".to_string(),
        value.get("start").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "end".to_string(),
        value.get("end").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "status".to_string(),
        value.get("status").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "htmlLink".to_string(),
        value.get("htmlLink").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// Normalize a single-event response (create/get/update/quick_add/respond/
/// move) to `{id,summary,start,end,status,htmlLink}`.
fn normalize_record(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid event response: {err}"))?;
    Ok(map_event_record(&value))
}

/// Normalize a search (list) response to `{total,results:[{id,summary,start,end,status,htmlLink}]}`.
fn normalize_search(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid events response: {err}"))?;
    let results: Vec<Value> = value
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(map_event_record)
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

/// Normalize a delete response — Google's delete endpoint returns `204 No
/// Content` on success, so `raw` is typically empty; `id` is only
/// recoverable if the (unusual) response body happens to echo it.
fn normalize_ack(raw: &[u8]) -> Value {
    let id = if raw.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice::<Value>(raw)
            .ok()
            .and_then(|value| value.get("id").cloned())
            .unwrap_or(Value::Null)
    };
    json!({ "ok": true, "id": id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Method;

    #[test]
    fn create_builds_post_with_default_calendar_and_event_body() {
        let call = build_call(
            r#"{"operation":"create","event":{"summary":"Standup","start":{"dateTime":"2026-07-03T09:00:00Z"},"end":{"dateTime":"2026-07-03T09:30:00Z"}}}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/calendars/primary/events");
        assert_eq!(call.body.as_ref().unwrap()["summary"], "Standup");
    }

    #[test]
    fn create_uses_explicit_calendar_id() {
        let call = build_call(
            r#"{"operation":"create","calendar_id":"team@example.com","event":{"summary":"Hi"}}"#,
        )
        .unwrap();
        assert_eq!(call.path, "/calendars/team@example.com/events");
    }

    #[test]
    fn create_missing_event_names_field() {
        let err = build_call(r#"{"operation":"create"}"#).unwrap_err();
        assert!(err.contains("event"));
    }

    #[test]
    fn search_builds_get_with_query_params_and_default_calendar() {
        let call = build_call(
            r#"{"operation":"search","q":"standup","time_min":"2026-07-01T00:00:00Z","time_max":"2026-07-31T00:00:00Z","max_results":10,"single_events":true,"order_by":"startTime"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/calendars/primary/events");
        assert!(call.query.iter().any(|(k, v)| k == "q" && v == "standup"));
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "timeMin" && v == "2026-07-01T00:00:00Z")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "timeMax" && v == "2026-07-31T00:00:00Z")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "maxResults" && v == "10")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "singleEvents" && v == "true")
        );
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "orderBy" && v == "startTime")
        );
    }

    #[test]
    fn search_with_no_filters_has_empty_query() {
        let call = build_call(r#"{"operation":"search"}"#).unwrap();
        assert!(call.query.is_empty());
    }

    #[test]
    fn get_requires_event_id() {
        let err = build_call(r#"{"operation":"get"}"#).unwrap_err();
        assert!(err.contains("event_id"));
        let call = build_call(r#"{"operation":"get","event_id":"evt-1"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/calendars/primary/events/evt-1");
    }

    #[test]
    fn update_builds_put_with_event_body() {
        let call = build_call(
            r#"{"operation":"update","event_id":"evt-1","event":{"summary":"Updated"}}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Put));
        assert_eq!(call.path, "/calendars/primary/events/evt-1");
        assert_eq!(call.body.as_ref().unwrap()["summary"], "Updated");
    }

    #[test]
    fn update_missing_event_names_field() {
        let err = build_call(r#"{"operation":"update","event_id":"evt-1"}"#).unwrap_err();
        assert!(err.contains("event"));
    }

    #[test]
    fn delete_builds_delete() {
        let call = build_call(r#"{"operation":"delete","event_id":"evt-9"}"#).unwrap();
        assert!(matches!(call.method, Method::Delete));
        assert_eq!(call.path, "/calendars/primary/events/evt-9");
    }

    #[test]
    fn quick_add_requires_text_and_builds_query_string() {
        let err = build_call(r#"{"operation":"quick_add"}"#).unwrap_err();
        assert!(err.contains("text"));
        let call =
            build_call(r#"{"operation":"quick_add","text":"Lunch tomorrow at noon"}"#).unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/calendars/primary/events/quickAdd");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "text" && v == "Lunch tomorrow at noon")
        );
    }

    #[test]
    fn build_call_rejects_respond_since_it_is_dispatch_layer_read_modify_write() {
        // respond needs a GET before the PATCH (to avoid wiping other
        // attendees), which a single `HttpCall` cannot express; the
        // dispatch layer intercepts `respond` before calling `build_call`.
        assert!(
            build_call(
                r#"{"operation":"respond","event_id":"evt-1","attendee_email":"a@example.com","response_status":"accepted"}"#,
            )
            .is_err()
        );
    }

    #[test]
    fn parse_respond_request_requires_event_id_attendee_email_and_response_status() {
        assert!(parse_respond_request(r#"{"operation":"respond","event_id":"evt-1"}"#).is_err());
        assert!(
            parse_respond_request(
                r#"{"operation":"respond","event_id":"evt-1","attendee_email":"a@example.com"}"#
            )
            .is_err()
        );
        let request = parse_respond_request(
            r#"{"operation":"respond","event_id":"evt-1","attendee_email":"a@example.com","response_status":"accepted"}"#,
        )
        .unwrap();
        assert_eq!(request.calendar_id, "primary");
        assert_eq!(request.event_id, "evt-1");
        assert_eq!(request.attendee_email, "a@example.com");
        assert_eq!(request.response_status, "accepted");
    }

    #[test]
    fn parse_respond_request_uses_explicit_calendar_id() {
        let request = parse_respond_request(
            r#"{"operation":"respond","calendar_id":"team@example.com","event_id":"evt-1","attendee_email":"a@example.com","response_status":"declined"}"#,
        )
        .unwrap();
        assert_eq!(request.calendar_id, "team@example.com");
    }

    #[test]
    fn get_call_and_patch_attendees_call_build_the_expected_requests() {
        let get = get_call("primary", "evt-1");
        assert!(matches!(get.method, Method::Get));
        assert_eq!(get.path, "/calendars/primary/events/evt-1");
        assert!(get.body.is_none());

        let patch = patch_attendees_call("primary", "evt-1", &json!([{"email": "a@example.com"}]));
        assert!(matches!(patch.method, Method::Patch));
        assert_eq!(patch.path, "/calendars/primary/events/evt-1");
        assert_eq!(
            patch.body.as_ref().unwrap()["attendees"][0]["email"],
            "a@example.com"
        );
    }

    #[test]
    fn merge_attendee_response_updates_matching_attendee_and_preserves_others() {
        let existing = json!([
            { "email": "a@example.com", "responseStatus": "needsAction", "displayName": "Alice" },
            { "email": "b@example.com", "responseStatus": "accepted" }
        ]);
        let merged = merge_attendee_response(&existing, "a@example.com", "declined");
        assert_eq!(merged[0]["email"], "a@example.com");
        assert_eq!(merged[0]["responseStatus"], "declined");
        assert_eq!(merged[0]["displayName"], "Alice");
        assert_eq!(merged[1]["email"], "b@example.com");
        assert_eq!(merged[1]["responseStatus"], "accepted");
        assert_eq!(merged.as_array().unwrap().len(), 2);
    }

    #[test]
    fn merge_attendee_response_matches_case_insensitively() {
        let existing = json!([{ "email": "A@Example.com", "responseStatus": "needsAction" }]);
        let merged = merge_attendee_response(&existing, "a@example.com", "accepted");
        assert_eq!(merged.as_array().unwrap().len(), 1);
        assert_eq!(merged[0]["email"], "A@Example.com");
        assert_eq!(merged[0]["responseStatus"], "accepted");
    }

    #[test]
    fn merge_attendee_response_appends_when_no_attendee_matches() {
        let existing = json!([{ "email": "b@example.com", "responseStatus": "accepted" }]);
        let merged = merge_attendee_response(&existing, "a@example.com", "tentative");
        assert_eq!(merged.as_array().unwrap().len(), 2);
        assert_eq!(merged[0]["email"], "b@example.com");
        assert_eq!(merged[1]["email"], "a@example.com");
        assert_eq!(merged[1]["responseStatus"], "tentative");
    }

    #[test]
    fn merge_attendee_response_defaults_to_empty_when_attendees_absent() {
        let merged = merge_attendee_response(&Value::Null, "a@example.com", "accepted");
        assert_eq!(merged.as_array().unwrap().len(), 1);
        assert_eq!(merged[0]["email"], "a@example.com");
        assert_eq!(merged[0]["responseStatus"], "accepted");
    }

    #[test]
    fn move_requires_destination_calendar_id() {
        let err = build_call(r#"{"operation":"move","event_id":"evt-1"}"#).unwrap_err();
        assert!(err.contains("destination_calendar_id"));
        let call = build_call(
            r#"{"operation":"move","event_id":"evt-1","destination_calendar_id":"team@example.com"}"#,
        )
        .unwrap();
        assert!(matches!(call.method, Method::Post));
        assert_eq!(call.path, "/calendars/primary/events/evt-1/move");
        assert!(
            call.query
                .iter()
                .any(|(k, v)| k == "destination" && v == "team@example.com")
        );
    }

    #[test]
    fn normalize_record_extracts_fields_without_panicking() {
        let raw = br#"{"id":"evt-1","summary":"Standup","start":{"dateTime":"2026-07-03T09:00:00Z"},"end":{"dateTime":"2026-07-03T09:30:00Z"},"status":"confirmed","htmlLink":"https://calendar.google.com/x"}"#;
        let out = normalize(EventOp::Get, raw).unwrap();
        assert_eq!(out["id"], "evt-1");
        assert_eq!(out["summary"], "Standup");
        assert_eq!(out["status"], "confirmed");
        assert_eq!(out["htmlLink"], "https://calendar.google.com/x");
    }

    #[test]
    fn normalize_record_handles_missing_fields() {
        let raw = br#"{"id":"evt-1"}"#;
        let out = normalize(EventOp::Create, raw).unwrap();
        assert_eq!(out["summary"], Value::Null);
        assert_eq!(out["start"], Value::Null);
        assert_eq!(out["end"], Value::Null);
        assert_eq!(out["status"], Value::Null);
        assert_eq!(out["htmlLink"], Value::Null);
    }

    #[test]
    fn normalize_search_maps_items_list() {
        let raw =
            br#"{"items":[{"id":"evt-1","summary":"Standup"},{"id":"evt-2","summary":"Retro"}]}"#;
        let out = normalize(EventOp::Search, raw).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"][0]["id"], "evt-1");
        assert_eq!(out["results"][1]["summary"], "Retro");
    }

    #[test]
    fn normalize_search_handles_empty_items() {
        let raw = br#"{"items":[]}"#;
        let out = normalize(EventOp::Search, raw).unwrap();
        assert_eq!(out["total"], 0);
        assert_eq!(out["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn normalize_delete_ack_handles_empty_body() {
        let out = normalize(EventOp::Delete, b"").unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["id"], Value::Null);
    }

    #[test]
    fn normalize_quick_add_and_move_and_respond_return_record() {
        let raw = br#"{"id":"evt-1","summary":"Lunch"}"#;
        assert_eq!(normalize(EventOp::QuickAdd, raw).unwrap()["id"], "evt-1");
        assert_eq!(normalize(EventOp::Move, raw).unwrap()["id"], "evt-1");
        assert_eq!(normalize(EventOp::Respond, raw).unwrap()["id"], "evt-1");
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(
            parse_operation(r#"{"operation":"delete","event_id":"evt-9"}"#),
            Ok(EventOp::Delete)
        );
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
