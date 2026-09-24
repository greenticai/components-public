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
        .find(|n| n["type_id"] == "trigger")
        .expect("the trigger node type")
}

#[test]
fn the_trigger_node_declares_its_contract_kind_inside_its_config_schema() {
    let describe = describe();
    let schema: Value = serde_json::from_str(
        trigger_node(&describe)["config_schema"]
            .as_str()
            .expect("config_schema is carried as a string"),
    )
    .expect("config_schema is valid JSON Schema");

    assert_eq!(
        schema["x-trigger-kind"], "webhook",
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
    // greentic-designer's `triggers_emit::collect` reads a webhook trigger's
    // settings from the TOP LEVEL of the node config and only then nests them
    // under a `webhook` object in the emitted `assets/triggers.json`. So the
    // shape an operator fills in and the shape the pack carries are different,
    // and a schema written to look like the emitted document — `webhook`
    // wrapping `verify`, `methods` and the rest — would render a form whose
    // every answer the emitter then refuses to find. Nothing would fail at
    // load: the extension keeps working, the node keeps rendering, and the
    // build drops the trigger with one warning.
    //
    // `trigger_id`, `enabled`, `session` and `limits` are read by `entry`;
    // the rest by `webhook_block`.
    let describe = describe();
    let schema: Value = serde_json::from_str(
        trigger_node(&describe)["config_schema"]
            .as_str()
            .expect("config_schema is carried as a string"),
    )
    .expect("config_schema is valid JSON Schema");

    let props = schema["properties"].as_object().expect("properties");
    for field in [
        "trigger_id",
        "enabled",
        "methods",
        "verify",
        "challenge",
        "idempotency",
        "max_body_bytes",
        "allowed_sources",
        "session",
        "limits",
    ] {
        assert!(props.contains_key(field), "`{field}` is not declared");
    }
    assert!(
        !props.contains_key("webhook"),
        "the webhook fields belong at the top level; a `webhook` object here is \
         the emitted shape, which the emitter cannot read as config"
    );
    assert_eq!(
        schema["required"],
        serde_json::json!(["verify"]),
        "the emitter refuses a trigger with no `verify` block rather than \
         defaulting it, so the schema must ask for one — `scheme: none` is how \
         an operator accepts unverified calls deliberately"
    );

    let verify = props["verify"]["properties"]
        .as_object()
        .expect("verify.properties");
    for field in ["scheme", "header", "prefix", "encoding", "secret_ref"] {
        assert!(
            verify.contains_key(field),
            "`verify.{field}` is read by the emitter and not declared"
        );
    }
    let challenge = props["challenge"]["properties"]
        .as_object()
        .expect("challenge.properties");
    for field in ["scheme", "verify_token_ref"] {
        assert!(
            challenge.contains_key(field),
            "`challenge.{field}` is read by the emitter and not declared"
        );
    }
    let idempotency = props["idempotency"]["properties"]
        .as_object()
        .expect("idempotency.properties");
    for field in ["key", "ttl_s"] {
        assert!(
            idempotency.contains_key(field),
            "`idempotency.{field}` is read by the emitter and not declared"
        );
    }
}

#[test]
fn every_secret_ref_field_declares_the_pattern_the_emitter_enforces() {
    // Both `*_ref` fields hold a secret NAME (`<provider>/<key>`) and never a
    // value — contract v1 §6.2, and the reason the emitter refuses anything
    // else WITHOUT echoing it, since the likeliest wrong answer is a pasted
    // credential and a build log is where it must not land.
    //
    // This pattern is the form's only chance to catch that before the build
    // does. It must stay equal to `triggers_emit::collect::is_valid_secret_ref`
    // in greentic-designer: exactly one `/`, each segment starting with a
    // lowercase letter or digit and continuing with those plus `_`, `.`, `-`.
    // A looser pattern accepts a value the build then refuses, which reads to
    // the operator as the build being wrong; a stricter one rejects a name that
    // would have worked.
    const PATTERN: &str = "^[a-z0-9][a-z0-9_.-]*/[a-z0-9][a-z0-9_.-]*$";

    let describe = describe();
    let schema: Value = serde_json::from_str(
        trigger_node(&describe)["config_schema"]
            .as_str()
            .expect("config_schema is carried as a string"),
    )
    .expect("config_schema is valid JSON Schema");
    let props = schema["properties"].as_object().expect("properties");

    assert_eq!(
        props["verify"]["properties"]["secret_ref"]["pattern"], PATTERN,
        "verify.secret_ref must declare the emitter's own pattern"
    );
    assert_eq!(
        props["challenge"]["properties"]["verify_token_ref"]["pattern"], PATTERN,
        "challenge.verify_token_ref must declare the emitter's own pattern"
    );
}
