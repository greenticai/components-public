//! `suggest_path` — turn an intent into a trigger id and the route it mounts at.
//!
//! Under trigger contract v1 the runtime mounts every webhook trigger at
//! `<deployment-prefix>/trigger/<trigger_id>`; there is no free-form path
//! anymore. The tool keeps its name so existing prompts keep working, and
//! returns the `trigger_id` to put in the node config plus the route it yields.
//!
//! Examples:
//!   "Receive Stripe events"   → trigger_id `stripe_events`   → /trigger/stripe_events
//!   "GitHub PR opened"        → `github_pr_opened`
//!   "intake from Salesforce"  → `intake_salesforce`

use serde_json::{Value, json};

use super::validate::is_valid_trigger_id;

const NOISE_WORDS: &[&str] = &[
    "receive", "incoming", "for", "from", "the", "a", "an", "to", "into",
];
const MAX_LEN: usize = 63;

pub fn suggest_path(args: &Value) -> Result<String, String> {
    let intent = args
        .get("intent")
        .and_then(Value::as_str)
        .ok_or("missing required field: intent")?;

    let slug = slugify(intent);
    if slug.is_empty() || !is_valid_trigger_id(&slug) {
        return Err("intent produced an empty slug — provide more descriptive text".into());
    }
    Ok(json!({
        "trigger_id": slug,
        "path": format!("/trigger/{slug}"),
        "rationale": format!(
            "Slugified '{intent}'. The runtime mounts the trigger at \
             <deployment-prefix>/trigger/{slug}; set `trigger_id` on the node to keep this URL \
             stable if the node is renamed."
        ),
    })
    .to_string())
}

fn slugify(input: &str) -> String {
    let mut tokens: Vec<String> = input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect();
    if tokens.len() > 2 {
        tokens.retain(|t| !NOISE_WORDS.contains(&t.as_str()));
    }
    let mut slug = tokens.join("_");
    slug.truncate(MAX_LEN);
    slug
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    fn suggestion(intent: &str) -> Value {
        serde_json::from_str(&suggest_path(&json!({"intent": intent})).expect("ok")).unwrap()
    }

    #[test]
    fn slugifies_basic_intent_into_a_trigger_id_and_route() {
        let s = suggestion("Receive Stripe events");
        assert_eq!(s["trigger_id"], "stripe_events");
        assert_eq!(s["path"], "/trigger/stripe_events");
    }

    #[test]
    fn handles_punctuation_and_case() {
        assert_eq!(
            suggestion("GitHub: PR Opened!")["trigger_id"],
            "github_pr_opened"
        );
    }

    #[test]
    fn drops_noise_words_only_when_intent_is_long() {
        assert_eq!(
            suggestion("intake from Salesforce")["trigger_id"],
            "intake_salesforce"
        );
        assert_eq!(suggestion("from slack")["trigger_id"], "from_slack");
    }

    #[test]
    fn an_old_style_absolute_path_is_slugified_not_passed_through() {
        // Free-form paths no longer exist; the route is always /trigger/<id>.
        assert_eq!(suggestion("/api/intake")["trigger_id"], "api_intake");
    }

    #[test]
    fn every_suggestion_is_a_valid_trigger_id() {
        let long = "x".repeat(200);
        for intent in ["Receive Stripe events", "9 lives", long.as_str()] {
            let id = suggestion(intent)["trigger_id"]
                .as_str()
                .unwrap()
                .to_string();
            assert!(is_valid_trigger_id(&id), "{id}");
        }
    }

    #[test]
    fn empty_intent_errors() {
        let err = suggest_path(&json!({"intent": "...!?"})).expect_err("must fail");
        assert!(err.contains("empty slug"));
    }

    #[test]
    fn missing_intent_errors() {
        let err = suggest_path(&json!({})).expect_err("must fail");
        assert!(err.contains("intent"));
    }
}
