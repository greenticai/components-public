//! Static metadata for the thirteen HubSpot tools: names, JSON schemas, capability
//! flags, and the `agentic_worker` metadata blob. The nine CRUD tools share one
//! schema shape generated per object, keeping this file small. Pure (no WIT
//! imports) so the `agentic_worker` opt-in is asserted by a host test.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
pub const CONTACTS_TOOL: &str = "hubspot_contacts";
pub const DEALS_TOOL: &str = "hubspot_deals";
pub const COMPANIES_TOOL: &str = "hubspot_companies";
pub const TICKETS_TOOL: &str = "hubspot_tickets";
pub const NOTES_TOOL: &str = "hubspot_notes";
pub const TASKS_TOOL: &str = "hubspot_tasks";
pub const CALLS_TOOL: &str = "hubspot_calls";
pub const MEETINGS_TOOL: &str = "hubspot_meetings";
pub const EMAILS_TOOL: &str = "hubspot_emails";
pub const PIPELINES_TOOL: &str = "hubspot_pipelines";
pub const OWNERS_TOOL: &str = "hubspot_owners";
pub const BATCH_TOOL: &str = "hubspot_batch";
pub const ASSOCIATE_TOOL: &str = "hubspot_associate";

/// Plain (non-WIT) description of one tool definition.
pub struct ToolMeta {
    pub name: String,
    pub description: String,
    pub input_schema_json: String,
    pub output_schema_json: String,
    pub capabilities: Vec<String>,
    pub agentic_worker_metadata: String,
}

/// Map a CRUD tool name to its HubSpot object type. `None` for non-CRUD tools
/// (associate, pipelines, owners, batch) or any unknown name.
#[must_use]
pub fn object_for_tool(name: &str) -> Option<&'static str> {
    match name {
        CONTACTS_TOOL => Some("contacts"),
        DEALS_TOOL => Some("deals"),
        COMPANIES_TOOL => Some("companies"),
        TICKETS_TOOL => Some("tickets"),
        NOTES_TOOL => Some("notes"),
        TASKS_TOOL => Some("tasks"),
        CALLS_TOOL => Some("calls"),
        MEETINGS_TOOL => Some("meetings"),
        EMAILS_TOOL => Some("emails"),
        _ => None,
    }
}

fn crud_input_schema(object_label: &str, property_examples: &str) -> String {
    format!(
        r#"{{
  "type": "object",
  "required": ["operation"],
  "properties": {{
    "operation": {{ "type": "string", "enum": ["create", "search", "update", "get"], "description": "Which CRUD action to perform on {object_label}." }},
    "id": {{ "type": "string", "description": "Record id. Required for 'get' and 'update'." }},
    "query": {{ "type": "string", "description": "Free-text search term (matched across {object_label} searchable properties). Required for 'search'." }},
    "properties": {{ "type": "object", "additionalProperties": true, "description": "{object_label} properties to set. Required for 'create' and 'update'. Example fields: {property_examples}." }},
    "return_properties": {{ "type": "array", "items": {{ "type": "string" }}, "description": "Property names to include in the response (for 'get' and 'search')." }},
    "limit": {{ "type": "integer", "minimum": 1, "maximum": 100, "description": "Max search results (default 10)." }}
  }}
}}"#
    )
}

fn crud_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/update/get: a single record {object,id,properties,created_at?,updated_at?}. For search: {results:[...],total}.",
  "properties": {
    "object": { "type": "string" },
    "id": { "type": "string" },
    "properties": { "type": "object" },
    "created_at": { "type": "string" },
    "updated_at": { "type": "string" },
    "results": { "type": "array", "items": { "type": "object" } },
    "total": { "type": "integer" }
  }
}"#
    .to_string()
}

fn crud_aw_meta(object_label: &str) -> String {
    format!(
        r#"{{
  "usage_hint": "Manage HubSpot {object_label}. Set 'operation' to create, search, get, or update. Search before creating to avoid duplicates; confirm with the user before update.",
  "examples": [
    {{ "when": "looking up {object_label} by a term", "input": {{ "operation": "search", "query": "acme" }} }}
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}}"#
    )
}

fn crud_tool(
    name: &str,
    object_label: &str,
    property_examples: &str,
    description: &str,
) -> ToolMeta {
    ToolMeta {
        name: name.to_string(),
        description: description.to_string(),
        input_schema_json: crud_input_schema(object_label, property_examples),
        output_schema_json: crud_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: crud_aw_meta(object_label),
    }
}

