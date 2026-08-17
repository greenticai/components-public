//! Static metadata for the Trello tools: names, JSON schemas, capability
//! flags, the `agentic_worker` metadata blob, and the secret requirements
//! each tool declares. Pure (no WIT imports) so it is fully host-testable;
//! `lib.rs` maps [`ToolMeta`] onto the WIT `ToolDefinition` shape (which has
//! no `secret_requirements` field — that list is consumed by the
//! `describe.json` authoring step).
//!
//! This is the template the remaining Trello tool domains extend: add a
//! `const <NAME>_TOOL` name, a builder function returning a [`ToolMeta`],
//! and push it onto the `vec![...]` in [`all_tools`]. Wire the matching
//! dispatch arm in `lib.rs::invoke_tool`.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several structs exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
pub const CARDS_TOOL: &str = "trello_cards";
pub const LISTS_TOOL: &str = "trello_lists";
pub const BOARDS_TOOL: &str = "trello_boards";
pub const CHECKLISTS_TOOL: &str = "trello_checklists";
pub const LABELS_TOOL: &str = "trello_labels";
pub const COMMENTS_TOOL: &str = "trello_comments";
pub const ATTACHMENTS_TOOL: &str = "trello_attachments";
pub const MEMBERS_TOOL: &str = "trello_members";

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

/// One secret the tool may read, surfaced to `describe.json` authoring so
/// the runtime can grant/prompt for it. Mirrors the shape used in
/// HubSpot's `describe.json` `contributions.tools[].secret_requirements`.
pub struct SecretRequirement {
    /// Secret key, without the `secret://` scheme (e.g. `"trello/api_key"`).
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

/// The two `trello/*` secrets every Trello tool reads: the `key`/`token`
/// query-auth pair (Trello's single auth mode). Both optional
/// (`required:false`) at the schema level — the extension itself fails
/// closed with `PermissionDenied` at call time if either is unresolvable.
#[must_use]
pub fn trello_secret_requirements() -> Vec<SecretRequirement> {
    vec![
        secret(
            "trello/api_key",
            "Trello API key, sent as the `key` query parameter on every request. Resolved by the host from secret://trello/api_key and never returned to the model.",
        ),
        secret(
            "trello/token",
            "Trello API token, sent as the `token` query parameter on every request. Resolved by the host from secret://trello/token and never returned to the model.",
        ),
    ]
}

fn cards_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "get", "update", "move", "archive", "delete"], "description": "Which Trello card action to perform." },
    "card_id": { "type": "string", "description": "Card id. Required for get, update, move, archive, and delete." },
    "list_id": { "type": "string", "description": "List id. Required for create (the card's initial list) and move (the destination list); optional for update (moves the card)." },
    "name": { "type": "string", "description": "Card name/title, for create and update." },
    "desc": { "type": "string", "description": "Card description, for create and update." },
    "pos": { "description": "Card position: \"top\", \"bottom\", or a positive number. For create and update.", "type": ["string", "number"] }
  }
}"#
    .to_string()
}

fn cards_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/get/update: a single card {id,name,idList,closed,url}. For move/archive/delete: {ok,id}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "idList": { "type": ["string", "null"] },
    "closed": { "type": ["boolean", "null"] },
    "url": { "type": "string" },
    "ok": { "type": "boolean" }
  }
}"#
    .to_string()
}

fn cards_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Trello cards. Set 'operation' to create, get, update, move, archive, or delete. Confirm with the user before archive or delete.",
  "examples": [
    { "when": "creating a card on a list", "input": { "operation": "create", "list_id": "L1", "name": "Ship the release" } },
    { "when": "moving a card to another list", "input": { "operation": "move", "card_id": "C1", "list_id": "L2" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn cards_tool() -> ToolMeta {
    ToolMeta {
        name: CARDS_TOOL.to_string(),
        description:
            "Create, get, update, move, archive, or delete Trello cards. The key/token auth is injected by the host and never returned."
                .to_string(),
        input_schema_json: cards_input_schema(),
        output_schema_json: cards_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: cards_agentic_worker_metadata(),
        secret_requirements: trello_secret_requirements(),
    }
}

fn lists_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "create", "update", "archive"], "description": "Which Trello list action to perform." },
    "board_id": { "type": "string", "description": "Board id. Required for list (the board whose lists to fetch) and create (the list's board)." },
    "list_id": { "type": "string", "description": "List id. Required for update and archive." },
    "name": { "type": "string", "description": "List name, for create and update." },
    "closed": { "type": "boolean", "description": "Set the list's closed state, for update." }
  }
}"#
    .to_string()
}

