//! Static metadata for the ClickUp tools: names, JSON schemas, capability
//! flags, the `agentic_worker` metadata blob, and the secret requirements
//! each tool declares. Pure (no WIT imports) so it is fully host-testable;
//! `lib.rs` maps [`ToolMeta`] onto the WIT `ToolDefinition` shape (which has
//! no `secret_requirements` field — that list is consumed by the
//! `describe.json` authoring step, a later task).
//!
//! Batch 1 (task B1b) added `clickup_tasks`, `clickup_spaces`,
//! `clickup_folders`, `clickup_lists`. Batch 2 (task B1c) adds
//! `clickup_comments`, `clickup_time_entries`, `clickup_custom_fields`,
//! `clickup_members`, completing the eight ClickUp tools. This is the
//! template: add a `const <NAME>_TOOL` name, a builder function returning a
//! [`ToolMeta`], and push it onto the `vec![...]` in [`all_tools`]. Wire the
//! matching dispatch arm in `lib.rs::invoke_tool`. Mirrors
//! `component-jira-ext`'s `src/tool_meta.rs`.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
pub const TASKS_TOOL: &str = "clickup_tasks";
pub const SPACES_TOOL: &str = "clickup_spaces";
pub const FOLDERS_TOOL: &str = "clickup_folders";
pub const LISTS_TOOL: &str = "clickup_lists";
pub const COMMENTS_TOOL: &str = "clickup_comments";
pub const TIME_ENTRIES_TOOL: &str = "clickup_time_entries";
pub const CUSTOM_FIELDS_TOOL: &str = "clickup_custom_fields";
pub const MEMBERS_TOOL: &str = "clickup_members";

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
/// used in Jira's and HubSpot's `describe.json`
/// `contributions.tools[].secret_requirements`.
pub struct SecretRequirement {
    /// Secret key, without the `secret://` scheme (e.g. `"clickup/token"`).
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

/// The three `clickup/*` secrets any ClickUp tool may read, depending on
/// the configured `auth_mode`. All optional: `required:false`, so a tenant
/// can configure either Token or OAuth mode without the other's secrets
/// present.
fn clickup_secret_requirements() -> Vec<SecretRequirement> {
    vec![
        secret(
            "clickup/token",
            "ClickUp personal API token, used when auth_mode is token (the default). Sent unmodified in the Authorization header (no Bearer prefix). Resolved by the host from secret://clickup/token and never returned to the model.",
        ),
        secret(
            "clickup/auth_mode",
            "Auth strategy selector: \"oauth\" routes tokens through the platform OAuth broker (provider \"clickup\"); any other value or unset uses token mode.",
        ),
        secret(
            "clickup/oauth_access_token",
            "Fallback OAuth access token, stored by gtc setup after the consent flow. Used only when auth_mode=oauth and the broker call fails; ClickUp OAuth access tokens do not expire and are issued without a refresh token, so this is a static fallback, not a brokerless refresh. Resolved by the host and never returned to the model.",
        ),
    ]
}

fn tasks_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "get", "update", "delete", "search"], "description": "Which ClickUp task action to perform." },
    "list_id": { "type": "string", "description": "List id the task belongs to. Required for create and search." },
    "task_id": { "type": "string", "description": "Task id. Required for get, update, and delete." },
    "fields": { "type": "object", "additionalProperties": true, "description": "Task fields to set, e.g. {name, description, assignees, status, priority, due_date}. Required for create and update; passed through to ClickUp as the request body." },
    "page": { "type": "integer", "minimum": 0, "description": "Zero-based page number, for search." },
    "statuses": { "type": "array", "items": { "type": "string" }, "description": "Filter results to these status names, for search." },
    "include_closed": { "type": "boolean", "description": "Include closed tasks in results, for search." }
  }
}"#
    .to_string()
}

fn tasks_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get/update: a single task {id,name,status,url,list_id?}. For search: {total,results:[{id,name,status}]}. For delete: {ok,id}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "status": { "type": ["string", "null"] },
    "url": { "type": ["string", "null"] },
    "list_id": { "type": "string" },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" }
  }
}"#
    .to_string()
}

fn tasks_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage ClickUp tasks. Set 'operation' to create, get, update, delete, or search. Search a list before creating to avoid duplicates; confirm with the user before delete.",
  "examples": [
    { "when": "creating a task in a list", "input": { "operation": "create", "list_id": "124", "fields": { "name": "Ship the release" } } },
    { "when": "searching open tasks in a list", "input": { "operation": "search", "list_id": "124", "statuses": ["open"] } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn tasks_tool() -> ToolMeta {
    ToolMeta {
        name: TASKS_TOOL.to_string(),
        description:
            "Create, get, update, delete, or search ClickUp tasks. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: tasks_input_schema(),
        output_schema_json: tasks_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: tasks_agentic_worker_metadata(),
        secret_requirements: clickup_secret_requirements(),
    }
}

fn spaces_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get"], "description": "Which ClickUp space action to perform." },
    "team_id": { "type": "string", "description": "Workspace (team) id whose spaces to list. Required for list." },
    "space_id": { "type": "string", "description": "Space id. Required for get." }
  }
}"#
    .to_string()
}

