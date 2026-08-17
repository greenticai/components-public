//! Static metadata for the Calendly tools: names, JSON schemas, capability
//! flags, the `agentic_worker` metadata blob, and the secret requirements
//! each tool declares. Pure (no WIT imports) so it is fully host-testable;
//! `lib.rs` maps [`ToolMeta`] onto the WIT `ToolDefinition` shape (which has
//! no `secret_requirements` field — that list is consumed by the
//! `describe.json` authoring step, a later task).
//!
//! This is the template later tool domains extend: add a `const <NAME>_TOOL`
//! name, a builder function returning a [`ToolMeta`], and push it onto the
//! `vec![...]` in [`all_tools`]. Wire the matching dispatch arm in
//! `lib.rs::invoke_tool`.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
pub const ME_TOOL: &str = "calendly_me";
pub const EVENT_TYPES_TOOL: &str = "calendly_event_types";
pub const EVENTS_TOOL: &str = "calendly_events";
pub const INVITEES_TOOL: &str = "calendly_invitees";
pub const SCHEDULING_LINKS_TOOL: &str = "calendly_scheduling_links";
pub const AVAILABILITY_TOOL: &str = "calendly_availability";
pub const WEBHOOKS_TOOL: &str = "calendly_webhooks";

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
/// later task) so the runtime can grant/prompt for it. Mirrors the shape
/// used in Jira's `describe.json` `contributions.tools[].secret_requirements`.
pub struct SecretRequirement {
    /// Secret key, without the `secret://` scheme (e.g. `"calendly/token"`).
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

/// The five `calendly/*` secrets any Calendly tool may read, depending on
/// the configured `auth_mode`. All optional: `required:false`, so a tenant
/// can configure either Token or OAuth mode without the other's secrets
/// present.
fn calendly_secret_requirements() -> Vec<SecretRequirement> {
    vec![
        secret(
            "calendly/token",
            "Calendly Personal Access Token for Bearer auth, used when auth_mode is token (the default). Resolved by the host from secret://calendly/token and never returned to the model.",
        ),
        secret(
            "calendly/auth_mode",
            "Auth strategy selector: \"oauth\" routes tokens through the platform OAuth broker (auto-refreshed) or a brokerless refresh; any other value or unset uses the static Bearer token (token mode).",
        ),
        secret(
            "calendly/oauth_refresh_token",
            "OAuth refresh token (brokerless OAuth mode). Stored by gtc setup after consent; the component refreshes the Calendly access token itself. Used only when auth_mode=oauth.",
        ),
        secret(
            "calendly/oauth_client_id",
            "Calendly OAuth app Client ID (brokerless OAuth mode).",
        ),
        secret(
            "calendly/oauth_client_secret",
            "Calendly OAuth app Client Secret (brokerless OAuth mode); resolved by the host, never returned to the model.",
        ),
    ]
}

fn me_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["get"], "description": "Which Calendly current-user action to perform." }
  }
}"#
    .to_string()
}

fn me_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "The current Calendly user: {uri,name,email,current_organization,scheduling_url}.",
  "properties": {
    "uri": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "email": { "type": ["string", "null"] },
    "current_organization": { "type": ["string", "null"] },
    "scheduling_url": { "type": ["string", "null"] }
  }
}"#
    .to_string()
}

fn me_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Look up the current Calendly user (the account the configured token belongs to). Set 'operation' to get. The returned 'uri' is the 'user' value other Calendly tools' list operations need, and 'current_organization' is the 'organization' value.",
  "examples": [
    { "when": "resolving the user/organization URIs before listing event types or scheduled events", "input": { "operation": "get" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn me_tool() -> ToolMeta {
    ToolMeta {
        name: ME_TOOL.to_string(),
        description:
            "Look up the current Calendly user. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: me_input_schema(),
        output_schema_json: me_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: me_agentic_worker_metadata(),
        secret_requirements: calendly_secret_requirements(),
    }
}

fn event_types_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get"], "description": "Which Calendly event type action to perform." },
    "user": { "type": "string", "description": "Calendly user URI to scope the list to (from calendly_me). Exactly one of user or organization is required for list." },
    "organization": { "type": "string", "description": "Calendly organization URI to scope the list to (from calendly_me). Exactly one of user or organization is required for list." },
    "uuid": { "type": "string", "description": "Event type UUID. Required for get." },
    "active": { "type": "boolean", "description": "Filter by active status, for list." },
    "count": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list (default Calendly page size)." }
  }
}"#
    .to_string()
}

