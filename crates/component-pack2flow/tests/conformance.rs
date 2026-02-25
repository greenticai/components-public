use component_pack2flow::{describe_payload, handle_message};

#[test]
fn describe_mentions_world() {
    let payload = describe_payload();
    let json: serde_json::Value = serde_json::from_str(&payload).expect("describe should be json");
    assert_eq!(
        json["component"]["world"],
        "greentic:component/component-v0-v6-v0@0.6.0"
    );
}

#[test]
fn handle_returns_transfer_contract() {
    let input = serde_json::json!({ "target": { "flow": "flow-b" } });

    let response = handle_message("handle_message", &input.to_string());
    let json: serde_json::Value = serde_json::from_str(&response).expect("response should be json");

    assert_eq!(json["greentic_control"]["v"], 1);
    assert_eq!(json["greentic_control"]["action"], "jump");
    assert_eq!(json["greentic_control"]["target"]["flow"], "flow-b");
    assert_eq!(
        json["greentic_control"]["target"]["node"],
        serde_json::Value::Null
    );
}
