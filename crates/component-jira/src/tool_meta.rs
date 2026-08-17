//! Static metadata for the Jira tools: names, JSON schemas, capability
//! flags, the `agentic_worker` metadata blob, and the secret requirements
//! each tool declares. Pure (no WIT imports) so it is fully host-testable;
//! `lib.rs` maps [`ToolMeta`] onto the WIT `ToolDefinition` shape (which has
//! no `secret_requirements` field — that list is consumed by the
//! `describe.json` authoring step, tasks A6-A13).
//!
//! This is the template the Jira tool domains extend: add a
//! `const <NAME>_TOOL` name, a builder function returning a [`ToolMeta`], and
//! push it onto the `vec![...]` in [`all_tools`]. Wire the matching
//! dispatch arm in `lib.rs::invoke_tool`.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
pub const ISSUES_TOOL: &str = "jira_issues";
pub const COMMENTS_TOOL: &str = "jira_comments";
pub const PROJECTS_TOOL: &str = "jira_projects";
pub const BOARDS_TOOL: &str = "jira_boards";
pub const SPRINTS_TOOL: &str = "jira_sprints";
pub const WORKLOGS_TOOL: &str = "jira_worklogs";
pub const ATTACHMENTS_TOOL: &str = "jira_attachments";
pub const USERS_TOOL: &str = "jira_users";

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

/// One secret the tool may read, surfaced to `describe.json` authoring
/// (tasks A6-A13) so the runtime can grant/prompt for it. Mirrors the shape
/// used in HubSpot's `describe.json` `contributions.tools[].secret_requirements`.
pub struct SecretRequirement {
    /// Secret key, without the `secret://` scheme (e.g. `"jira/email"`).
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

/// The seven `jira/*` secrets any Jira tool may read, depending on the
/// configured `auth_mode`. All optional: `required:false`, so a tenant can
/// configure either Token or OAuth mode without the other's secrets present.
fn jira_secret_requirements() -> Vec<SecretRequirement> {
    vec![
        secret(
            "jira/email",
            "Atlassian account email for HTTP Basic auth, used when auth_mode is token (the default). Resolved by the host from secret://jira/email and never returned to the model.",
        ),
        secret(
            "jira/api_token",
            "Jira API token paired with jira/email for HTTP Basic auth (token mode). Resolved by the host and never returned to the model.",
        ),
        secret(
            "jira/site",
            "Jira Cloud site name or host (e.g. \"acme\" or \"acme.atlassian.net\"), used to build the REST API base URL in token mode.",
        ),
        secret(
            "jira/auth_mode",
            "Auth strategy selector: \"oauth\" routes tokens through the platform OAuth broker (auto-refreshed) against api.atlassian.com; any other value or unset uses token (Basic auth) mode.",
        ),
        secret(
            "jira/oauth_refresh_token",
            "OAuth refresh token (brokerless OAuth mode). Stored by gtc setup after consent; the component refreshes the Jira access token itself. Used only when auth_mode=oauth.",
        ),
        secret(
            "jira/oauth_client_id",
            "Atlassian OAuth app Client ID (brokerless OAuth mode).",
        ),
        secret(
            "jira/oauth_client_secret",
            "Atlassian OAuth app Client Secret (brokerless OAuth mode); resolved by the host, never returned to the model.",
        ),
    ]
}

fn issues_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "search", "get", "update", "transition", "assign", "delete"], "description": "Which Jira issue action to perform." },
    "id": { "type": "string", "description": "Issue id or key (e.g. \"AB-123\"). Required for get, update, transition, assign, and delete." },
    "jql": { "type": "string", "description": "JQL query string. Required for search." },
    "fields": { "type": "object", "additionalProperties": true, "description": "Issue fields to set. Required for create (e.g. project, issuetype, summary) and update." },
    "transition_id": { "type": "string", "description": "Target workflow transition id. Required for transition." },
    "account_id": { "type": "string", "description": "Atlassian account id of the new assignee. Required for assign." },
    "max_results": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max search results (default Jira page size)." },
    "return_fields": { "type": "array", "items": { "type": "string" }, "description": "Field names to include in the response, for get and search." }
  }
}"#
    .to_string()
}

