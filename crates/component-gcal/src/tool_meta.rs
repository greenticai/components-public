//! Static metadata for the Google Calendar tools: names, JSON schemas,
//! capability flags, the `agentic_worker` metadata blob, and the secret
//! requirements each tool declares. Pure (no WIT imports) so it is fully
//! host-testable; `lib.rs` maps [`ToolMeta`] onto the WIT `ToolDefinition`
//! shape (which has no `secret_requirements` field — that list is consumed
//! by the `describe.json` authoring step, a follow-up task).
//!
//! This is the template the Google Calendar tool domains extend: add a
//! `const <NAME>_TOOL` name, a builder function returning a [`ToolMeta`], and
//! push it onto the `vec![...]` in [`all_tools`]. Wire the matching
//! dispatch arm in `lib.rs::invoke_tool`.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
pub const EVENTS_TOOL: &str = "gcal_events";
pub const CALENDARS_TOOL: &str = "gcal_calendars";
pub const FREEBUSY_TOOL: &str = "gcal_freebusy";

/// Plain (non-WIT) description of one tool definition.
pub struct ToolMeta {
    pub name: String,
    pub description: String,
    pub input_schema_json: String,
    pub output_schema_json: String,
    pub capabilities: Vec<String>,
    pub agentic_worker_metadata: String,
    pub secret_requirements: Vec<SecretRequirement>,
}

/// One secret the tool may read, surfaced to `describe.json` authoring (a
/// follow-up task) so the runtime can grant/prompt for it. Mirrors the shape
/// used in Jira's `describe.json` `contributions.tools[].secret_requirements`.
pub struct SecretRequirement {
    /// Secret key, without the `secret://` scheme (e.g. `"gcal/oauth_refresh_token"`).
    pub key: String,
    pub required: bool,
    pub description: String,
    pub format: String,
}

fn secret(key: &str, description: &str) -> SecretRequirement {
    SecretRequirement {
        key: key.to_string(),
        required: false,
        description: description.to_string(),
        format: "text".to_string(),
    }
}

/// The three `gcal/*` OAuth secrets any Google Calendar tool may read. All
/// optional: `required:false`, so `describe.json` authoring can decide
/// whether the tenant configured brokerless OAuth (all three present) or
/// relies on the platform OAuth broker (none present).
fn gcal_secret_requirements() -> Vec<SecretRequirement> {
    vec![
        secret(
            "gcal/oauth_refresh_token",
            "OAuth refresh token (brokerless OAuth mode). Stored by gtc setup after consent; the component refreshes the Google access token itself. Resolved by the host and never returned to the model.",
        ),
        secret(
            "gcal/oauth_client_id",
            "Google Cloud OAuth app Client ID (brokerless OAuth mode).",
        ),
        secret(
            "gcal/oauth_client_secret",
            "Google Cloud OAuth app Client Secret (brokerless OAuth mode); resolved by the host, never returned to the model.",
        ),
    ]
}

fn events_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "search", "get", "update", "delete", "quick_add", "respond", "move"], "description": "Which Google Calendar event action to perform." },
    "calendar_id": { "type": "string", "description": "Calendar id to act on (e.g. \"primary\" or a calendar email address). Defaults to \"primary\" when omitted." },
    "event_id": { "type": "string", "description": "Event id. Required for get, update, delete, respond, and move." },
    "event": { "type": "object", "additionalProperties": true, "description": "Event resource (summary, start, end, description, attendees, location, etc.), passed through as-is. Required for create and update." },
    "text": { "type": "string", "description": "Free-text event description, parsed by Google (e.g. \"Lunch tomorrow at noon\"). Required for quick_add." },
    "destination_calendar_id": { "type": "string", "description": "Calendar id to move the event to. Required for move." },
    "attendee_email": { "type": "string", "description": "Email address of the responding attendee. Required for respond." },
    "response_status": { "type": "string", "enum": ["accepted", "declined", "tentative"], "description": "The attendee's RSVP status. Required for respond." },
    "q": { "type": "string", "description": "Free-text search query, for search." },
    "time_min": { "type": "string", "description": "ISO-8601 lower bound (exclusive) on event end time, for search." },
    "time_max": { "type": "string", "description": "ISO-8601 upper bound (exclusive) on event start time, for search." },
    "max_results": { "type": "integer", "minimum": 1, "maximum": 2500, "description": "Max search results (default Google page size)." },
    "single_events": { "type": "boolean", "description": "Whether to expand recurring events into instances, for search." },
    "order_by": { "type": "string", "enum": ["startTime", "updated"], "description": "Sort order for search results." }
  }
}"#
    .to_string()
}

fn events_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get/update/quick_add/respond/move: a single event {id,summary,start,end,status,htmlLink}. For search: {total,results:[{id,summary,start,end,status,htmlLink}]}. For delete: {ok,id}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "summary": { "type": ["string", "null"] },
    "start": {},
    "end": {},
    "status": { "type": ["string", "null"] },
    "htmlLink": { "type": ["string", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" }
  }
}"#
    .to_string()
}

