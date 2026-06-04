//! `generate_from_card_submit` — map Adaptive Card Input.* ids to HTTP body template.

use super::runtime_component_ref;
use http_core::{ComponentConfig, NodeBuilder};
use serde_json::{Value, json};

pub fn generate_from_card_submit(args: &Value) -> Result<String, String> {
    let card = args
        .get("card_schema")
        .ok_or("missing required field: card_schema")?;
    let intent = args
        .get("api_intent")
        .and_then(Value::as_str)
        .ok_or("missing required field: api_intent")?;
    let node_id = args
        .get("node_id")
        .and_then(Value::as_str)
        .unwrap_or("http_call")
        .to_string();

    let input_ids = extract_input_ids(card);
    let (method, path) = parse_intent(intent);

    // Build JSON body template mapping each Input.id to ${submit.<id>}
    let mut body_obj: Vec<(String, String)> = input_ids
        .iter()
        .map(|id| (id.clone(), format!("${{submit.{id}}}")))
        .collect();
    body_obj.sort_by(|a, b| a.0.cmp(&b.0));
    let parts: Vec<String> = body_obj
        .iter()
        .map(|(k, v)| format!(r#""{k}":"{v}""#))
        .collect();
    let body_template = format!("{{{}}}", parts.join(","));

    // Mapping summary
    let mut card_to_body = serde_json::Map::new();
    let mut unmapped = Vec::new();
    for id in &input_ids {
        let low = id.to_lowercase();
        if ["internal", "debug", "note_private"]
            .iter()
            .any(|k| low.contains(k))
        {
            unmapped.push(format!("submit.{id}"));
        } else {
            card_to_body.insert(format!("submit.{id}"), json!(format!("body.{id}")));
        }
    }

    let cfg = ComponentConfig {
        base_url: Some("https://api.example.com".into()),
        auth_type: "bearer".into(),
        auth_token: Some("secret:HTTP_TOKEN".into()),
        default_headers: Some(json!({"Content-Type": "application/json"})),
        ..Default::default()
    };

    let node = NodeBuilder::new(node_id, runtime_component_ref())
        .with_config(cfg)
        .with_input("method", method)
        .with_input("path", path)
        .with_input("body_template", body_template)
        .with_mapping(json!({
            "card_to_body": Value::Object(card_to_body),
            "unmapped_card_fields": unmapped
        }))
        .with_rationale(format!(
            "Mapped {} card Input.* fields to body template.",
            input_ids.len()
        ))
        .build();

    serde_json::to_string(&node).map_err(|e| e.to_string())
}

fn extract_input_ids(card: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(card, &mut out);
    out
}
fn walk(v: &Value, out: &mut Vec<String>) {
    if let Some(arr) = v.as_array() {
        for item in arr {
            walk(item, out);
        }
        return;
    }
    if let Some(obj) = v.as_object() {
        let is_input = obj
            .get("type")
            .and_then(Value::as_str)
            .map(|s| s.starts_with("Input."))
            .unwrap_or(false);
        if is_input && let Some(id) = obj.get("id").and_then(Value::as_str) {
            out.push(id.to_string());
        }
        for (_, child) in obj {
            walk(child, out);
        }
    }
}

fn parse_intent(intent: &str) -> (&'static str, String) {
    let lower = intent.to_lowercase();
    let method = if lower.contains("post") {
        "POST"
    } else if lower.contains("put") {
        "PUT"
    } else if lower.contains("patch") {
        "PATCH"
    } else {
        "POST"
    };
    let path = intent
        .split_whitespace()
        .find(|t| t.starts_with('/'))
        .map(String::from)
        .unwrap_or_else(|| "/".into());
    (method, path)
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn maps_input_text_fields_into_body_template() {
        let card = json!({
            "type": "AdaptiveCard",
            "version": "1.6",
            "body": [
                { "type": "Input.Text", "id": "subject", "label": "Subject" },
                { "type": "Input.Text", "id": "description", "label": "Description" },
                { "type": "Input.ChoiceSet", "id": "priority", "choices": [{"title":"High","value":"high"}] }
            ]
        });
        let args = json!({
            "card_schema": card,
            "api_intent": "POST to /api/tickets with form fields as JSON",
            "node_id": "submit_ticket"
        });
        let out = generate_from_card_submit(&args).expect("ok");
        let j: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(j["node_id"], "submit_ticket");
        assert_eq!(j["inputs"]["method"], "POST");
        assert_eq!(j["inputs"]["path"], "/api/tickets");
        let tpl = j["inputs"]["body_template"].as_str().unwrap();
        assert!(tpl.contains("${submit.subject}"));
        assert!(tpl.contains("${submit.priority}"));
        assert!(tpl.contains("${submit.description}"));

        let map = &j["mapping"]["card_to_body"];
        assert_eq!(map["submit.subject"], "body.subject");
    }

    #[test]
    fn reports_unmapped_fields_when_names_differ() {
        let card = json!({
            "type": "AdaptiveCard",
            "body": [{ "type": "Input.Text", "id": "internal_note" }]
        });
        let args = json!({
            "card_schema": card,
            "api_intent": "POST to /api/tickets",
            "node_id": "submit"
        });
        let out = generate_from_card_submit(&args).expect("ok");
        let j: Value = serde_json::from_str(&out).unwrap();
        let unmapped = j["mapping"]["unmapped_card_fields"].as_array().unwrap();
        assert!(unmapped.iter().any(|v| v == "submit.internal_note"));
    }
}
