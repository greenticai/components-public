//! `describe.json` is the *only* source of tool metadata for a `greentic.ai/v2`
//! extension — the runtime never calls the wasm's `list-tools` export. That
//! makes `contributions.tools[]` a second copy of what `tool_meta.rs` declares,
//! so these tests fail the build if the two ever drift apart.

use greentic_jev_extension::tool_meta;
use serde_json::Value;

fn describe() -> Value {
    let raw = include_str!("../describe.json");
    serde_json::from_str(raw).expect("describe.json is valid JSON")
}

fn decide_entry() -> Value {
    describe()["contributions"]["tools"]
        .as_array()
        .expect("contributions.tools is an array")
        .iter()
        .find(|tool| tool["name"] == tool_meta::DECIDE_TOOL)
        .expect("describe.json declares the decide tool")
        .clone()
}

/// Read a `Tool` field that the contract stores as a JSON Schema *serialized as
/// a string*, and parse it back. Fails loudly if the field was written as a JSON
/// object instead of a string — the most common way to break this file.
fn nested_json(entry: &Value, field: &str) -> Value {
    let raw = entry[field].as_str().unwrap_or_else(|| {
        panic!("describe.json: contributions.tools[].{field} must be a JSON string, not an object")
    });
    serde_json::from_str(raw).unwrap_or_else(|error| panic!("{field} is not valid JSON: {error}"))
}

#[test]
fn describe_declares_the_same_metadata_as_tool_meta() {
    let entry = decide_entry();
    let meta = tool_meta::decide_tool();

    assert_eq!(entry["description"], meta.description);
    assert_eq!(
        nested_json(&entry, "input_schema"),
        serde_json::from_str::<Value>(meta.input_schema_json).unwrap()
    );
    assert_eq!(entry["capabilities"], serde_json::json!(meta.capabilities));
}

/// `output_schema` and `agentic_worker_metadata` exist on the `Tool` struct only
/// on the SDK's unreleased `main`; the newest tagged contract
/// (`v1.3.0-research.1`, the ref `.github/workflows/validate-describe.yml` pins)
/// stops at `input_schema`. Because `Tool` is `#[serde(deny_unknown_fields)]`,
/// emitting either one here does not degrade gracefully — it fails the *whole*
/// describe parse and takes the extension down. `tool_meta.rs` still carries
/// both, and `list_tools()` still reports them over WIT; they may be added here
/// once a contract release contains them.
#[test]
fn describe_omits_fields_the_pinned_contract_cannot_decode() {
    let entry = decide_entry();
    assert!(entry.get("output_schema").is_none());
    assert!(entry.get("agentic_worker_metadata").is_none());
}

/// The input schema must describe the arguments `DecideInput` actually
/// deserializes: `state` and `questions` are required, `model` is optional.
#[test]
fn input_schema_matches_the_handler_arguments() {
    let schema = nested_json(&decide_entry(), "input_schema");

    let mut required: Vec<&str> = schema["required"]
        .as_array()
        .expect("required is an array")
        .iter()
        .map(|v| v.as_str().expect("required entries are strings"))
        .collect();
    required.sort_unstable();
    assert_eq!(required, ["questions", "state"]);

    let mut properties: Vec<&str> = schema["properties"]
        .as_object()
        .expect("properties is an object")
        .keys()
        .map(String::as_str)
        .collect();
    properties.sort_unstable();
    assert_eq!(properties, ["model", "questions", "state"]);
}
