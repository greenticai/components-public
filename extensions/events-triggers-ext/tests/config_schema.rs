use greentic_extension_sdk_contract::DescribeJson;
use serde_json::Value;

fn schema_for(type_id: &str) -> Value {
    let d: DescribeJson = serde_json::from_str(include_str!("../describe.json")).unwrap();
    let nt = d.contributions.node_types.iter().find(|n| n.type_id == type_id).unwrap();
    serde_json::from_str(nt.config_schema.as_str()).expect("config_schema is valid JSON")
}

fn required(schema: &Value) -> Vec<String> {
    schema["required"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default()
}

#[test]
fn timer_requires_schedule() {
    let s = schema_for("timer-trigger");
    assert_eq!(s["type"], "object");
    assert!(required(&s).contains(&"schedule".to_string()));
}

#[test]
fn sms_requires_from_and_secret_refs() {
    let s = schema_for("sms-trigger");
    let req = required(&s);
    assert!(req.contains(&"from_number".to_string()));
    assert!(req.contains(&"account_sid".to_string()));
    assert!(req.contains(&"auth_token".to_string()));
    // secret fields describe a secret_ref, not a raw value
    assert!(s["properties"]["account_sid"]["description"].as_str().unwrap().to_lowercase().contains("secret"));
}

#[test]
fn email_requires_from_and_api_key_secret() {
    let s = schema_for("email-trigger");
    let req = required(&s);
    assert!(req.contains(&"from_address".to_string()));
    assert!(req.contains(&"api_key".to_string()));
}
