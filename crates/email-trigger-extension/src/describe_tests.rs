//! What `describe.json` must keep saying, checked against the file itself.
//!
//! A describe is read by every designer that loads this extension, and the
//! typed reader (`greentic_extension_sdk_contract::DescribeJson`) is
//! `#[serde(deny_unknown_fields)]` from the root down through `Contributions`
//! and `NodeType`. So a key that looks additive is not: a designer that does
//! not know it refuses the whole describe and UNLOADS the extension. There is
//! no partial read and no warning — the extension simply stops existing for
//! that workspace.
//!
//! That is why the trigger contract's kind marker lives INSIDE `config_schema`
//! as the JSON Schema extension keyword `x-trigger-kind` rather than as a
//! `contributions.trigger_kinds` map: `config_schema` is a free-form schema
//! carried as a string, so the typed reader never looks inside it and every
//! schema validator is required to ignore an `x-` keyword it does not know.
//!
//! These tests exist because both halves fail SILENTLY in opposite directions:
//! a re-added `contributions` key breaks the extension everywhere at once, and
//! a dropped `x-trigger-kind` makes every designer go on loading the extension
//! while quietly declining to emit any trigger from it.

use serde_json::Value;

const DESCRIBE: &str = include_str!("../describe.json");

fn describe() -> Value {
    serde_json::from_str(DESCRIBE).expect("describe.json is valid JSON")
}

fn trigger_node(describe: &Value) -> &Value {
    describe["contributions"]["nodeTypes"]
        .as_array()
        .expect("nodeTypes array")
        .iter()
        .find(|n| n["type_id"] == "trigger.email")
        .expect("the trigger node type")
}

#[test]
fn the_email_node_declares_its_contract_kind_inside_its_config_schema() {
    let describe = describe();
    let schema: Value = serde_json::from_str(
        trigger_node(&describe)["config_schema"]
            .as_str()
            .expect("config_schema is carried as a string"),
    )
    .expect("config_schema is valid JSON Schema");

    assert_eq!(
        schema["x-trigger-kind"], "email_imap",
        "the designer reads the kind from this keyword; without it the node is \
         still refused as a step but no trigger is emitted for it"
    );
}

#[test]
fn contributions_declares_no_key_the_typed_describe_would_refuse() {
    // `Contributions` names these and nothing else. A new key here is not an
    // additive change — it is an unload on every designer that reads it.
    const KNOWN: &[&str] = &[
        "nodeTypes",
        "tools",
        "recipes",
        "knowledge",
        "prompts",
        "schemas",
        "dwProviders",
        "guardrails",
        "views",
        "connection_test",
        "messaging_channel",
    ];

    let describe = describe();
    for key in describe["contributions"]
        .as_object()
        .expect("contributions object")
        .keys()
    {
        assert!(
            KNOWN.contains(&key.as_str()),
            "`contributions.{key}` is not a field of the typed Contributions: \
             a designer would refuse this describe and unload the extension. \
             Carry the value inside `config_schema` instead."
        );
    }
}

#[test]
fn the_declared_version_is_the_crate_version() {
    // `build.sh` reads the version out of describe.json to name the artifact,
    // while the component reports `CARGO_PKG_VERSION` as its identity. The two
    // disagreeing ships a pack whose contents announce a different version
    // than its file name.
    assert_eq!(
        describe()["metadata"]["version"],
        env!("CARGO_PKG_VERSION"),
        "describe.json metadata.version and Cargo.toml version must move together"
    );
}

#[test]
fn the_config_schema_names_the_fields_the_designer_emitter_actually_reads() {
    // greentic-designer's `triggers_emit::email_block::email_imap_block` reads
    // these from the TOP level of the node config. A schema that nested them
    // under an `email_imap` object — how they look in the emitted
    // `assets/triggers.json` — would render a form whose answers the emitter
    // then refuses to find.
    let describe = describe();
    let schema: serde_json::Value = serde_json::from_str(
        trigger_node(&describe)["config_schema"]
            .as_str()
            .expect("config_schema is carried as a string"),
    )
    .expect("config_schema is valid JSON Schema");

    let props = schema["properties"].as_object().expect("properties");
    for field in [
        "host",
        "tls",
        "port",
        "mailbox",
        "username",
        "password_ref",
        "poll_interval_s",
        "search",
        "on_success",
        "move_to",
        "max_messages_per_poll",
        "max_message_bytes",
        "include_html",
        "attachments",
        "trigger_id",
        "enabled",
    ] {
        assert!(props.contains_key(field), "`{field}` is not declared");
    }
    assert!(!props.contains_key("email_imap"));
    assert_eq!(
        schema["required"],
        serde_json::json!(["host", "username", "password_ref"])
    );
}

#[test]
fn the_password_is_only_ever_a_secret_name() {
    // A field that held the password itself would put a credential in the
    // flow, the pack and every export of either.
    let describe = describe();
    let schema: serde_json::Value = serde_json::from_str(
        trigger_node(&describe)["config_schema"]
            .as_str()
            .expect("string"),
    )
    .expect("schema");
    let props = schema["properties"].as_object().expect("properties");
    assert!(!props.contains_key("password"));
    assert!(
        props["password_ref"]["description"]
            .as_str()
            .expect("description")
            .contains("NAME")
    );
}