fn lists_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/update: a single list {id,name,idBoard,closed}. For list: {total,results:[{id,name,idBoard,closed}]}. For archive: {ok,id}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "idBoard": { "type": ["string", "null"] },
    "closed": { "type": ["boolean", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" }
  }
}"#
    .to_string()
}

fn lists_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Trello lists. Set 'operation' to list (by 'board_id'), create (needs 'board_id'), update, or archive. Confirm with the user before archive.",
  "examples": [
    { "when": "fetching a board's lists", "input": { "operation": "list", "board_id": "B1" } },
    { "when": "creating a new list on a board", "input": { "operation": "create", "board_id": "B1", "name": "In Review" } }
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
            "List, create, update, or archive Trello lists. The key/token auth is injected by the host and never returned."
                .to_string(),
        input_schema_json: lists_input_schema(),
        output_schema_json: lists_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: lists_agentic_worker_metadata(),
        secret_requirements: trello_secret_requirements(),
    }
}

fn boards_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "get", "create"], "description": "Which Trello board action to perform." },
    "board_id": { "type": "string", "description": "Board id. Required for get." },
    "name": { "type": "string", "description": "Board name, for create." }
  }
}"#
    .to_string()
}

fn boards_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For get/create: a single board {id,name,url,closed}. For list: {total,results:[{id,name,url,closed}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "url": { "type": ["string", "null"] },
    "closed": { "type": ["boolean", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn boards_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Look up or create Trello boards. Set 'operation' to list (the caller's boards), get (by 'board_id'), or create (needs 'name').",
  "examples": [
    { "when": "listing the caller's boards", "input": { "operation": "list" } },
    { "when": "fetching one board by id", "input": { "operation": "get", "board_id": "B1" } }
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
            "List, get, or create Trello boards. The key/token auth is injected by the host and never returned."
                .to_string(),
        input_schema_json: boards_input_schema(),
        output_schema_json: boards_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: boards_agentic_worker_metadata(),
        secret_requirements: trello_secret_requirements(),
    }
}

fn checklists_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["create", "add_item", "update_item"], "description": "Which Trello checklist action to perform." },
    "card_id": { "type": "string", "description": "Card id. Required for create (the checklist's card) and update_item." },
    "checklist_id": { "type": "string", "description": "Checklist id. Required for add_item." },
    "checkitem_id": { "type": "string", "description": "Checklist item id. Required for update_item." },
    "name": { "type": "string", "description": "Checklist name (create) or checklist item name (add_item, update_item)." },
    "state": { "type": "string", "enum": ["complete", "incomplete"], "description": "Checklist item completion state, for update_item." }
  }
}"#
    .to_string()
}

fn checklists_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For create/add_item: a single checklist or checklist item {id,name,state?}. For update_item: {ok,id}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "state": { "type": "string" },
    "ok": { "type": "boolean" }
  }
}"#
    .to_string()
}

