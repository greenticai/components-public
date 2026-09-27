//! Greentic profanity / toxicity guardrail design extension.
//!
//! Exports the `guardrail` interface:
//! - **update** (mask) profane words from a built-in list plus any extra words
//!   supplied via `input.context` JSON (`{"blocklist": [...]}`). Matched words
//!   are replaced with asterisks.
//! - **accept** otherwise.
//!
//! Applies in both directions. The pure [`mask_profanity`] function is
//! separately testable on the host.

#[allow(warnings)]
mod bindings;

use bindings::exports::greentic::extension_base::{lifecycle, manifest};
use bindings::exports::greentic::extension_design::guardrail::{self, GuardrailInput, Verdict};
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
            id: "greentic.guardrail-profanity".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        vec![types::CapabilityRef {
            id: "greentic:guardrail/profanity".into(),
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
        let extra = extra_blocklist(input.context.as_deref());
        match mask_profanity(&input.content, &extra) {
            Some(masked) => Verdict::Update(masked),
            None => Verdict::Accept,
        }
    }
}

/// Built-in profanity list (mild, representative — operators extend it via the
/// per-guardrail `blocklist` config). Lower-case; matched case-insensitively.
const PROFANITY: &[&str] = &[
    "damn", "dammit", "hell", "crap", "bastard", "idiot", "moron", "stupid", "jerk", "scum",
];

/// Parse `input.context` JSON for an extra `{"blocklist": [..]}` of words.
fn extra_blocklist(ctx_json: Option<&str>) -> Vec<String> {
    let Some(raw) = ctx_json else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    value
        .get("blocklist")
        .and_then(|b| b.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.as_str().map(str::to_lowercase))
                .collect()
        })
        .unwrap_or_default()
}

/// Mask profane words (built-in + `extra`). Returns `Some(masked)` if anything
/// was replaced, `None` if clean. Matching is per whitespace token,
/// case-insensitive, on the token's alphanumeric core.
#[must_use]
pub fn mask_profanity(input: &str, extra: &[String]) -> Option<String> {
    let mut result = input.to_string();
    let mut changed = false;
    for token in input.split_whitespace() {
        let core: String = token
            .trim_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_lowercase();
        if core.is_empty() {
            continue;
        }
        let hit = PROFANITY.contains(&core.as_str()) || extra.contains(&core);
        if hit {
            result = result.replace(token, &mask_token(token));
            changed = true;
        }
    }
    if changed { Some(result) } else { None }
}

/// Replace the alphanumeric characters of a token with `*`, keeping any
/// surrounding punctuation (e.g. `idiot!` -> `*****!`).
fn mask_token(token: &str) -> String {
    token
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { '*' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::mask_profanity;

    #[test]
    fn masks_builtin_and_extra() {
        let m = mask_profanity("you idiot, what a crap day", &[]).unwrap();
        assert!(!m.contains("idiot") && !m.contains("crap"));
        assert!(m.contains("*****")); // idiot -> *****
        let e = mask_profanity("this is fubar", &["fubar".to_string()]).unwrap();
        assert!(!e.contains("fubar"));
    }

    #[test]
    fn case_insensitive_keeps_punctuation() {
        let m = mask_profanity("STUPID!", &[]).unwrap();
        assert_eq!(m, "******!");
    }

    #[test]
    fn accepts_clean_text() {
        assert!(mask_profanity("Where is my order? Thank you!", &[]).is_none());
    }
}