fn events_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Google Calendar events. Set 'operation' to create, search, get, update, delete, quick_add, respond, or move. calendar_id defaults to \"primary\". Search before creating to avoid duplicates; confirm with the user before delete.",
  "examples": [
    { "when": "checking today's schedule", "input": { "operation": "search", "time_min": "2026-07-02T00:00:00Z", "time_max": "2026-07-03T00:00:00Z", "single_events": true, "order_by": "startTime" } },
    { "when": "scheduling a quick meeting from natural language", "input": { "operation": "quick_add", "text": "Lunch with Jane tomorrow at noon" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn events_tool() -> ToolMeta {
    ToolMeta {
        name: EVENTS_TOOL.to_string(),
        description:
            "Create, search, get, update, delete, quick-add, respond to, or move Google Calendar events. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: events_input_schema(),
        output_schema_json: events_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: events_agentic_worker_metadata(),
        secret_requirements: gcal_secret_requirements(),
    }
}

fn calendars_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get", "create"], "description": "Which Google Calendar calendar action to perform." },
    "calendar_id": { "type": "string", "description": "Calendar id (e.g. \"primary\" or a calendar email address). Defaults to \"primary\" when omitted, for get." },
    "summary": { "type": "string", "description": "Calendar display name. Required for create." }
  }
}"#
    .to_string()
}

fn calendars_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get/create: a single calendar {id,summary,timeZone?,accessRole?}. For list: {total,results:[{id,summary,timeZone?,accessRole?}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "summary": { "type": ["string", "null"] },
    "timeZone": { "type": "string" },
    "accessRole": { "type": "string" },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn calendars_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Look up or create Google Calendars. Set 'operation' to list (all calendars on the user's calendar list), get (by 'calendar_id', defaults to \"primary\"), or create (needs 'summary').",
  "examples": [
    { "when": "listing which calendars the user has access to", "input": { "operation": "list" } },
    { "when": "fetching the primary calendar's metadata", "input": { "operation": "get" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn calendars_tool() -> ToolMeta {
    ToolMeta {
        name: CALENDARS_TOOL.to_string(),
        description:
            "List, get, or create Google Calendars. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: calendars_input_schema(),
        output_schema_json: calendars_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: calendars_agentic_worker_metadata(),
        secret_requirements: gcal_secret_requirements(),
    }
}

fn freebusy_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation", "time_min", "time_max"],
  "properties": {
    "operation": { "type": "string", "enum": ["query"], "description": "Which Google Calendar free/busy action to perform." },
    "time_min": { "type": "string", "description": "ISO-8601 start of the interval to query. Required." },
    "time_max": { "type": "string", "description": "ISO-8601 end of the interval to query. Required." },
    "calendar_ids": { "type": "array", "items": { "type": "string" }, "description": "Calendar ids to query. Defaults to [\"primary\"] when omitted." }
  }
}"#
    .to_string()
}

fn freebusy_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "{results:[{calendar_id,busy:[{start,end}]}]}.",
  "properties": {
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn freebusy_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Check free/busy status across one or more Google Calendars. Set 'operation' to query with 'time_min', 'time_max', and optionally 'calendar_ids' (defaults to [\"primary\"]).",
  "examples": [
    { "when": "checking if the user is free tomorrow afternoon", "input": { "operation": "query", "time_min": "2026-07-03T13:00:00Z", "time_max": "2026-07-03T17:00:00Z" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn freebusy_tool() -> ToolMeta {
    ToolMeta {
        name: FREEBUSY_TOOL.to_string(),
        description:
            "Query free/busy status across one or more Google Calendars. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: freebusy_input_schema(),
        output_schema_json: freebusy_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: freebusy_agentic_worker_metadata(),
        secret_requirements: gcal_secret_requirements(),
    }
}

/// All tool definitions the extension exposes.
#[must_use]
pub fn all_tools() -> Vec<ToolMeta> {
    vec![events_tool(), calendars_tool(), freebusy_tool()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_all_three_gcal_tools() {
        let names: Vec<String> = all_tools().into_iter().map(|tool| tool.name).collect();
        assert_eq!(names, vec![EVENTS_TOOL, CALENDARS_TOOL, FREEBUSY_TOOL]);
    }

    #[test]
    fn every_tool_declares_agentic_worker() {
        for tool in all_tools() {
            assert!(
                tool.capabilities.iter().any(|cap| cap == "agentic_worker"),
                "{} must opt into the agentic worker",
                tool.name
            );
        }
    }

    #[test]
    fn every_tool_declares_three_optional_gcal_oauth_secrets() {
        for tool in all_tools() {
            assert_eq!(tool.secret_requirements.len(), 3);
            assert!(tool.secret_requirements.iter().all(|req| !req.required));
            let keys: Vec<&str> = tool
                .secret_requirements
                .iter()
                .map(|req| req.key.as_str())
                .collect();
            assert_eq!(
                keys,
                vec![
                    "gcal/oauth_refresh_token",
                    "gcal/oauth_client_id",
                    "gcal/oauth_client_secret",
                ]
            );
        }
    }

    #[test]
    fn all_schemas_and_metadata_are_valid_json() {
        for tool in all_tools() {
            serde_json::from_str::<serde_json::Value>(&tool.input_schema_json)
                .unwrap_or_else(|_| panic!("{} input schema invalid", tool.name));
            serde_json::from_str::<serde_json::Value>(&tool.output_schema_json)
                .unwrap_or_else(|_| panic!("{} output schema invalid", tool.name));
            serde_json::from_str::<serde_json::Value>(&tool.agentic_worker_metadata)
                .unwrap_or_else(|_| panic!("{} aw metadata invalid", tool.name));
        }
    }
}
