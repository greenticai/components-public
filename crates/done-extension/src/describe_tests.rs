//! What `describe.json` must keep saying, checked against the file itself.
//!
//! A describe is read by every designer that loads this extension, and the typed
//! reader (`greentic_extension_sdk_contract::DescribeJson`) is
//! `#[serde(deny_unknown_fields)]` from the root down through `Contributions`,
//! `NodeType` and `Tool`. A key that looks additive is therefore an UNLOAD on
//! every designer that does not know it. The trigger kind rides inside
//! `config_schema` as `x-trigger-kind` for exactly that reason.
//!
//! The rest of these tests pin the joins that fail silently: a node type's
//! `operation` must name the tool that declares its secrets, the secret keys
//! must produce the field names the component reads, and a node's config must
//! avoid the keys the runner's envelope parser consumes.

use serde_json::Value;

const DESCRIBE: &str = include_str!("../describe.json");

/// The designer's `inject::component_exec` rewrites `input.*` keys the runner's
/// `InvocationPayload::parse` consumes as an envelope. A component input with
/// any of these at the top level has its siblings silently re-parented.
const RESERVED_INPUT_KEYS: &[&str] = &["operation", "input", "config", "payload", "envelope"];

fn describe() -> Value {
    serde_json::from_str(DESCRIBE).expect("describe.json is valid JSON")
}

fn node<'a>(d: &'a Value, type_id: &str) -> &'a Value {
    d["contributions"]["nodeTypes"]
        .as_array()
        .expect("nodeTypes")
        .iter()
        .find(|n| n["type_id"] == type_id)
        .unwrap_or_else(|| panic!("node type {type_id}"))
}

fn schema_of(n: &Value) -> Value {
    serde_json::from_str(
        n["config_schema"]
            .as_str()
            .expect("config_schema is a string"),
    )
    .expect("config_schema is JSON")
}

fn tool<'a>(d: &'a Value, name: &str) -> &'a Value {
    d["contributions"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("tool {name}"))
}

#[test]
fn the_typed_describe_reader_accepts_the_file() {
    let parsed: Result<greentic_extension_sdk_contract::DescribeJson, _> =
        serde_json::from_str(DESCRIBE);
    if let Err(e) = parsed {
        panic!("the contract crate refuses describe.json, so every designer would unload it: {e}");
    }
}

#[test]
fn a_key_outside_the_contract_is_what_the_typed_reader_refuses() {
    // Proves the test above has teeth: add the key the trigger contract tempts
    // you to add and the typed reader must say no.
    let mut d = describe();
    d["contributions"]["trigger_kinds"] = serde_json::json!({ "done.event": "webhook" });
    let parsed: Result<greentic_extension_sdk_contract::DescribeJson, _> =
        serde_json::from_value(d);
    assert!(
        parsed.is_err(),
        "a `contributions.trigger_kinds` key must unload the extension"
    );
}

#[test]
fn contributions_declares_only_keys_the_contract_knows() {
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
    for key in describe()["contributions"].as_object().unwrap().keys() {
        assert!(
            KNOWN.contains(&key.as_str()),
            "`contributions.{key}` would unload the extension"
        );
    }
}