fn pipelines_tool() -> ToolMeta {
    // NOTE: no format arguments — raw string avoids clippy::useless_format.
    let input_schema = r#"{
  "type": "object",
  "required": ["object_type"],
  "properties": {
    "object_type": { "type": "string", "enum": ["deals", "tickets"], "description": "Which pipeline family to read." },
    "pipeline_id": { "type": "string", "description": "Optional pipeline id; omit to list all pipelines." }
  }
}"#
    .to_string();
    let output_schema = r#"{
  "type": "object",
  "description": "HubSpot pipelines response: {results:[{id,label,stages:[{id,label,displayOrder}]}]} for a list, or a single pipeline object."
}"#
    .to_string();
    let aw_meta = r#"{
  "usage_hint": "Read HubSpot deal or ticket pipelines and their stages, to select a valid dealstage/stage id before creating or updating a deal/ticket.",
  "examples": [
    { "when": "listing deal pipelines", "input": { "object_type": "deals" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string();
    ToolMeta {
        name: PIPELINES_TOOL.to_string(),
        description: "Read HubSpot deal or ticket pipelines and their stages.".to_string(),
        input_schema_json: input_schema,
        output_schema_json: output_schema,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: aw_meta,
    }
}

fn owners_tool() -> ToolMeta {
    // NOTE: no format arguments — raw string avoids clippy::useless_format.
    let input_schema = r#"{
  "type": "object",
  "properties": {
    "owner_id": { "type": "string", "description": "Optional owner id; omit to list owners." },
    "limit": { "type": "integer", "description": "Max owners to list (default 100)." }
  }
}"#
    .to_string();
    let output_schema = r#"{
  "type": "object",
  "description": "HubSpot owners response: {results:[{id,email,firstName,lastName,userId}]} for a list, or a single owner."
}"#
    .to_string();
    let aw_meta = r#"{
  "usage_hint": "List or get HubSpot owners (portal users) to assign a hubspot_owner_id on a deal or ticket.",
  "examples": [
    { "when": "listing owners", "input": {} }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string();
    ToolMeta {
        name: OWNERS_TOOL.to_string(),
        description: "List or get HubSpot owners (portal users).".to_string(),
        input_schema_json: input_schema,
        output_schema_json: output_schema,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: aw_meta,
    }
}

fn batch_tool() -> ToolMeta {
    // NOTE: no format arguments — raw string avoids clippy::useless_format.
    let input_schema = r#"{"type":"object","required":["object_type","operation","inputs"],"properties":{"object_type":{"type":"string","enum":["contacts","deals","companies","tickets","notes","tasks","calls","meetings"]},"operation":{"type":"string","enum":["read","create","update"]},"inputs":{"type":"array","description":"Batch inputs: read=[{id}], create=[{properties}], update=[{id,properties}]."}}}"#
        .to_string();
    let output_schema =
        r#"{"type":"object","description":"HubSpot batch response {status,results:[...]}."}"#
            .to_string();
    let aw_meta = r#"{"usage_hint":"Batch read/create/update up to 100 HubSpot records of one object type in a single call. inputs shape depends on operation: read=[{id}], create=[{properties}], update=[{id,properties}].","examples":[{"when":"batch-reading contacts by id","input":{"object_type":"contacts","operation":"read","inputs":[{"id":"1"},{"id":"2"}]}}],"side_effects":"write","cost":"low","confirmation_required":false}"#
        .to_string();
    ToolMeta {
        name: BATCH_TOOL.to_string(),
        description: "Batch read, create, or update HubSpot records of one object type."
            .to_string(),
        input_schema_json: input_schema,
        output_schema_json: output_schema,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: aw_meta,
    }
}

fn associate_tool() -> ToolMeta {
    // NOTE: no format arguments here — raw string avoids clippy::useless_format.
    let input_schema = r#"{
  "type": "object",
  "required": ["from_object", "from_id", "to_object", "to_id"],
  "properties": {
    "from_object": { "type": "string", "enum": ["contacts", "deals", "companies", "tickets", "notes", "tasks", "calls", "meetings", "emails"], "description": "Source object type." },
    "from_id": { "type": "string", "description": "Source record id." },
    "to_object": { "type": "string", "enum": ["contacts", "deals", "companies", "tickets", "notes", "tasks", "calls", "meetings", "emails"], "description": "Target object type." },
    "to_id": { "type": "string", "description": "Target record id." },
    "association_type": { "type": "string", "description": "Reserved for custom association labels (not yet used; a default association is created)." }
  }
}"#
    .to_string();
    let output_schema = r#"{
  "type": "object",
  "properties": {
    "ok": { "type": "boolean" },
    "from": { "type": "object" },
    "to": { "type": "object" }
  }
}"#
    .to_string();
    let aw_meta = r#"{
  "usage_hint": "Link two HubSpot records with a default association, e.g. attach a contact to a company or a deal to a contact.",
  "examples": [
    { "when": "attaching a contact to a company", "input": { "from_object": "contacts", "from_id": "1", "to_object": "companies", "to_id": "2" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": true
}"#
    .to_string();
    ToolMeta {
        name: ASSOCIATE_TOOL.to_string(),
        description: "Create a default association between two HubSpot CRM records.".to_string(),
        input_schema_json: input_schema,
        output_schema_json: output_schema,
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: aw_meta,
    }
}

/// All thirteen tool definitions the extension exposes.
#[must_use]
pub fn all_tools() -> Vec<ToolMeta> {
    vec![
        crud_tool(
            CONTACTS_TOOL,
            "contacts",
            "email, firstname, lastname, phone, company",
            "Create, search, get, or update HubSpot contacts. The Private App token is injected by the host and never returned.",
        ),
        crud_tool(
            DEALS_TOOL,
            "deals",
            "dealname, amount, dealstage, pipeline, closedate",
            "Create, search, get, or update HubSpot deals. The Private App token is injected by the host and never returned.",
        ),
        crud_tool(
            COMPANIES_TOOL,
            "companies",
            "name, domain, industry, city, phone",
            "Create, search, get, or update HubSpot companies. The Private App token is injected by the host and never returned.",
        ),
        crud_tool(
            TICKETS_TOOL,
            "tickets",
            "subject, content, hs_pipeline_stage, hs_ticket_priority",
            "Create, search, get, or update HubSpot support tickets. The Private App token is injected by the host and never returned.",
        ),
        crud_tool(
            NOTES_TOOL,
            "notes",
            "hs_note_body, hs_timestamp",
            "Create, search, get, or update HubSpot notes (activity logged to CRM records). The Private App token is injected by the host and never returned.",
        ),
        crud_tool(
            TASKS_TOOL,
            "tasks",
            "hs_task_subject, hs_task_body, hs_timestamp, hs_task_status",
            "Create, search, get, or update HubSpot tasks. The Private App token is injected by the host and never returned.",
        ),
        crud_tool(
            CALLS_TOOL,
            "calls",
            "hs_call_title, hs_call_body, hs_timestamp, hs_call_duration, hs_call_direction",
            "Create, search, get, or update HubSpot calls (activity logged to CRM records). The Private App token is injected by the host and never returned.",
        ),
        crud_tool(
            MEETINGS_TOOL,
            "meetings",
            "hs_meeting_title, hs_meeting_body, hs_timestamp, hs_meeting_start_time, hs_meeting_end_time",
            "Create, search, get, or update HubSpot meetings. The Private App token is injected by the host and never returned.",
        ),
        crud_tool(
            EMAILS_TOOL,
            "emails",
            "hs_email_subject, hs_email_text, hs_email_direction, hs_timestamp, hs_email_status",
            "Create, search, get, or update HubSpot emails (activity logged to CRM records). The Private App token is injected by the host and never returned.",
        ),
        pipelines_tool(),
        owners_tool(),
        batch_tool(),
        associate_tool(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_exactly_thirteen_tools_with_expected_names() {
        let names: Vec<String> = all_tools().into_iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            vec![
                CONTACTS_TOOL,
                DEALS_TOOL,
                COMPANIES_TOOL,
                TICKETS_TOOL,
                NOTES_TOOL,
                TASKS_TOOL,
                CALLS_TOOL,
                MEETINGS_TOOL,
                EMAILS_TOOL,
                PIPELINES_TOOL,
                OWNERS_TOOL,
                BATCH_TOOL,
                ASSOCIATE_TOOL,
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

    #[test]
    fn object_for_tool_maps_crud_only() {
        assert_eq!(object_for_tool(CONTACTS_TOOL), Some("contacts"));
        assert_eq!(object_for_tool(DEALS_TOOL), Some("deals"));
        assert_eq!(object_for_tool(COMPANIES_TOOL), Some("companies"));
        assert_eq!(object_for_tool(TICKETS_TOOL), Some("tickets"));
        assert_eq!(object_for_tool(NOTES_TOOL), Some("notes"));
        assert_eq!(object_for_tool(TASKS_TOOL), Some("tasks"));
        assert_eq!(object_for_tool(CALLS_TOOL), Some("calls"));
        assert_eq!(object_for_tool(MEETINGS_TOOL), Some("meetings"));
        assert_eq!(object_for_tool(EMAILS_TOOL), Some("emails"));
        assert_eq!(object_for_tool(PIPELINES_TOOL), None);
        assert_eq!(object_for_tool(OWNERS_TOOL), None);
        assert_eq!(object_for_tool(BATCH_TOOL), None);
        assert_eq!(object_for_tool(ASSOCIATE_TOOL), None);
        assert_eq!(object_for_tool("nope"), None);
    }
}
