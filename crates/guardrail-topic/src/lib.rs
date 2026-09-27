//! Greentic topic / scope guardrail design extension (deterministic, keyword-based).
//!
//! Exports the `guardrail` interface:
//! - **deny** an INBOUND message that is on-scope-checkable but matches none of
//!   the operator-configured `allowed_keywords` (an allow-list scope fence).
//! - **accept** otherwise (no config, short greetings, on-topic, or outbound).
//!
//! Config comes from `input.context` JSON: `{"allowed_keywords": ["order", ...]}`.
//! With no `allowed_keywords` the guardrail is a no-op (accepts everything), so
//! it is safe to attach before the operator fills the list. A smarter,
//! semantic scope check would require an LLM host import (out of scope here).
//!
//! The pure [`off_topic`] function is separately testable on the host.

#[allow(warnings)]
mod bindings;

use bindings::exports::greentic::extension_base::{lifecycle, manifest};
use bindings::exports::greentic::extension_design::guardrail::{
    self, DenyInfo, Direction, GuardrailInput, Verdict,
};
use bindings::greentic::extension_base::types;

pub struct Component;

// The export macro emits WIT symbols containing `@`, which the native linker
// rejects when the workspace builds this cdylib for the host (`cargo test
// --workspace --all-targets`). They only mean anything in the wasm component.
#[cfg(target_arch = "wasm32")]
bindings::export!(Component with_types_in bindings);

// ===== manifest =====

impl manifest::Guest for Component {
    fn get_identity() -> types::ExtensionIdentity {
        types::ExtensionIdentity {
            id: "greentic.guardrail-topic".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        vec![types::CapabilityRef {
            id: "greentic:guardrail/topic".into(),
            version: "1.0.0".into(),
        }]
    }

    fn get_required() -> Vec<types::CapabilityRef> {
        vec![]
    }
}

// ===== lifecycle =====

impl lifecycle::Guest for Component {
    fn init(_config_json: String) -> Result<(), types::ExtensionError> {
        Ok(())
    }

    fn shutdown() {}
}

// ===== guardrail =====

impl guardrail::Guest for Component {
    fn evaluate(input: GuardrailInput) -> Verdict {
        // Scope only constrains user-supplied INBOUND content.
        if !matches!(input.direction, Direction::Inbound) {
            return Verdict::Accept;
        }
        let allowed = allowed_keywords(input.context.as_deref());
        if off_topic(&input.content, &allowed) {
            Verdict::Deny(DenyInfo {
                code: "permission_denied".into(),
                message: "Request is outside this assistant's supported topics.".into(),
                details: None,
            })
        } else {
            Verdict::Accept
        }
    }
}

/// Short messages (greetings, acks) are never treated as off-topic.
const MIN_CHECK_LEN: usize = 12;

/// Parse `input.context` JSON for `{"allowed_keywords": [..]}` (lower-cased).
fn allowed_keywords(ctx_json: Option<&str>) -> Vec<String> {
    let Some(raw) = ctx_json else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    value
        .get("allowed_keywords")
        .and_then(|b| b.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.as_str().map(str::to_lowercase))
                .collect()
        })
        .unwrap_or_default()
}

/// `true` when the content should be denied as off-topic: an allow-list is
/// configured, the message is long enough to be a real query, and it contains
/// none of the allowed keywords. With no allow-list the guard is a no-op.
#[must_use]
pub fn off_topic(content: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return false;
    }
    if content.trim().len() < MIN_CHECK_LEN {
        return false;
    }
    let lower = content.to_lowercase();
    !allowed.iter().any(|kw| lower.contains(kw.as_str()))
}

#[cfg(test)]
mod tests {
    use super::off_topic;

    fn allowed() -> Vec<String> {
        ["order", "delivery", "return", "refund"]
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }

    #[test]
    fn denies_off_topic_query() {
        assert!(off_topic(
            "What do you think about the election results?",
            &allowed()
        ));
    }

    #[test]
    fn accepts_on_topic_and_greetings_and_no_config() {
        assert!(!off_topic("Where is my order #1234?", &allowed())); // on-topic
        assert!(!off_topic("Hi there", &allowed())); // too short
        assert!(!off_topic("anything goes here", &[])); // no allow-list -> no-op
    }
}