fn event_types_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get: a single event type {uri,name,duration,active,scheduling_url}. For list: {total,results:[{uri,name,duration,active,scheduling_url}]}.",
  "properties": {
    "uri": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "duration": { "type": ["integer", "null"] },
    "active": { "type": ["boolean", "null"] },
    "scheduling_url": { "type": ["string", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn event_types_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Look up Calendly event types. Set 'operation' to list (needs exactly one of 'user' or 'organization', both obtainable from calendly_me) or get (by 'uuid').",
  "examples": [
    { "when": "listing a user's active event types", "input": { "operation": "list", "user": "https://api.calendly.com/users/AAAA", "active": true } },
    { "when": "fetching one event type by uuid", "input": { "operation": "get", "uuid": "BBBB" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn event_types_tool() -> ToolMeta {
    ToolMeta {
        name: EVENT_TYPES_TOOL.to_string(),
        description:
            "List or get Calendly event types. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: event_types_input_schema(),
        output_schema_json: event_types_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: event_types_agentic_worker_metadata(),
        secret_requirements: calendly_secret_requirements(),
    }
}

fn events_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get", "cancel"], "description": "Which Calendly scheduled event action to perform." },
    "user": { "type": "string", "description": "Calendly user URI to scope the list to (from calendly_me). Exactly one of user or organization is required for list." },
    "organization": { "type": "string", "description": "Calendly organization URI to scope the list to (from calendly_me). Exactly one of user or organization is required for list." },
    "uuid": { "type": "string", "description": "Scheduled event UUID. Required for get and cancel." },
    "status": { "type": "string", "enum": ["active", "canceled"], "description": "Filter by event status, for list." },
    "count": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list (default Calendly page size)." },
    "min_start_time": { "type": "string", "description": "ISO-8601 lower bound on start_time, for list." },
    "max_start_time": { "type": "string", "description": "ISO-8601 upper bound on start_time, for list." },
    "reason": { "type": "string", "description": "Optional cancellation reason, for cancel." }
  }
}"#
    .to_string()
}

fn events_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get: a single scheduled event {uri,name,status,start_time,end_time}. For list: {total,results:[{uri,name,status,start_time,end_time}]}. For cancel: {ok,id}.",
  "properties": {
    "uri": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "status": { "type": ["string", "null"] },
    "start_time": { "type": ["string", "null"] },
    "end_time": { "type": ["string", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" },
    "id": { "type": ["string", "null"] }
  }
}"#
    .to_string()
}

fn events_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Calendly scheduled events. Set 'operation' to list (needs exactly one of 'user' or 'organization', both obtainable from calendly_me), get (by 'uuid'), or cancel (by 'uuid', optionally with 'reason'). Confirm with the user before cancel.",
  "examples": [
    { "when": "listing a user's upcoming active events", "input": { "operation": "list", "user": "https://api.calendly.com/users/AAAA", "status": "active" } },
    { "when": "canceling a scheduled event", "input": { "operation": "cancel", "uuid": "CCCC", "reason": "Scheduling conflict" } }
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
            "List, get, or cancel Calendly scheduled events. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: events_input_schema(),
        output_schema_json: events_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: events_agentic_worker_metadata(),
        secret_requirements: calendly_secret_requirements(),
    }
}

fn invitees_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get"], "description": "Which Calendly invitee action to perform." },
    "event_uuid": { "type": "string", "description": "Scheduled event UUID the invitee(s) belong to. Required for list and get." },
    "invitee_uuid": { "type": "string", "description": "Invitee UUID. Required for get." },
    "count": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list (default Calendly page size)." },
    "status": { "type": "string", "enum": ["active", "canceled"], "description": "Filter by invitee status, for list." },
    "email": { "type": "string", "description": "Filter by invitee email, for list." }
  }
}"#
    .to_string()
}