fn issues_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get/update: a single issue {id,key,summary,status,assignee,url?}. For search: {total,results:[{key,summary,status,assignee}]}. For delete/assign/transition: {ok,id}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "key": { "type": "string" },
    "summary": { "type": ["string", "null"] },
    "status": { "type": ["string", "null"] },
    "assignee": { "type": ["string", "null"] },
    "url": { "type": "string" },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" }
  }
}"#
    .to_string()
}

fn issues_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Jira issues. Set 'operation' to create, search, get, update, transition, assign, or delete. Search with JQL before creating to avoid duplicates; confirm with the user before delete.",
  "examples": [
    { "when": "looking up issues by project", "input": { "operation": "search", "jql": "project = AB ORDER BY created DESC" } },
    { "when": "fetching one issue by key", "input": { "operation": "get", "id": "AB-123" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn issues_tool() -> ToolMeta {
    ToolMeta {
        name: ISSUES_TOOL.to_string(),
        description:
            "Create, search, get, update, transition, assign, or delete Jira issues. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: issues_input_schema(),
        output_schema_json: issues_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: issues_agentic_worker_metadata(),
        secret_requirements: jira_secret_requirements(),
    }
}

fn comments_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["add", "list", "update", "delete"], "description": "Which Jira comment action to perform." },
    "id": { "type": "string", "description": "Issue id or key (e.g. \"AB-123\") the comment belongs to. Required for all operations." },
    "comment_id": { "type": "string", "description": "Comment id. Required for update and delete." },
    "body": { "description": "Comment text: a plain string or a full Atlassian Document Format (ADF) object, passed through to Jira as-is. Required for add and update.", "type": ["string", "object"] }
  }
}"#
    .to_string()
}

fn comments_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For add/update: a single comment {id,author,body,created}. For list: {total,results:[{id,author,body,created}]}. For delete: {ok,id}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "author": { "type": ["string", "null"] },
    "body": {},
    "created": { "type": ["string", "null"] },
    "total": { "type": ["integer", "null"] },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" }
  }
}"#
    .to_string()
}

fn comments_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage comments on a Jira issue. Set 'operation' to add, list, update, or delete. List existing comments before adding a duplicate; confirm with the user before delete.",
  "examples": [
    { "when": "adding a comment to an issue", "input": { "operation": "add", "id": "AB-123", "body": "Fixed in the latest deploy." } },
    { "when": "listing comments on an issue", "input": { "operation": "list", "id": "AB-123" } }
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
            "Add, list, update, or delete comments on a Jira issue. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: comments_input_schema(),
        output_schema_json: comments_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: comments_agentic_worker_metadata(),
        secret_requirements: jira_secret_requirements(),
    }
}

fn projects_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get"], "description": "Which Jira project action to perform." },
    "id": { "type": "string", "description": "Project id or key (e.g. \"AB\"). Required for get." },
    "query": { "type": "string", "description": "Free-text filter on project name/key, for list." },
    "max_results": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list (default Jira page size)." }
  }
}"#
    .to_string()
}

