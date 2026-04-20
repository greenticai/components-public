//! `generate_http_node` — produce a YGTc HTTP node stanza from natural language intent.

use http_core::{ComponentConfig, NodeBuilder};
use serde_json::Value;

use super::runtime_component_ref;

pub fn generate_http_node(args: &Value) -> Result<String, String> {
    let intent = args
        .get("intent")
        .and_then(Value::as_str)
        .ok_or("missing required field: intent")?;
    let context = args
        .get("context")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let base_url = context
        .get("base_url_hint")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| detect_url_from_intent(intent));
    let auth_type = detect_auth_type(intent);
    let method = detect_method(intent);
    let path = detect_path(intent).unwrap_or_default();

    let secret_names: Vec<String> = context
        .get("secret_names")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let token = pick_token(auth_type, &secret_names);
    let node_id = derive_node_id(intent, method);

    let mut cfg = ComponentConfig {
        base_url,
        auth_type: auth_type.to_string(),
        auth_token: token,
        ..Default::default()
    };
    if matches!(method, "POST" | "PUT" | "PATCH") {
        cfg.default_headers = Some(serde_json::json!({ "Content-Type": "application/json" }));
    }

    let node = NodeBuilder::new(node_id, runtime_component_ref())
        .with_config(cfg)
        .with_input("method", method)
        .with_input("path", path)
        .with_rationale(format!(
            "Auth={auth_type}, method={method}. Chose from intent keywords."
        ))
        .build();

    serde_json::to_string(&node).map_err(|e| e.to_string())
}

fn detect_auth_type(intent: &str) -> &'static str {
    let s = intent.to_lowercase();
    if s.contains("bearer") {
        "bearer"
    } else if s.contains("api key") || s.contains("api-key") {
        "api_key"
    } else if s.contains("basic auth") {
        "basic"
    } else {
        "none"
    }
}

fn detect_method(intent: &str) -> &'static str {
    let s = intent.to_lowercase();
    for m in ["POST", "PUT", "PATCH", "DELETE", "GET"] {
        if s.contains(&m.to_lowercase()) {
            return m;
        }
    }
    "GET"
}

fn detect_path(intent: &str) -> Option<String> {
    intent
        .split_whitespace()
        .find(|t| t.starts_with('/'))
        .map(String::from)
}

fn detect_url_from_intent(intent: &str) -> Option<String> {
    intent
        .split_whitespace()
        .find(|t| t.starts_with("http://") || t.starts_with("https://"))
        .map(|u| {
            if let Some(scheme_end) = u.find("://")
                && let Some(path_start) = u[scheme_end + 3..].find('/')
            {
                return u[..scheme_end + 3 + path_start].to_string();
            }
            u.to_string()
        })
}

fn pick_token(auth_type: &str, secret_names: &[String]) -> Option<String> {
    if auth_type == "none" {
        return None;
    }
    let preferred = secret_names.iter().find(|n| {
        let u = n.to_uppercase();
        u.contains("HTTP") || u.contains("TOKEN") || u.contains("API")
    });
    match preferred {
        Some(n) => Some(format!("secret:{n}")),
        None if secret_names.is_empty() => Some("secret:HTTP_TOKEN".into()),
        None => Some(format!("secret:{}", secret_names[0])),
    }
}

fn derive_node_id(intent: &str, method: &'static str) -> String {
    let verb = method.to_lowercase();
    let noun = intent
        .split_whitespace()
        .find(|t| {
            t.len() > 3
                && !t.starts_with('/')
                && !t.starts_with("http")
                && !t.to_lowercase().contains("bearer")
        })
        .unwrap_or("http")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    format!("{verb}_{noun}")
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
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
}
