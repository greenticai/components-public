//! Greentic PII-masking guardrail design extension.
//!
//! Exports the `guardrail` interface with three behaviours:
//! - **deny** when `input.context` JSON contains a `blocklist` matching the content.
//! - **update** when emails or phone numbers are detected (masked in place).
//! - **accept** otherwise.
//!
//! The pure [`mask_pii`] function is separately testable on the host target.

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
            id: "greentic.guardrail-pii".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        vec![types::CapabilityRef {
            id: "greentic:guardrail/pii".into(),
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
        let _ = Direction::Inbound; // suppress unused variant warning
        if let Some(ref blocked_word) =
            find_blocklist_match(&input.content, input.context.as_deref())
        {
            return Verdict::Deny(DenyInfo {
                code: "permission_denied".into(),
                message: "Request blocked by content policy.".into(),
                details: Some(format!("Matched blocklist entry: {blocked_word}")),
            });
        }

        match mask_pii(&input.content) {
            Some(masked) => Verdict::Update(masked),
            None => Verdict::Accept,
        }
    }
}

/// Parse `context` JSON for `{"blocklist": [...]}` and return the first matching entry as an owned String.
fn find_blocklist_match(text: &str, ctx_json: Option<&str>) -> Option<String> {
    let raw = ctx_json?;
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let entries = value.get("blocklist")?.as_array()?;
    entries.iter().find_map(|entry| {
        let word = entry.as_str()?;
        if text.contains(word) {
            Some(word.to_owned())
        } else {
            None
        }
    })
}

/// Mask PII (emails and phone numbers) in `input`.
///
/// Returns `Some(masked)` if anything was replaced, `None` if the text was clean.
#[must_use]
pub fn mask_pii(input: &str) -> Option<String> {
    let after_emails = mask_emails(input);
    let after_phones = mask_phones(&after_emails);
    if after_phones == input {
        None
    } else {
        Some(after_phones)
    }
}

fn mask_emails(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut result = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' && i > 0 {
            // Walk backward to find local-part start.
            let mut local_start = i;
            while local_start > 0 && is_email_local_char(chars[local_start - 1]) {
                local_start -= 1;
            }
            let local_len = i - local_start;
            // Walk forward to find domain end (must contain at least one dot).
            let domain_start = i + 1;
            let mut domain_end = domain_start;
            while domain_end < chars.len()
                && (is_email_local_char(chars[domain_end]) || chars[domain_end] == '.')
            {
                domain_end += 1;
            }
            let domain_slice = &chars[domain_start..domain_end];
            let has_dot = domain_slice.contains(&'.');
            if local_len > 0 && has_dot {
                // Remove the local-part already pushed to result.
                let bytes_to_remove = count_utf8_bytes_of_last_n_chars(&result, local_len);
                result.truncate(result.len() - bytes_to_remove);
                result.push_str("[REDACTED_EMAIL]");
                i = domain_end;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn mask_phones(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut result = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some(end) = try_match_phone(&chars, i) {
            result.push_str("[REDACTED_PHONE]");
            i = end;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

fn try_match_phone(chars: &[char], start: usize) -> Option<usize> {
    let n = chars.len();

    // Must be preceded by a digit or an attaching separator (not space).
    // Space/punctuation before '+' or first digit is fine — it's a boundary.
    if start > 0 {
        let prev = chars[start - 1];
        if prev.is_ascii_digit() || is_phone_attaching_sep(prev) {
            return None;
        }
    }

    let mut i = start;
    // Optional leading '+'.
    if i < n && chars[i] == '+' {
        i += 1;
    }

    let token_start = i;
    let mut digit_count = 0usize;
    while i < n && (chars[i].is_ascii_digit() || is_phone_sep(chars[i])) {
        if chars[i].is_ascii_digit() {
            digit_count += 1;
        }
        i += 1;
    }

    if i == token_start || digit_count < 7 {
        return None;
    }

    // Rewind trailing separators (e.g. trailing space consumed in the loop).
    while i > token_start && !chars[i - 1].is_ascii_digit() {
        i -= 1;
    }

    // After rewinding, the next char must not be a letter (avoids matching inside words/URLs).
    if i < n && chars[i].is_alphabetic() {
        return None;
    }

    Some(i)
}

fn is_email_local_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-')
}

/// Separators that can appear WITHIN a phone number token.
fn is_phone_sep(c: char) -> bool {
    matches!(c, ' ' | '-' | '.' | '(' | ')')
}

/// Separators that attach immediately before a phone number (excluding space).
/// Space before a phone number is allowed; `-` or `.` directly before indicates mid-token.
fn is_phone_attaching_sep(c: char) -> bool {
    matches!(c, '-' | '.' | '(' | ')')
}

fn count_utf8_bytes_of_last_n_chars(s: &str, n: usize) -> usize {
    s.char_indices()
        .rev()
        .take(n)
        .last()
        .map_or(s.len(), |(idx, _)| s.len() - idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_email() {
        let out = mask_pii("contact me at a@b.com please").unwrap();
        assert!(!out.contains("a@b.com"), "email still present: {out}");
        assert!(out.contains("[REDACTED_EMAIL]"), "marker missing: {out}");
    }

    #[test]
    fn leaves_clean_text_untouched() {
        assert!(mask_pii("hello world").is_none());
    }

    #[test]
    fn masks_phone_e164() {
        let out = mask_pii("call me at +1-800-555-1234 ok").unwrap();
        assert!(
            out.contains("[REDACTED_PHONE]"),
            "phone marker missing: {out}"
        );
        assert!(
            !out.contains("555-1234"),
            "phone digits still present: {out}"
        );
    }

    #[test]
    fn masks_multiple_pii() {
        let out = mask_pii("email: user@example.com phone: 555-867-5309").unwrap();
        assert!(
            out.contains("[REDACTED_EMAIL]"),
            "email marker missing: {out}"
        );
        assert!(
            out.contains("[REDACTED_PHONE]"),
            "phone marker missing: {out}"
        );
        assert!(
            !out.contains("user@example.com"),
            "email still present: {out}"
        );
        assert!(!out.contains("555-867-5309"), "phone still present: {out}");
    }

    #[test]
    fn blocklist_deny() {
        let ctx = r#"{"blocklist": ["badword", "evil"]}"#;
        let result = find_blocklist_match("this has badword in it", Some(ctx));
        assert_eq!(result.as_deref(), Some("badword"));
    }

    #[test]
    fn blocklist_no_match() {
        let ctx = r#"{"blocklist": ["forbidden"]}"#;
        let result = find_blocklist_match("clean text here", Some(ctx));
        assert!(result.is_none());
    }
}

#[cfg(test)]
mod proptests {
    use super::mask_pii;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn email_always_redacted(
            local in "[a-z][a-z0-9]{1,8}",
            domain in "[a-z]{2,8}",
            tld in "[a-z]{2,4}",
            prefix in "[a-z ]{0,20}",
            suffix in "[a-z ]{0,20}",
        ) {
            let email = format!("{local}@{domain}.{tld}");
            let text = format!("{prefix}{email}{suffix}");
            // The text always contains a real email, so mask_pii MUST return Some
            // (returning None would mean the email was present but unmasked).
            let masked = mask_pii(&text);
            prop_assert!(
                masked.is_some(),
                "mask_pii returned None (email '{email}' present but not masked) for input: '{text}'"
            );
            let masked = masked.unwrap();
            prop_assert!(
                !masked.contains(email.as_str()),
                "email '{email}' still present in masked output: '{masked}'"
            );
        }
    }
}