fn projects_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get: a single project {id,key,name,lead?}. For list: {total,results:[{id,key,name,lead?}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "key": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "lead": { "type": "string" },
    "total": { "type": ["integer", "null"] },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn projects_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Look up Jira projects. Set 'operation' to list (optionally filtered by 'query') or get (by 'id', a project id or key).",
  "examples": [
    { "when": "finding a project by name", "input": { "operation": "list", "query": "acme" } },
    { "when": "fetching one project by key", "input": { "operation": "get", "id": "AB" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn projects_tool() -> ToolMeta {
    ToolMeta {
        name: PROJECTS_TOOL.to_string(),
        description:
            "List or get Jira projects. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: projects_input_schema(),
        output_schema_json: projects_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: projects_agentic_worker_metadata(),
        secret_requirements: jira_secret_requirements(),
    }
}

fn boards_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get"], "description": "Which Jira Software board action to perform." },
    "id": { "type": "string", "description": "Board id. Required for get." },
    "project_key_or_id": { "type": "string", "description": "Filter boards by project key or id, for list." },
    "max_results": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for list (default Jira page size)." }
  }
}"#
    .to_string()
}

fn boards_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get: a single board {id,name,type,projectKey?}. For list: {total,results:[{id,name,type,projectKey?}]}.",
  "properties": {
    "id": { "type": ["string", "integer", "null"] },
    "name": { "type": ["string", "null"] },
    "type": { "type": ["string", "null"] },
    "projectKey": { "type": "string" },
    "total": { "type": ["integer", "null"] },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn boards_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Look up Jira Software agile boards. Set 'operation' to list (optionally filtered by 'project_key_or_id') or get (by 'id', a board id).",
  "examples": [
    { "when": "finding boards for a project", "input": { "operation": "list", "project_key_or_id": "AB" } },
    { "when": "fetching one board by id", "input": { "operation": "get", "id": "1" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn boards_tool() -> ToolMeta {
    ToolMeta {
        name: BOARDS_TOOL.to_string(),
        description:
            "List or get Jira Software agile boards. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: boards_input_schema(),
        output_schema_json: boards_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: boards_agentic_worker_metadata(),
        secret_requirements: jira_secret_requirements(),
    }
}

fn sprints_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get", "create", "move_issues"], "description": "Which Jira Software sprint action to perform." },
    "id": { "type": "string", "description": "Sprint id. Required for get and move_issues." },
    "board_id": { "type": "string", "description": "Board id whose sprints to list. Required for list." },
    "state": { "type": "string", "enum": ["future", "active", "closed"], "description": "Filter sprints by state, for list." },
    "name": { "type": "string", "description": "Sprint name. Required for create." },
    "origin_board_id": { "type": "string", "description": "Board id the new sprint belongs to. Required for create." },
    "start_date": { "type": "string", "description": "ISO-8601 sprint start date, for create." },
    "end_date": { "type": "string", "description": "ISO-8601 sprint end date, for create." },
    "issues": { "type": "array", "items": { "type": "string" }, "description": "Issue ids/keys to move into the sprint. Required for move_issues." }
  }
}"#
    .to_string()
}

fn sprints_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get/create: a single sprint {id,name,state,startDate?,endDate?}. For list: {total,results:[{id,name,state,startDate?,endDate?}]}. For move_issues: {ok,moved}.",
  "properties": {
    "id": { "type": ["string", "integer", "null"] },
    "name": { "type": ["string", "null"] },
    "state": { "type": ["string", "null"] },
    "startDate": { "type": "string" },
    "endDate": { "type": "string" },
    "total": { "type": ["integer", "null"] },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" },
    "moved": { "type": ["integer", "null"] }
  }
}"#
    .to_string()
}

fn sprints_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Jira Software sprints. Set 'operation' to list (by 'board_id'), get (by 'id'), create (needs 'name' and 'origin_board_id'), or move_issues (needs 'id' and 'issues'). Confirm with the user before create or move_issues.",
  "examples": [
    { "when": "listing active sprints on a board", "input": { "operation": "list", "board_id": "1", "state": "active" } },
    { "when": "moving issues into a sprint", "input": { "operation": "move_issues", "id": "37", "issues": ["AB-1", "AB-2"] } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn sprints_tool() -> ToolMeta {
    ToolMeta {
        name: SPRINTS_TOOL.to_string(),
        description:
            "List, get, create Jira Software sprints, or move issues into a sprint. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: sprints_input_schema(),
        output_schema_json: sprints_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: sprints_agentic_worker_metadata(),
        secret_requirements: jira_secret_requirements(),
    }
}

fn worklogs_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["add", "list"], "description": "Which Jira worklog action to perform." },
    "id": { "type": "string", "description": "Issue id or key (e.g. \"AB-123\") the worklog belongs to. Required for add and list." },
    "time_spent": { "type": "string", "description": "Jira worklog duration string (e.g. \"3h 30m\"). Required for add." },
    "comment": { "description": "Optional worklog comment, for add. A plain string is automatically wrapped into a minimal Atlassian Document Format (ADF) paragraph before being sent, since Jira REST v3 requires worklog comments to be ADF; a full ADF object is accepted as-is and passed through unchanged.", "type": ["string", "object"] },
    "started": { "type": "string", "description": "Optional ISO-8601 timestamp the work started, for add." }
  }
}"#
    .to_string()
}