fn spaces_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get: a single space {id,name,private?}. For list: {total,results:[{id,name,private?}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "private": { "type": "boolean" },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn spaces_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Look up ClickUp spaces. Set 'operation' to list (by 'team_id') or get (by 'space_id').",
  "examples": [
    { "when": "listing spaces in a workspace", "input": { "operation": "list", "team_id": "1" } },
    { "when": "fetching one space by id", "input": { "operation": "get", "space_id": "90" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn spaces_tool() -> ToolMeta {
    ToolMeta {
        name: SPACES_TOOL.to_string(),
        description:
            "List or get ClickUp spaces. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: spaces_input_schema(),
        output_schema_json: spaces_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: spaces_agentic_worker_metadata(),
        secret_requirements: clickup_secret_requirements(),
    }
}

fn folders_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get", "create"], "description": "Which ClickUp folder action to perform." },
    "space_id": { "type": "string", "description": "Space id. Required for list and create." },
    "folder_id": { "type": "string", "description": "Folder id. Required for get." },
    "fields": { "type": "object", "additionalProperties": true, "description": "Folder fields to set, e.g. {name}. Required for create; passed through to ClickUp as the request body." }
  }
}"#
    .to_string()
}

fn folders_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get/create: a single folder {id,name,space_id?}. For list: {total,results:[{id,name,space_id?}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "space_id": { "type": "string" },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn folders_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage ClickUp folders. Set 'operation' to list (by 'space_id'), get (by 'folder_id'), or create (needs 'space_id' and 'fields.name').",
  "examples": [
    { "when": "listing folders in a space", "input": { "operation": "list", "space_id": "90" } },
    { "when": "creating a folder in a space", "input": { "operation": "create", "space_id": "90", "fields": { "name": "Sprint 12" } } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn folders_tool() -> ToolMeta {
    ToolMeta {
        name: FOLDERS_TOOL.to_string(),
        description:
            "List, get, or create ClickUp folders. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: folders_input_schema(),
        output_schema_json: folders_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: folders_agentic_worker_metadata(),
        secret_requirements: clickup_secret_requirements(),
    }
}

fn lists_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get", "create"], "description": "Which ClickUp list action to perform." },
    "folder_id": { "type": "string", "description": "Folder id. Required for list and create." },
    "list_id": { "type": "string", "description": "List id. Required for get." },
    "fields": { "type": "object", "additionalProperties": true, "description": "List fields to set, e.g. {name}. Required for create; passed through to ClickUp as the request body." }
  }
}"#
    .to_string()
}

fn lists_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get/create: a single list {id,name,folder_id?}. For list: {total,results:[{id,name,folder_id?}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "folder_id": { "type": "string" },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn lists_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage ClickUp lists. Set 'operation' to list (by 'folder_id'), get (by 'list_id'), or create (needs 'folder_id' and 'fields.name').",
  "examples": [
    { "when": "listing lists in a folder", "input": { "operation": "list", "folder_id": "457" } },
    { "when": "creating a list in a folder", "input": { "operation": "create", "folder_id": "457", "fields": { "name": "Backlog" } } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn lists_tool() -> ToolMeta {
    ToolMeta {
        name: LISTS_TOOL.to_string(),
        description:
            "List, get, or create ClickUp lists. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: lists_input_schema(),
        output_schema_json: lists_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: lists_agentic_worker_metadata(),
        secret_requirements: clickup_secret_requirements(),
    }
}

fn comments_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["add", "list", "update"], "description": "Which ClickUp comment action to perform." },
    "task_id": { "type": "string", "description": "Task id the comment belongs to. Required for add and list." },
    "comment_id": { "type": "string", "description": "Comment id. Required for update." },
    "comment_text": { "type": "string", "description": "Comment body text. Required for add and update." },
    "notify_all": { "type": "boolean", "description": "Notify all task watchers of the new comment, for add." },
    "resolved": { "type": "boolean", "description": "Mark the comment resolved or unresolved, for update." }
  }
}"#
    .to_string()
}

fn comments_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For add/update: a single comment {id,user,comment_text,date}. For list: {total,results:[{id,user,comment_text,date}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "user": {},
    "comment_text": { "type": ["string", "null"] },
    "date": { "type": ["string", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn comments_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage ClickUp task comments. Set 'operation' to add, list, or update. List the task's comments before adding to avoid duplicates.",
  "examples": [
    { "when": "adding a comment to a task", "input": { "operation": "add", "task_id": "9hz", "comment_text": "Shipped in v1.2." } },
    { "when": "listing comments on a task", "input": { "operation": "list", "task_id": "9hz" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn comments_tool() -> ToolMeta {
    ToolMeta {
        name: COMMENTS_TOOL.to_string(),
        description:
            "Add, list, or update ClickUp task comments. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: comments_input_schema(),
        output_schema_json: comments_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: comments_agentic_worker_metadata(),
        secret_requirements: clickup_secret_requirements(),
    }
}

fn time_entries_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["start", "stop", "list", "add"], "description": "Which ClickUp time-tracking action to perform." },
    "team_id": { "type": "string", "description": "Workspace (team) id. Required for start, stop, list, and add." },
    "tid": { "type": "string", "description": "Task id to associate the time entry with, for start and add." },
    "description": { "type": "string", "description": "Time entry description, for start and add." },
    "start": { "description": "Entry start time (Unix ms timestamp). Required for add." },
    "duration": { "description": "Entry duration in milliseconds. Required for add." }
  }
}"#
    .to_string()
}

fn time_entries_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For start/add: a single time entry {id,task_id?,start,duration}. For list: {total,results:[{id,task_id?,start,duration}]}. For stop: {ok,id}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "task_id": { "type": "string" },
    "start": {},
    "duration": {},
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" }
  }
}"#
    .to_string()
}

fn time_entries_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Track ClickUp time entries. Set 'operation' to start (with 'team_id', optional 'tid'), stop (with 'team_id'), list (with 'team_id'), or add (with 'team_id','start','duration').",
  "examples": [
    { "when": "starting a timer for a task", "input": { "operation": "start", "team_id": "1", "tid": "9hz" } },
    { "when": "stopping the running timer", "input": { "operation": "stop", "team_id": "1" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn time_entries_tool() -> ToolMeta {
    ToolMeta {
        name: TIME_ENTRIES_TOOL.to_string(),
        description:
            "Start, stop, list, or add ClickUp time entries. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: time_entries_input_schema(),
        output_schema_json: time_entries_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: time_entries_agentic_worker_metadata(),
        secret_requirements: clickup_secret_requirements(),
    }
}

fn custom_fields_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["get", "set"], "description": "Which ClickUp custom field action to perform." },
    "list_id": { "type": "string", "description": "List id whose custom fields to fetch. Required for get." },
    "task_id": { "type": "string", "description": "Task id to set a custom field value on. Required for set." },
    "field_id": { "type": "string", "description": "Custom field id. Required for set." },
    "value": { "description": "Custom field value to set. Required for set; any JSON type." }
  }
}"#
    .to_string()
}

fn custom_fields_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get: {total,results:[{id,name,type}]}. For set: {ok,id}.",
  "properties": {
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" },
    "id": { "type": ["string", "null"] }
  }
}"#
    .to_string()
}

