use greentic_extension_sdk_contract::DescribeJson;
use serde_json::Value;

fn schema_for(type_id: &str) -> Value {
    let d: DescribeJson = serde_json::from_str(include_str!("../describe.json")).unwrap();
    let nt = d.contributions.node_types.iter().find(|n| n.type_id == type_id).unwrap();
    serde_json::from_str(nt.config_schema.as_str()).expect("config_schema is valid JSON")
}

fn required(schema: &Value) -> Vec<String> {
    schema["required"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

#[test]
fn timer_requires_enabled() {
    let s = schema_for("timer-trigger");
    assert_eq!(s["type"], "object");
    assert!(required(&s).contains(&"enabled".to_string()));
}

#[test]
fn sms_requires_messaging_provider_id() {
    let s = schema_for("sms-trigger");
    let req = required(&s);
    assert!(req.contains(&"messaging_provider_id".to_string()));
    // Credentials are delegated to the messaging provider — no secret fields here
    assert!(s["properties"]["messaging_provider_id"].is_object());
    assert!(s["properties"]["from"].is_object());
    assert!(s["properties"]["persistence_key_prefix"].is_object());
}

#[test]
fn email_requires_messaging_provider_id() {
    let s = schema_for("email-trigger");
    let req = required(&s);
    assert!(req.contains(&"messaging_provider_id".to_string()));
}
