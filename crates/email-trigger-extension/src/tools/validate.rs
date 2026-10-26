//! `validate_email_trigger` — diagnostics on an email trigger's config block.
//!
//! Mirrors, rule for rule, what greentic-designer's emitter
//! (`triggers_emit::email_block`) and greentic-start's own parser accept. A
//! declaration the host refuses refuses the WHOLE `triggers.json`, and so every
//! other trigger in the pack, naming nothing the operator drew; catching it
//! here names the node. The password is never a value: `password_ref` is a
//! `<provider>/<key>` secret NAME.

use serde_json::{Map, Value};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Diagnostic {
    pub severity: &'static str,
    pub code: &'static str,
    pub message: String,
    pub path: Option<String>,
}

fn error(code: &'static str, message: impl Into<String>, path: &str) -> Diagnostic {
    Diagnostic {
        severity: "error",
        code,
        message: message.into(),
        path: Some(path.to_string()),
    }
}

fn warn(code: &'static str, message: impl Into<String>, path: &str) -> Diagnostic {
    Diagnostic {
        severity: "warning",
        code,
        message: message.into(),
        path: Some(path.to_string()),
    }
}

const MAX_FIELD_LEN: usize = 256;

fn text<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn is_valid_trigger_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 63
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-'))
}

fn is_valid_secret_ref(value: &str) -> bool {
    fn segment_ok(s: &str) -> bool {
        let b = s.as_bytes();
        !b.is_empty()
            && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
            && b.iter().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'-')
            })
    }
    match value.split_once('/') {
        Some((provider, key)) => !key.contains('/') && segment_ok(provider) && segment_ok(key),
        None => false,
    }
}

/// Bounded, no control character, no quote or backslash (the characters that
/// end an IMAP quoted string).
fn check_safe_text(
    obj: &Map<String, Value>,
    key: &str,
    path: &str,
    required: bool,
    diags: &mut Vec<Diagnostic>,
) {
    match text(obj, key) {
        Some(v)
            if v.len() > MAX_FIELD_LEN
                || v.chars().any(|c| c.is_control() || c == '"' || c == '\\') =>
        {
            diags.push(error(
                "unsafe-text",
                format!(
                    "`{key}` must be at most {MAX_FIELD_LEN} characters with no control character, quote or backslash"
                ),
                path,
            ));
        }
        Some(_) => {}
        None if required => {
            diags.push(error("missing-field", format!("`{key}` is required"), path))
        }
        None => {}
    }
}

fn check_range(
    obj: &Map<String, Value>,
    key: &str,
    min: u64,
    max: u64,
    diags: &mut Vec<Diagnostic>,
) {
    if let Some(v) = obj.get(key).filter(|v| !v.is_null())
        && !v.as_u64().is_some_and(|n| (min..=max).contains(&n))
    {
        diags.push(error(
            "out-of-range",
            format!("`{key}` must be a whole number from {min} to {max}"),
            &format!("config.{key}"),
        ));
    }
}

