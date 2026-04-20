use http_extension::tools::generate::generate_http_node;
use serde_json::json;

#[test]
fn generates_node_with_intent_only() {
    let args = json!({
        "intent": "POST to CRM /api/leads with JSON body, bearer auth",
        "context": {
            "base_url_hint": "https://crm.example.com",
            "secret_names": ["CRM_TOKEN"]
        }
    });
    let out = generate_http_node(&args).expect("generate ok");
    let j = serde_json::from_str::<serde_json::Value>(&out).unwrap();
    assert_eq!(j["config"]["base_url"], "https://crm.example.com");
    assert_eq!(j["config"]["auth_type"], "bearer");
    assert_eq!(j["config"]["auth_token"], "secret:CRM_TOKEN");
    assert_eq!(j["inputs"]["method"], "POST");
    assert!(
        j["component"]
            .as_str()
            .unwrap()
            .starts_with("oci://ghcr.io/greenticai/component/component-http:")
    );
    assert!(j["rationale"].is_string());
}

#[test]
fn falls_back_to_generic_secret_name_when_none_provided() {
    let args = json!({ "intent": "GET from api.example.com with bearer", "context": {} });
    let out = generate_http_node(&args).expect("ok");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(j["config"]["auth_token"], "secret:HTTP_TOKEN");
}

#[test]
fn rejects_missing_intent() {
    let args = json!({ "context": {} });
    let err = generate_http_node(&args).expect_err("must fail");
    assert!(err.contains("intent"));
}