fn invitees_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get: a single invitee {uri,email,name,status}. For list: {total,results:[{uri,email,name,status}]}.",
  "properties": {
    "uri": { "type": ["string", "null"] },
    "email": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "status": { "type": ["string", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn invitees_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Look up invitees on a Calendly scheduled event. Set 'operation' to list (needs 'event_uuid') or get (needs 'event_uuid' and 'invitee_uuid').",
  "examples": [
    { "when": "listing active invitees on an event", "input": { "operation": "list", "event_uuid": "CCCC", "status": "active" } },
    { "when": "fetching one invitee by uuid", "input": { "operation": "get", "event_uuid": "CCCC", "invitee_uuid": "DDDD" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn invitees_tool() -> ToolMeta {
    ToolMeta {
        name: INVITEES_TOOL.to_string(),
        description:
            "List or get invitees on a Calendly scheduled event. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: invitees_input_schema(),
        output_schema_json: invitees_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: invitees_agentic_worker_metadata(),
        secret_requirements: calendly_secret_requirements(),
    }
}

fn scheduling_links_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create"], "description": "Which Calendly scheduling link action to perform." },
    "event_type_uri": { "type": "string", "description": "Event type URI to create a single-use scheduling link for (from calendly_event_types). Required for create." }
  }
}"#
    .to_string()
}

fn scheduling_links_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "The created single-use scheduling link: {booking_url,owner,owner_type}.",
  "properties": {
    "booking_url": { "type": ["string", "null"] },
    "owner": { "type": ["string", "null"] },
    "owner_type": { "type": ["string", "null"] }
  }
}"#
    .to_string()
}

fn scheduling_links_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Create a single-use Calendly scheduling link for an event type. Set 'operation' to create with 'event_type_uri' (from calendly_event_types). The returned 'booking_url' can be shared directly with an invitee.",
  "examples": [
    { "when": "generating a one-time booking link for a specific event type", "input": { "operation": "create", "event_type_uri": "https://api.calendly.com/event_types/AAAA" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn scheduling_links_tool() -> ToolMeta {
    ToolMeta {
        name: SCHEDULING_LINKS_TOOL.to_string(),
        description:
            "Create a single-use Calendly scheduling link for an event type. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: scheduling_links_input_schema(),
        output_schema_json: scheduling_links_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: scheduling_links_agentic_worker_metadata(),
        secret_requirements: calendly_secret_requirements(),
    }
}

fn availability_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["user_busy_times", "list_schedules"], "description": "Which Calendly availability action to perform." },
    "user": { "type": "string", "description": "Calendly user URI to scope the lookup to (from calendly_me). Required for both operations." },
    "start_time": { "type": "string", "description": "ISO-8601 lower bound on the busy-time window. Required for user_busy_times." },
    "end_time": { "type": "string", "description": "ISO-8601 upper bound on the busy-time window. Required for user_busy_times." }
  }
}"#
    .to_string()
}

