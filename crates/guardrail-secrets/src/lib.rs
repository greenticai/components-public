//! Greentic secrets-leak guardrail design extension.
//!
//! Exports the `guardrail` interface:
//! - **update** (mask) when the content contains a high-confidence secret token
//!   (API keys, tokens, private-key blocks) — replaced with `[REDACTED_SECRET]`.
//! - **accept** otherwise.
//!
//! Applies in both directions: stops a user pasting a live key inbound and
//! stops the agent leaking one outbound. The pure [`mask_secrets`] function is
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
            id: "greentic.guardrail-secrets".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        vec![types::CapabilityRef {
            id: "greentic:guardrail/secrets".into(),
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
        match mask_secrets(&input.content) {
            Some(masked) => Verdict::Update(masked),
            None => Verdict::Accept,
        }
    }
}

const SECRET_PLACEHOLDER: &str = "[REDACTED_SECRET]";

/// Mask high-confidence secret tokens. Returns `Some(masked)` if anything was
/// replaced, `None` if the text was clean.
#[must_use]
pub fn mask_secrets(input: &str) -> Option<String> {
    let mut result = input.to_string();
    let mut changed = false;

    // Whitespace-delimited tokens: each unique secret token is replaced
    // wholesale (secrets are unique strings, so `replace` is safe here).
    for token in input.split_whitespace() {
        let trimmed = token
            .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.');
        if !trimmed.is_empty() && is_secret_token(trimmed) {
            result = result.replace(trimmed, SECRET_PLACEHOLDER);
            changed = true;
        }
    }

    // Multi-line PEM private-key block.
    if result.contains("-----BEGIN")
        && result.contains("PRIVATE KEY-----")
        && let Some(masked) = mask_pem_block(&result)
    {
        result = masked;
        changed = true;
    }

    if changed { Some(result) } else { None }
}

/// High-confidence single-token secret shapes (provider key prefixes).
fn is_secret_token(t: &str) -> bool {
    let alnum_tail = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric());
    (t.starts_with("sk-") && t.len() >= 20)
        || (t.starts_with("ghp_") && t.len() >= 20)
        || (t.starts_with("xoxb-") || t.starts_with("xoxp-"))
        || (t.starts_with("AKIA") && t.len() == 20 && alnum_tail(&t[4..]))
        || (t.starts_with("eyJ") && t.len() >= 30) // JWT-ish
        || (t.starts_with("AIza") && t.len() >= 30) // Google API key
}

/// Replace everything from `-----BEGIN` to the end of the `PRIVATE KEY-----`
/// trailer with the placeholder.
fn mask_pem_block(text: &str) -> Option<String> {
    let start = text.find("-----BEGIN")?;
    let end_marker = "PRIVATE KEY-----";
    let end_rel = text[start..].find(end_marker)? + end_marker.len();
    let end = start + end_rel;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start]);
    out.push_str(SECRET_PLACEHOLDER);
    out.push_str(&text[end..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::mask_secrets;

    #[test]
    fn masks_api_keys() {
        let m = mask_secrets("here is my key sk-abcdef0123456789ABCDEF use it").unwrap();
        assert!(m.contains("[REDACTED_SECRET]"));
        assert!(!m.contains("sk-abcdef0123456789ABCDEF"));
    }

    #[test]
    fn masks_aws_and_jwt() {
        assert!(mask_secrets("AKIAIOSFODNN7EXAMPLE").is_some());
        assert!(mask_secrets("token eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9").is_some());
    }

    #[test]
    fn accepts_clean_text() {
        assert!(mask_secrets("Where is my order #1234? Thanks!").is_none());
    }
}