fn checklists_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Trello checklists on a card. Set 'operation' to create (needs 'card_id'), add_item (needs 'checklist_id' and 'name'), or update_item (needs 'card_id' and 'checkitem_id').",
  "examples": [
    { "when": "adding a checklist to a card", "input": { "operation": "create", "card_id": "C1", "name": "Release steps" } },
    { "when": "checking off an item", "input": { "operation": "update_item", "card_id": "C1", "checkitem_id": "CI1", "state": "complete" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn checklists_tool() -> ToolMeta {
    ToolMeta {
        name: CHECKLISTS_TOOL.to_string(),
        description:
            "Create Trello checklists, add checklist items, or update a checklist item's name/state. The key/token auth is injected by the host and never returned."
                .to_string(),
        input_schema_json: checklists_input_schema(),
        output_schema_json: checklists_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: checklists_agentic_worker_metadata(),
        secret_requirements: trello_secret_requirements(),
    }
}

fn labels_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["list", "add", "remove"], "description": "Which Trello label action to perform." },
    "board_id": { "type": "string", "description": "Board id. Required for list (the board whose labels to fetch)." },
    "card_id": { "type": "string", "description": "Card id. Required for add and remove." },
    "label_id": { "type": "string", "description": "Label id. Required for add and remove." }
  }
}"#
    .to_string()
}

fn labels_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For list: {total,results:[{id,name,color}]}. For add/remove: {ok,id} where id is the card_id.",
  "properties": {
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" },
    "id": { "type": ["string", "null"] }
  }
}"#
    .to_string()
}

fn labels_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Manage Trello labels. Set 'operation' to list (by 'board_id'), add (needs 'card_id' and 'label_id'), or remove (needs 'card_id' and 'label_id').",
  "examples": [
    { "when": "fetching a board's labels", "input": { "operation": "list", "board_id": "B1" } },
    { "when": "tagging a card with a label", "input": { "operation": "add", "card_id": "C1", "label_id": "LB1" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn labels_tool() -> ToolMeta {
    ToolMeta {
        name: LABELS_TOOL.to_string(),
        description:
            "List a board's Trello labels, or add/remove a label on a card. The key/token auth is injected by the host and never returned."
                .to_string(),
        input_schema_json: labels_input_schema(),
        output_schema_json: labels_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: labels_agentic_worker_metadata(),
        secret_requirements: trello_secret_requirements(),
    }
}

fn comments_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["add", "list"], "description": "Which Trello comment action to perform." },
    "card_id": { "type": "string", "description": "Card id. Required for add and list." },
    "text": { "type": "string", "description": "Comment body text. Required for add." }
  }
}"#
    .to_string()
}

fn comments_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For add: a single comment {id,text,memberCreator,date}. For list: {total,results:[{id,text,memberCreator,date}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "text": { "type": ["string", "null"] },
    "memberCreator": { "type": ["object", "null"] },
    "date": { "type": ["string", "null"] },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn comments_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Add or list comments on a Trello card. Set 'operation' to add (needs 'card_id' and 'text') or list (needs 'card_id').",
  "examples": [
    { "when": "commenting on a card", "input": { "operation": "add", "card_id": "C1", "text": "Looks good to ship." } },
    { "when": "reading a card's comment history", "input": { "operation": "list", "card_id": "C1" } }
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
            "Add a comment to a Trello card, or list a card's comments. The key/token auth is injected by the host and never returned."
                .to_string(),
        input_schema_json: comments_input_schema(),
        output_schema_json: comments_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: comments_agentic_worker_metadata(),
        secret_requirements: trello_secret_requirements(),
    }
}

fn attachments_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["add", "list"], "description": "Which Trello attachment action to perform." },
    "card_id": { "type": "string", "description": "Card id. Required for add and list." },
    "url": { "type": "string", "description": "URL of the file to attach. Required for add. Only URL-based attachments are supported; file upload is not." },
    "name": { "type": "string", "description": "Display name for the attachment, for add." }
  }
}"#
    .to_string()
}

fn attachments_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For add: a single attachment {id,name,url,bytes?}. For list: {total,results:[{id,name,url,bytes?}]}.",
  "properties": {
    "id": { "type": ["string", "null"] },
    "name": { "type": ["string", "null"] },
    "url": { "type": ["string", "null"] },
    "bytes": { "type": "integer" },
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } }
  }
}"#
    .to_string()
}