/// Validate one email trigger node. Returns `(valid, diagnostics)`; `valid` is
/// false only when an `error` is present.
pub fn validate_email_trigger(node: &Value) -> (bool, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let Some(config) = node.get("config").and_then(Value::as_object) else {
        diags.push(error(
            "missing-config",
            "the node has no `config` block; an email trigger needs at least `host`, `username` and `password_ref`",
            "config",
        ));
        return (false, diags);
    };

    if config.contains_key("email_imap") && !config.contains_key("host") {
        diags.push(error(
            "nested-block",
            "the mailbox fields belong at the top level of the node config: write `host`, `username` and the rest directly, not inside an `email_imap` object",
            "config.email_imap",
        ));
    }

    match text(config, "host") {
        Some(host) => {
            if host.len() > 253
                || !host
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
            {
                diags.push(error(
                    "invalid-host",
                    "`host` must be a DNS name (letters, digits, `.`, `-`)",
                    "config.host",
                ));
            }
        }
        None => diags.push(error(
            "missing-field",
            "the node has no mail server; set `host` to e.g. `imap.gmail.com`",
            "config.host",
        )),
    }

    match text(config, "tls").unwrap_or("tls") {
        "tls" | "starttls" => {}
        other => diags.push(error(
            "invalid-tls",
            format!("`tls` `{other}` must be `tls` or `starttls`"),
            "config.tls",
        )),
    }
    check_range(config, "port", 1, 65_535, &mut diags);
    check_safe_text(config, "mailbox", "config.mailbox", false, &mut diags);
    check_safe_text(config, "username", "config.username", true, &mut diags);

    match text(config, "password_ref") {
        Some(r) if is_valid_secret_ref(r) => {}
        // Never echo the value: the likeliest cause is a pasted password.
        Some(_) => diags.push(error(
            "invalid-password-ref",
            "`password_ref` must name a secret as <provider>/<key> (e.g. email/imap_password), not hold the password itself",
            "config.password_ref",
        )),
        None => diags.push(error(
            "missing-field",
            "the node has no password reference; set `password_ref` to the NAME of the secret holding the mailbox password",
            "config.password_ref",
        )),
    }

    check_range(config, "poll_interval_s", 30, 86_400, &mut diags);
    check_range(config, "max_messages_per_poll", 1, 100, &mut diags);
    check_range(config, "max_message_bytes", 1, 25 * 1024 * 1024, &mut diags);

    let mut unseen = true;
    if let Some(search) = config.get("search").filter(|v| !v.is_null()) {
        match search.as_object() {
            Some(s) => {
                match s.get("unseen") {
                    None | Some(Value::Null) => {}
                    Some(Value::Bool(b)) => unseen = *b,
                    Some(_) => diags.push(error(
                        "invalid-search",
                        "`search.unseen` must be true or false",
                        "config.search.unseen",
                    )),
                }
                check_safe_text(s, "from", "config.search.from", false, &mut diags);
                check_safe_text(s, "subject", "config.search.subject", false, &mut diags);
            }
            None => diags.push(error(
                "invalid-search",
                "`search` must be an object",
                "config.search",
            )),
        }
    }

    match text(config, "on_success").unwrap_or("mark_seen") {
        "mark_seen" => {}
        "none" => {
            if unseen {
                diags.push(error(
                    "mail-would-repeat",
                    "`on_success` `none` needs the search to include read mail (`search.unseen` set to false); otherwise nothing takes the message out of the unread set and every poll lists it again",
                    "config.on_success",
                ));
            }
        }
        "move" => check_safe_text(config, "move_to", "config.move_to", true, &mut diags),
        other => diags.push(error(
            "invalid-on-success",
            format!("`on_success` `{other}` must be mark_seen, move or none"),
            "config.on_success",
        )),
    }

    if let Some(v) = config.get("include_html")
        && !v.is_boolean()
    {
        diags.push(error(
            "invalid-include-html",
            "`include_html` must be true or false",
            "config.include_html",
        ));
    }
    if let Some(mode) = text(config, "attachments")
        && !matches!(mode, "none" | "artifacts")
    {
        diags.push(error(
            "invalid-attachments",
            format!("`attachments` `{mode}` must be `none` or `artifacts`"),
            "config.attachments",
        ));
    }

    if let Some(id) = config.get("trigger_id") {
        match id.as_str() {
            Some(id) if is_valid_trigger_id(id) => {}
            _ => diags.push(error(
                "invalid-trigger-id",
                "`trigger_id` must be a string matching [a-z0-9][a-z0-9_-]{0,62}",
                "config.trigger_id",
            )),
        }
    }
    if let Some(enabled) = config.get("enabled")
        && !enabled.is_boolean()
    {
        diags.push(error(
            "invalid-enabled",
            "`enabled` must be true or false",
            "config.enabled",
        ));
    }

    if config.get("poll_interval_s").is_none() {
        diags.push(warn(
            "default-poll-interval",
            "no `poll_interval_s` is set, so the host uses its default interval; set one to control how often the mailbox is checked",
            "config.poll_interval_s",
        ));
    }

    let valid = !diags.iter().any(|d| d.severity == "error");
    (valid, diags)
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use serde_json::json;

    fn codes(diags: &[Diagnostic]) -> Vec<&str> {
        diags.iter().map(|d| d.code).collect()
    }

    fn base() -> Value {
        json!({ "config": {
            "host": "imap.example.com", "username": "bot@example.com",
            "password_ref": "email/imap_password", "poll_interval_s": 60
        }})
    }

    #[test]
    fn a_minimal_mailbox_is_valid() {
        let (valid, diags) = validate_email_trigger(&base());
        assert!(valid, "{diags:?}");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_pasted_password_is_refused_without_echoing_it() {
        let mut n = base();
        n["config"]["password_ref"] = json!("hunter2 secret!");
        let (valid, diags) = validate_email_trigger(&n);
        assert!(!valid);
        assert!(codes(&diags).contains(&"invalid-password-ref"));
        assert!(diags.iter().all(|d| !d.message.contains("hunter2")));
    }

    #[test]
    fn a_missing_host_names_the_field_to_set() {
        let (valid, diags) = validate_email_trigger(&json!({ "config": {
            "username": "u", "password_ref": "email/p" }}));
        assert!(!valid);
        assert!(
            diags
                .iter()
                .any(|d| d.path.as_deref() == Some("config.host"))
        );
    }

    #[test]
    fn a_host_with_a_scheme_is_refused() {
        let mut n = base();
        n["config"]["host"] = json!("imaps://imap.example.com");
        assert!(!validate_email_trigger(&n).0);
    }

    #[test]
    fn a_quote_in_the_username_is_refused() {
        let mut n = base();
        n["config"]["username"] = json!("a\"b");
        let (valid, diags) = validate_email_trigger(&n);
        assert!(!valid);
        assert!(codes(&diags).contains(&"unsafe-text"));
    }

    #[test]
    fn leaving_mail_unread_forever_is_refused() {
        let mut n = base();
        n["config"]["on_success"] = json!("none");
        let (valid, diags) = validate_email_trigger(&n);
        assert!(!valid);
        assert!(codes(&diags).contains(&"mail-would-repeat"));
        n["config"]["search"] = json!({ "unseen": false });
        assert!(validate_email_trigger(&n).0);
    }

    #[test]
    fn move_needs_a_destination() {
        let mut n = base();
        n["config"]["on_success"] = json!("move");
        assert!(!validate_email_trigger(&n).0);
        n["config"]["move_to"] = json!("Processed");
        assert!(validate_email_trigger(&n).0);
    }

    #[test]
    fn out_of_range_numbers_are_refused() {
        for (k, v) in [
            ("poll_interval_s", 5),
            ("port", 70_000),
            ("max_messages_per_poll", 0),
        ] {
            let mut n = base();
            n["config"][k] = json!(v);
            assert!(!validate_email_trigger(&n).0, "{k}");
        }
    }

    #[test]
    fn a_nested_block_is_named_with_the_fix() {
        let (valid, diags) = validate_email_trigger(&json!({ "config": {
            "email_imap": { "host": "imap.example.com" } }}));
        assert!(!valid);
        assert!(codes(&diags).contains(&"nested-block"));
    }

    #[test]
    fn a_node_with_no_config_is_an_error_not_a_panic() {
        let (valid, diags) = validate_email_trigger(&json!({}));
        assert!(!valid);
        assert_eq!(codes(&diags), vec!["missing-config"]);
    }

    #[test]
    fn a_bad_trigger_id_is_an_error() {
        let mut n = base();
        n["config"]["trigger_id"] = json!("Inbox Watch");
        assert!(!validate_email_trigger(&n).0);
    }
}