#[test]
fn the_declared_version_is_the_crate_version() {
    assert_eq!(describe()["metadata"]["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn the_event_trigger_declares_its_kind_inside_config_schema() {
    let d = describe();
    let n = node(&d, "done.event");
    assert_eq!(n["category"], "trigger");
    assert!(
        n.get("runtime_ref").is_none(),
        "a trigger is not a step and has no component"
    );
    assert_eq!(schema_of(n)["x-trigger-kind"], "webhook");
}

#[test]
fn the_event_trigger_is_prefilled_to_verify_done_events_and_offers_no_unverified_mode() {
    let d = describe();
    let schema = schema_of(node(&d, "done.event"));
    let verify = &schema["properties"]["verify"];
    assert_eq!(schema["required"], serde_json::json!(["verify"]));
    assert_eq!(
        verify["default"],
        serde_json::json!({
            "scheme": "hmac-sha256", "header": "X-Done-Signature", "prefix": "sha256=",
            "encoding": "hex", "secret_ref": "done/webhook_secret"
        })
    );
    assert_eq!(
        verify["properties"]["scheme"]["enum"],
        serde_json::json!(["hmac-sha256"]),
        "scheme `none` must not be offered (spec 7.3: no unauthenticated mode)"
    );
    assert_eq!(
        schema["properties"]["methods"]["default"],
        serde_json::json!(["POST"])
    );
    assert_eq!(
        schema["properties"]["idempotency"]["default"]["key"],
        "body.event.id"
    );
    assert_eq!(
        verify["properties"]["secret_ref"]["pattern"],
        "^[a-z0-9][a-z0-9_.-]*/[a-z0-9][a-z0-9_.-]*$",
        "must stay equal to the designer's is_valid_secret_ref"
    );
}

#[test]
fn the_trigger_schema_uses_the_top_level_shape_the_designer_emitter_reads() {
    let d = describe();
    let schema = schema_of(node(&d, "done.event"));
    let props = schema["properties"].as_object().unwrap();
    assert!(
        !props.contains_key("webhook"),
        "the emitted shape is not the config shape"
    );
    for field in [
        "trigger_id",
        "enabled",
        "methods",
        "verify",
        "idempotency",
        "session",
        "limits",
    ] {
        assert!(props.contains_key(field), "`{field}` missing");
    }
}

#[test]
fn step_nodes_name_the_tool_that_declares_their_secrets() {
    let d = describe();
    for type_id in ["done.create_request", "done.read_request"] {
        let n = node(&d, type_id);
        let op = n["operation"].as_str().unwrap();
        // The designer joins operation -> tool by NAME to find the secrets.
        assert_eq!(op, "done_api");
        assert!(
            tool(&d, op)["secret_requirements"]
                .as_array()
                .is_some_and(|s| !s.is_empty())
        );
        assert_eq!(n["runtime_ref"], "done-node");
        assert!(
            d["runtime"]["components"]["done-node"]["oci_ref"]
                .as_str()
                .unwrap()
                .starts_with("oci://")
        );
    }
}

#[test]
fn the_secret_keys_become_the_input_fields_the_component_reads() {
    // add_secret_refs writes `secret:<key>` into the field named by the key with
    // its provider prefix dropped. The component reads `api_key` and `base_url`.
    let d = describe();
    let fields: Vec<String> = tool(&d, "done_api")["secret_requirements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            s["key"]
                .as_str()
                .unwrap()
                .split_once('/')
                .unwrap()
                .1
                .to_string()
        })
        .collect();
    assert_eq!(fields, ["api_key", "base_url", "webhook_secret"]);
    let keys: Vec<&str> = tool(&d, "done_api")["secret_requirements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, crate::tools::SECRET_KEYS);
}

#[test]
fn only_the_webhook_secret_is_optional() {
    let d = describe();
    for s in tool(&d, "done_api")["secret_requirements"]
        .as_array()
        .unwrap()
    {
        let required = s["required"].as_bool().unwrap();
        assert_eq!(required, s["key"] != "done/webhook_secret", "{}", s["key"]);
    }
}

#[test]
fn no_node_config_field_collides_with_the_runner_envelope_keys() {
    let d = describe();
    for type_id in ["done.create_request", "done.read_request"] {
        let schema = schema_of(node(&d, type_id));
        for key in schema["properties"].as_object().unwrap().keys() {
            assert!(
                !RESERVED_INPUT_KEYS.contains(&key.as_str()),
                "{type_id}.{key} is a reserved key"
            );
        }
    }
}

#[test]
fn each_step_carries_a_constant_action_the_component_understands() {
    let d = describe();
    for (type_id, action) in [
        ("done.create_request", "create_request"),
        ("done.read_request", "read_request"),
    ] {
        let prop = &schema_of(node(&d, type_id))["properties"]["action"];
        assert_eq!(prop["const"], action);
        assert_eq!(prop["default"], action);
        assert_eq!(
            component_done::action_of(&serde_json::json!({ "action": action })),
            Some(action)
        );
    }
}

#[test]
fn steps_have_a_success_and_a_failure_port() {
    let d = describe();
    for type_id in ["done.create_request", "done.read_request"] {
        let ports: Vec<&str> = node(&d, type_id)["output_ports"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(ports, ["on_complete", "on_error"]);
    }
}

#[test]
fn the_tool_matches_the_wasm_and_omits_fields_the_pinned_contract_cannot_decode() {
    let d = describe();
    let t = tool(&d, "done_api");
    assert_eq!(t["input_schema"], crate::tools::INPUT_SCHEMA);
    serde_json::from_str::<Value>(crate::tools::INPUT_SCHEMA).expect("input schema is JSON");
    assert_eq!(t["capabilities"], serde_json::json!(["agentic_worker"]));
    assert!(t.get("output_schema").is_none());
    assert!(t.get("agentic_worker_metadata").is_none());
}

#[test]
fn the_component_reference_is_the_documented_unpublished_placeholder() {
    // The node component is pulled by digest from OCI and has not been
    // published. build.sh refuses to package a RELEASE while this is true.
    let d = describe();
    let c = &d["runtime"]["components"]["done-node"];
    assert!(c["oci_ref"].as_str().unwrap().ends_with(&"0".repeat(64)));
    assert_eq!(c["sha256"], "0".repeat(64));
}

#[test]
fn the_ask_and_react_template_pairs_its_flows_by_trigger_id() {
    // Templates are YAML under ../../templates; the pairing rule is checked there.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/ask-and-react");
    let a = std::fs::read_to_string(dir.join("flow-a-ask.recipe.yaml")).expect("flow A");
    let b = std::fs::read_to_string(dir.join("flow-b-react.recipe.yaml")).expect("flow B");
    let d = describe();
    for flow in [&a, &b] {
        for type_id in ["done.create_request", "done.event", "done.read_request"] {
            let _ = (flow, node(&d, type_id)); // every capability named below exists
        }
    }
    assert!(a.contains("capability: done.create_request"));
    assert!(a.contains("callback_trigger: ask_and_react_events"));
    assert!(b.contains("capability: done.event"));
    assert!(b.contains("trigger_id: ask_and_react_events"));
}