fn availability_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For user_busy_times: {results:[{type,start_time,end_time}]}. For list_schedules: {results:[{uri,name,default}]}.",
  "properties": {
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn availability_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Look up a Calendly user's busy times or availability schedules. Set 'operation' to user_busy_times (needs 'user', 'start_time', 'end_time') or list_schedules (needs 'user'). The 'user' URI comes from calendly_me.",
  "examples": [
    { "when": "checking a user's busy blocks before proposing a meeting time", "input": { "operation": "user_busy_times", "user": "https://api.calendly.com/users/AAAA", "start_time": "2026-07-03T00:00:00Z", "end_time": "2026-07-10T00:00:00Z" } },
    { "when": "listing a user's named availability schedules", "input": { "operation": "list_schedules", "user": "https://api.calendly.com/users/AAAA" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn availability_tool() -> ToolMeta {
    ToolMeta {
        name: AVAILABILITY_TOOL.to_string(),
        description:
            "Look up a Calendly user's busy times or availability schedules. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: availability_input_schema(),
        output_schema_json: availability_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: availability_agentic_worker_metadata(),
        secret_requirements: calendly_secret_requirements(),
    }
}

fn webhooks_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "list", "delete"], "description": "Which Calendly webhook subscription action to perform." },
    "url": { "type": "string", "description": "Callback URL Calendly will POST events to. Required for create." },
    "events": { "type": "array", "items": { "type": "string" }, "description": "Event types to subscribe to (e.g. invitee.created, invitee.canceled). Required for create." },
    "organization": { "type": "string", "description": "Calendly organization URI (from calendly_me). Required for create and list." },
    "user": { "type": "string", "description": "Calendly user URI (from calendly_me). Required for create/list when scope is user." },
    "scope": { "type": "string", "enum": ["organization", "user"], "description": "Subscription scope. Required for create and list." },
    "signing_key": { "type": "string", "description": "Optional secret used to sign webhook payloads, for create." },
    "count": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list (default Calendly page size)." },
    "uuid": { "type": "string", "description": "Webhook subscription UUID. Required for delete." }
  }
}"#
    .to_string()
}

fn webhooks_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create: a single webhook subscription {uri,callback_url,state,events,scope}. For list: {total,results:[{uri,callback_url,state,events,scope}]}. For delete: {ok,id}.",
  "properties": {
    "uri": { "type": ["string", "null"] },
    "callback_url": { "type": ["string", "null"] },
    "state": { "type": ["string", "null"] },
    "events": { "type": ["array", "null"], "items": { "type": "string" } },
    "scope": { "type": ["string", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" },
    "id": { "type": ["string", "null"] }
  }
}"#
    .to_string()
}

fn webhooks_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Calendly webhook subscriptions. Set 'operation' to create (needs 'url', 'events', 'organization', 'scope'), list (needs 'organization', 'scope'), or delete (needs 'uuid'). Confirm with the user before delete.",
  "examples": [
    { "when": "subscribing to invitee-created events for an organization", "input": { "operation": "create", "url": "https://example.com/hooks/calendly", "events": ["invitee.created"], "organization": "https://api.calendly.com/organizations/AAAA", "scope": "organization" } },
    { "when": "removing a webhook subscription", "input": { "operation": "delete", "uuid": "BBBB" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn webhooks_tool() -> ToolMeta {
    ToolMeta {
        name: WEBHOOKS_TOOL.to_string(),
        description:
            "Create, list, or delete Calendly webhook subscriptions. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: webhooks_input_schema(),
        output_schema_json: webhooks_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: webhooks_agentic_worker_metadata(),
        secret_requirements: calendly_secret_requirements(),
    }
}

/// All tool definitions the extension exposes.
#[must_use]
pub fn all_tools() -> Vec<ToolMeta> {
    vec![
        me_tool(),
        event_types_tool(),
        events_tool(),
        invitees_tool(),
        scheduling_links_tool(),
        availability_tool(),
        webhooks_tool(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_all_seven_calendly_tools() {
        let names: Vec<String> = all_tools().into_iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            vec![
                ME_TOOL,
                EVENT_TYPES_TOOL,
                EVENTS_TOOL,
                INVITEES_TOOL,
                SCHEDULING_LINKS_TOOL,
                AVAILABILITY_TOOL,
                WEBHOOKS_TOOL,
            ]
        );
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
    fn every_tool_declares_five_optional_calendly_secrets() {
        for tool in all_tools() {
            assert_eq!(tool.secret_requirements.len(), 5);
            assert!(tool.secret_requirements.iter().all(|req| !req.required));
            let keys: Vec<&str> = tool
                .secret_requirements
                .iter()
                .map(|req| req.key.as_str())
                .collect();
            assert_eq!(
                keys,
                vec![
                    "calendly/token",
                    "calendly/auth_mode",
                    "calendly/oauth_refresh_token",
                    "calendly/oauth_client_id",
                    "calendly/oauth_client_secret",
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
