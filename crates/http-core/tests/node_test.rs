use http_core::{ComponentConfig, NodeBuilder};
use serde_json::json;

#[test]
fn builds_post_http_node_from_config_and_inputs() {
    let cfg = ComponentConfig {
        base_url: Some("https://api.example.com".into()),
        auth_type: "bearer".into(),
        auth_token: Some("secret:CRM_TOKEN".into()),
        timeout_ms: 15000,
        default_headers: Some(json!({"Content-Type": "application/json"})),
        ..Default::default()
    };
    let node = NodeBuilder::new(
        "post_to_crm",
        "oci://ghcr.io/greenticai/component/component-http:0.1.0",
    )
    .with_config(cfg)
    .with_input("method", "POST")
    .with_input("path", "/api/leads")
    .with_rationale("bearer auth chosen from intent")
    .build();
    let j = serde_json::to_value(&node).unwrap();

    assert_eq!(j["node_id"], "post_to_crm");
    assert_eq!(
        j["component"],
        "oci://ghcr.io/greenticai/component/component-http:0.1.0"
    );
    assert_eq!(j["config"]["base_url"], "https://api.example.com");
    assert_eq!(j["config"]["auth_type"], "bearer");
    assert_eq!(j["config"]["timeout_ms"], 15000);
    assert_eq!(j["inputs"]["method"], "POST");
    assert_eq!(j["inputs"]["path"], "/api/leads");
    assert_eq!(j["rationale"], "bearer auth chosen from intent");
}

#[test]
fn output_is_deterministic() {
    let cfg = ComponentConfig {
        base_url: Some("https://x".into()),
        ..Default::default()
    };
    let n1 = NodeBuilder::new("n", "oci://r:1")
        .with_config(cfg.clone())
        .build();
    let n2 = NodeBuilder::new("n", "oci://r:1").with_config(cfg).build();
    let s1 = serde_json::to_string(&n1).unwrap();
    let s2 = serde_json::to_string(&n2).unwrap();
    assert_eq!(s1, s2);
}