fn attachments_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Add a URL attachment to a Trello card, or list a card's attachments. Set 'operation' to add (needs 'card_id' and 'url') or list (needs 'card_id'). File upload is not supported — only URL attachments.",
  "examples": [
    { "when": "attaching a link to a card", "input": { "operation": "add", "card_id": "C1", "url": "https://example.com/spec.pdf", "name": "Spec" } },
    { "when": "listing a card's attachments", "input": { "operation": "list", "card_id": "C1" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn attachments_tool() -> ToolMeta {
    ToolMeta {
        name: ATTACHMENTS_TOOL.to_string(),
        description:
            "Add a URL attachment to a Trello card, or list a card's attachments. File upload is not supported — only URL-based attachments. The key/token auth is injected by the host and never returned."
                .to_string(),
        input_schema_json: attachments_input_schema(),
        output_schema_json: attachments_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: attachments_agentic_worker_metadata(),
        secret_requirements: trello_secret_requirements(),
    }
}

fn members_input_schema() -> String {
    r#"{
  "type": "object",
  "required": ["operation"],
  "properties": {
    "operation": { "type": "string", "enum": ["search", "assign"], "description": "Which Trello member action to perform." },
    "query": { "type": "string", "description": "Search text. Required for search." },
    "card_id": { "type": "string", "description": "Card id. Required for assign." },
    "member_id": { "type": "string", "description": "Member id. Required for assign." }
  }
}"#
    .to_string()
}

fn members_output_schema() -> String {
    r#"{
  "type": "object",
  "description": "For search: {total,results:[{id,username,fullName}]}. For assign: {ok,id} where id is the card_id.",
  "properties": {
    "total": { "type": "integer" },
    "results": { "type": "array", "items": { "type": "object" } },
    "ok": { "type": "boolean" },
    "id": { "type": ["string", "null"] }
  }
}"#
    .to_string()
}

fn members_agentic_worker_metadata() -> String {
    r#"{
  "usage_hint": "Search Trello members or assign one to a card. Set 'operation' to search (needs 'query') or assign (needs 'card_id' and 'member_id').",
  "examples": [
    { "when": "finding a member to assign", "input": { "operation": "search", "query": "ada" } },
    { "when": "assigning a member to a card", "input": { "operation": "assign", "card_id": "C1", "member_id": "M1" } }
  ],
  "side_effects": "write",
  "cost": "low",
  "confirmation_required": false
}"#
    .to_string()
}

fn members_tool() -> ToolMeta {
    ToolMeta {
        name: MEMBERS_TOOL.to_string(),
        description:
            "Search Trello members, or assign a member to a card. The key/token auth is injected by the host and never returned."
                .to_string(),
        input_schema_json: members_input_schema(),
        output_schema_json: members_output_schema(),
        capabilities: vec!["agentic_worker".into()],
        agentic_worker_metadata: members_agentic_worker_metadata(),
        secret_requirements: trello_secret_requirements(),
    }
}

/// All tools exported by this extension. Batch 1: cards, lists, boards,
/// checklists. Batch 2: labels, comments, attachments, members.
#[must_use]
pub fn all_tools() -> Vec<ToolMeta> {
    vec![
        cards_tool(),
        lists_tool(),
        boards_tool(),
        checklists_tool(),
        labels_tool(),
        comments_tool(),
        attachments_tool(),
        members_tool(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_all_eight_trello_tools() {
        let names: Vec<String> = all_tools().into_iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            vec![
                CARDS_TOOL,
                LISTS_TOOL,
                BOARDS_TOOL,
                CHECKLISTS_TOOL,
                LABELS_TOOL,
                COMMENTS_TOOL,
                ATTACHMENTS_TOOL,
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
    fn every_tool_declares_two_optional_trello_secrets() {
        for tool in all_tools() {
            assert_eq!(tool.secret_requirements.len(), 2);
            assert!(tool.secret_requirements.iter().all(|req| !req.required));
            let keys: Vec<&str> = tool
                .secret_requirements
                .iter()
                .map(|req| req.key.as_str())
                .collect();
            assert_eq!(keys, vec!["trello/api_key", "trello/token"]);
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