fn worklogs_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For add: a single worklog {id,author,timeSpentSeconds,started}. For list: {total,results:[{id,author,timeSpentSeconds,started}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "author": { "type": ["string", "null"] },
    "timeSpentSeconds": { "type": ["integer", "null"] },
    "started": { "type": ["string", "null"] },
    "total": { "type": ["integer", "null"] },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn worklogs_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Log or list work logged against a Jira issue. Set 'operation' to add (needs 'id' and 'time_spent') or list (needs 'id').",
  "examples": [
    { "when": "logging time spent on an issue", "input": { "operation": "add", "id": "AB-123", "time_spent": "3h 30m", "comment": "Investigated the regression." } },
    { "when": "listing worklogs on an issue", "input": { "operation": "list", "id": "AB-123" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn worklogs_tool() -> ToolMeta {
    ToolMeta {
        name: WORKLOGS_TOOL.to_string(),
        description:
            "Log or list work logged against a Jira issue. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: worklogs_input_schema(),
        output_schema_json: worklogs_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: worklogs_agentic_worker_metadata(),
        secret_requirements: jira_secret_requirements(),
    }
}

fn attachments_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["add", "list"], "description": "Which Jira attachment action to perform. Note: file upload (add) is not supported by this extension; list only." },
    "id": { "type": "string", "description": "Issue id or key (e.g. \"AB-123\") the attachment belongs to. Required for add and list." }
  }
}"#
    .to_string()
}

fn attachments_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For list: {total,results:[{id,filename,size,mimeType,url}]}. add always fails: this extension cannot express a multipart file upload.",
  "properties": {
    "total": { "type": ["integer", "null"] },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn attachments_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "List attachments on a Jira issue. Set 'operation' to list (needs 'id'). Note: file upload is not supported by this extension; direct the user to attach files via the Jira UI or API directly.",
  "examples": [
    { "when": "listing attachments on an issue", "input": { "operation": "list", "id": "AB-123" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn attachments_tool() -> ToolMeta {
    ToolMeta {
        name: ATTACHMENTS_TOOL.to_string(),
        description:
            "List attachments on a Jira issue. File upload is not supported by this extension (Jira's upload endpoint requires multipart/form-data, which this host's HTTP call cannot express) — attach files via the Jira UI or API directly. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: attachments_input_schema(),
        output_schema_json: attachments_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: attachments_agentic_worker_metadata(),
        secret_requirements: jira_secret_requirements(),
    }
}

fn users_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation", "query"],
  "properties": {
    "operation": { "type": "string", "enum": ["search"], "description": "Which Jira user action to perform." },
    "query": { "type": "string", "description": "Free-text filter on user name/email. Required for search." },
    "max_results": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results to return, for search (default Jira page size)." }
  }
}"#
    .to_string()
}

fn users_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For search: {total,results:[{accountId,displayName,emailAddress?,active}]}.",
  "properties": {
    "total": { "type": ["integer", "null"] },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn users_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Search Jira users by name or email. Set 'operation' to search with 'query'; use the returned 'accountId' for assign or comment mentions.",
  "examples": [
    { "when": "finding a user to assign an issue to", "input": { "operation": "search", "query": "jane" } }
  ],
  "side_effects": "read",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn users_tool() -> ToolMeta {
    ToolMeta {
        name: USERS_TOOL.to_string(),
        description:
            "Search Jira users by name or email. The auth token is injected by the host and never returned."
                .to_string(),
        input_schema_json: users_input_schema(),
        output_schema_json: users_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: users_agentic_worker_metadata(),
        secret_requirements: jira_secret_requirements(),
    }
}

/// All tool definitions the extension exposes.
#[must_use]
pub fn all_tools() -> Vec<ToolMeta> {
    vec![
        issues_tool(),
        comments_tool(),
        projects_tool(),
        boards_tool(),
        sprints_tool(),
        worklogs_tool(),
        attachments_tool(),
        users_tool(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_all_eight_jira_tools() {
        let names: Vec<String> = all_tools().into_iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            vec![
                ISSUES_TOOL,
                COMMENTS_TOOL,
                PROJECTS_TOOL,
                BOARDS_TOOL,
                SPRINTS_TOOL,
                WORKLOGS_TOOL,
                ATTACHMENTS_TOOL,
                USERS_TOOL,
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
    fn every_tool_declares_seven_optional_jira_secrets() {
        for tool in all_tools() {
            assert_eq!(tool.secret_requirements.len(), 7);
            assert!(tool.secret_requirements.iter().all(|req| !req.required));
            let keys: Vec<&str> = tool
                .secret_requirements
                .iter()
                .map(|req| req.key.as_str())
                .collect();
            assert_eq!(
                keys,
                vec![
                    "jira/email",
                    "jira/api_token",
                    "jira/site",
                    "jira/auth_mode",
                    "jira/oauth_refresh_token",
                    "jira/oauth_client_id",
                    "jira/oauth_client_secret",
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