fn custom_fields_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Read or set ClickUp custom fields. Set 'operation' to get (by 'list_id', to discover field ids) or set (needs 'task_id','field_id','value').",
  "examples": [
    { "when": "listing a list's custom fields", "input": { "operation": "get", "list_id": "124" } },
    { "when": "setting a custom field value on a task", "input": { "operation": "set", "task_id": "9hz", "field_id": "f1", "value": "high" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn custom_fields_tool() -> ToolMeta {
    ToolMeta {
        name: CUSTOM_FIELDS_TOOL.to_string(),
        description:
            "Get a list's custom fields or set a custom field value on a task. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: custom_fields_input_schema(),
        output_schema_json: custom_fields_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: custom_fields_agentic_worker_metadata(),
        secret_requirements: clickup_secret_requirements(),
    }
}

fn members_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list"], "description": "Which ClickUp member action to perform." },
    "list_id": { "type": "string", "description": "List id whose members to fetch. Required for list." }
  }
}"#
    .to_string()
}

fn members_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For list: {total,results:[{id,username,email?}]}.",
  "properties": {
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn members_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Look up members of a ClickUp list. Set 'operation' to list (by 'list_id') to find assignable users.",
  "examples": [
    { "when": "listing members of a list", "input": { "operation": "list", "list_id": "124" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn members_tool() -> ToolMeta {
    ToolMeta {
        name: MEMBERS_TOOL.to_string(),
        description:
            "List the members of a ClickUp list. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: members_input_schema(),
        output_schema_json: members_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: members_agentic_worker_metadata(),
        secret_requirements: clickup_secret_requirements(),
    }
}

/// All tool definitions the extension exposes. Batch 1 (task B1b): tasks,
/// spaces, folders, lists. Batch 2 (task B1c): comments, time_entries,
/// custom_fields, members — completes the eight ClickUp tools.
#[must_use]
pub fn all_tools() -> Vec<ToolMeta> {
    vec![
        tasks_tool(),
        spaces_tool(),
        folders_tool(),
        lists_tool(),
        comments_tool(),
        time_entries_tool(),
        custom_fields_tool(),
        members_tool(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_all_eight_clickup_tools() {
        let names: Vec<String> = all_tools().into_iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            vec![
                TASKS_TOOL,
                SPACES_TOOL,
                FOLDERS_TOOL,
                LISTS_TOOL,
                COMMENTS_TOOL,
                TIME_ENTRIES_TOOL,
                CUSTOM_FIELDS_TOOL,
                MEMBERS_TOOL,
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
    fn every_tool_declares_three_optional_clickup_secrets() {
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
                    "clickup/token",
                    "clickup/auth_mode",
                    "clickup/oauth_access_token",
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
