//! `suggest_path` — slugify an intent string into a webhook path.
//!
//! Examples:
//!   "Receive Stripe events"            → /webhooks/stripe-events
//!   "GitHub PR opened"                 → /webhooks/github-pr-opened
//!   "/custom/path/already"             → /custom/path/already   (already absolute, untouched)
//!   "intake from Salesforce"           → /webhooks/intake-from-salesforce

use serde_json::{Value, json};

const NOISE_WORDS: &[&str] = &[
    "receive", "incoming", "for", "from", "the", "a", "an", "to", "into",
];
const PREFIX: &str = "/webhooks/";

pub fn suggest_path(args: &Value) -> Result<String, String> {
    let intent = args
        .get("intent")
        .and_then(Value::as_str)
        .ok_or("missing required field: intent")?;

    if intent.starts_with('/') && !intent.contains(char::is_whitespace) {
        return Ok(json!({"path": intent, "rationale": "Input already looks like an absolute path; passed through."}).to_string());
    }

    let slug = slugify(intent);
    if slug.is_empty() {
        return Err("intent produced an empty slug — provide more descriptive text".into());
    }
    let path = format!("{PREFIX}{slug}");
    Ok(json!({
        "path": path,
        "rationale": format!("Slugified '{intent}' under {PREFIX}* — adjust before publishing if you need a custom prefix."),
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
    tokens.join("-")
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    fn path_of(intent: &str) -> String {
        let raw = suggest_path(&json!({"intent": intent})).expect("ok");
        let v: Value = serde_json::from_str(&raw).unwrap();
        v["path"].as_str().unwrap().to_string()
    }

    #[test]
    fn slugifies_basic_intent() {
        assert_eq!(path_of("Receive Stripe events"), "/webhooks/stripe-events");
    }

    #[test]
    fn handles_punctuation_and_case() {
        assert_eq!(path_of("GitHub: PR Opened!"), "/webhooks/github-pr-opened");
    }

    #[test]
    fn drops_noise_words_only_when_intent_is_long() {
        // "from Salesforce" → 'from' is noise, drops it because >2 tokens
        assert_eq!(
            path_of("intake from Salesforce"),
            "/webhooks/intake-salesforce"
        );
    }

    #[test]
    fn keeps_short_intents_intact() {
        // 2-token intent should NOT have noise stripped (would empty it out otherwise)
        assert_eq!(path_of("from slack"), "/webhooks/from-slack");
    }

    #[test]
    fn passes_through_explicit_absolute_paths() {
        assert_eq!(path_of("/api/intake"), "/api/intake");
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
