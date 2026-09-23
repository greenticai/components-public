//! `validate_cron` — diagnostics on a schedule trigger's config block.
//!
//! The same shape the designer's emitter reads (greentic-designer
//! `src/orchestrate/triggers_emit/collect.rs`): the cron fields sit at the TOP
//! level of the node config beside the common `trigger_id` / `enabled` /
//! `session` / `limits`, not inside a nested `cron` object. The nesting
//! happens on the way into `assets/triggers.json`; a node that nests it here
//! reaches the emitter with no `expr` at all, so that mistake is reported by
//! name rather than as a missing field.

use serde_json::Value;

use super::cron_expr;

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

/// `[a-z0-9][a-z0-9_-]{0,62}` — the same rule the designer applies, because
/// the trigger id becomes a path segment and a store namespace.
pub fn is_valid_trigger_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    id.len() <= 63
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Validate one schedule trigger node. Returns `(valid, diagnostics)`;
/// `valid` is false only when an `error` is present.
pub fn validate_cron(node: &Value) -> (bool, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let config = node.get("config").unwrap_or(&Value::Null);
    let Some(config) = config.as_object() else {
        diags.push(error(
            "missing-config",
            "the node has no `config` block; a schedule trigger needs at least an `expr`",
            "config",
        ));
        return (false, diags);
    };

    // The pre-contract / mis-nested shapes, named with the fix. Both produce a
    // node that packs successfully and never fires, which is the failure this
    // whole tool exists to move to design time.
    if config.contains_key("cron") && !config.contains_key("expr") {
        diags.push(error(
            "nested-cron-block",
            "the schedule fields belong at the top level of the node config: write `expr` (and `timezone`) directly, not inside a `cron` object",
            "config.cron",
        ));
    }
    for legacy in ["schedule", "schedule_id", "interval", "every"] {
        if config.contains_key(legacy) {
            diags.push(warn(
                "unknown-field",
                format!(
                    "`{legacy}` is not part of the schedule trigger contract and is dropped when the flow is packed; the schedule is `expr`, and the URL-stable id is `trigger_id`"
                ),
                &format!("config.{legacy}"),
            ));
        }
    }

    match config.get("expr").and_then(Value::as_str) {
        Some(expr) => match cron_expr::parse(expr) {
            Ok(parsed) => {
                if parsed.seconds_prepended {
                    // Not a defect — just the one thing about this field that
                    // surprises people who have written crontabs for years.
                    diags.push(warn(
                        "five-field-expression",
                        format!(
                            "`{}` is read as a 5-field crontab line, so the finest it can fire is once a minute; write 6 fields to control seconds",
                            parsed.written
                        ),
                        "config.expr",
                    ));
                }
            }
            Err(message) => diags.push(error("invalid-cron", message, "config.expr")),
        },
        None if config.contains_key("expr") => diags.push(error(
            "invalid-cron",
            "`expr` must be a cron expression written as a string",
            "config.expr",
        )),
        None => diags.push(error(
            "missing-cron",
            "the node has no schedule; set `expr` to a cron expression such as `0 9 * * *`",
            "config.expr",
        )),
    }

    match config.get("timezone") {
        Some(Value::String(tz)) => {
            if let Err(message) = cron_expr::parse_timezone(tz) {
                diags.push(error("invalid-timezone", message, "config.timezone"));
            }
        }
        Some(_) => diags.push(error(
            "invalid-timezone",
            "`timezone` must be an IANA zone name written as a string, e.g. `Asia/Jakarta`",
            "config.timezone",
        )),
        None => diags.push(warn(
            "no-timezone",
            "no `timezone` is set, so the schedule runs in UTC; set one if the time was meant as local wall-clock time",
            "config.timezone",
        )),
    }

    if let Some(id) = config.get("trigger_id") {
        match id.as_str() {
            Some(id) if is_valid_trigger_id(id) => {}
            Some(id) => diags.push(error(
                "invalid-trigger-id",
                format!("`{id}` must match [a-z0-9][a-z0-9_-]{{0,62}}"),
                "config.trigger_id",
            )),
            None => diags.push(error(
                "invalid-trigger-id",
                "`trigger_id` must be a string",
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

    #[test]
    fn a_daily_schedule_in_a_named_zone_is_valid() {
        let (valid, diags) = validate_cron(&json!({
            "config": { "expr": "0 9 * * *", "timezone": "Asia/Jakarta" }
        }));
        assert!(valid, "{diags:?}");
        // The five-field note is a warning, not an error: it explains the one
        // surprising thing about the field rather than refusing a correct line.
        assert_eq!(codes(&diags), vec!["five-field-expression"]);
    }

    #[test]
    fn a_missing_schedule_is_an_error_naming_the_field_to_set() {
        let (valid, diags) = validate_cron(&json!({ "config": { "timezone": "UTC" } }));
        assert!(!valid);
        assert!(codes(&diags).contains(&"missing-cron"));
        assert!(diags[0].message.contains("0 9 * * *"), "{diags:?}");
    }

    #[test]
    fn a_nested_cron_block_is_named_with_the_fix() {
        // This shape reads as correct — it is how the contract entry looks in
        // assets/triggers.json — and packs with no schedule at all.
        let (valid, diags) = validate_cron(&json!({
            "config": { "cron": { "expr": "0 9 * * *" } }
        }));
        assert!(!valid);
        assert!(codes(&diags).contains(&"nested-cron-block"));
    }

    #[test]
    fn an_abbreviated_timezone_is_refused_at_design_time() {
        let (valid, diags) = validate_cron(&json!({
            "config": { "expr": "0 9 * * *", "timezone": "WIB" }
        }));
        assert!(!valid);
        assert!(codes(&diags).contains(&"invalid-timezone"));
    }

    #[test]
    fn no_timezone_is_a_warning_that_says_what_happens_instead() {
        let (valid, diags) = validate_cron(&json!({ "config": { "expr": "0 9 * * *" } }));
        assert!(valid);
        let note = diags
            .iter()
            .find(|d| d.code == "no-timezone")
            .expect("the UTC note");
        assert!(note.message.contains("UTC"), "{note:?}");
    }

    #[test]
    fn a_legacy_field_is_reported_as_dropped_rather_than_silently_ignored() {
        let (valid, diags) = validate_cron(&json!({
            "config": { "expr": "0 9 * * *", "timezone": "UTC", "schedule_id": "daily" }
        }));
        assert!(valid, "a dropped field must not fail a build that works");
        let note = diags
            .iter()
            .find(|d| d.code == "unknown-field")
            .expect("the dropped-field note");
        assert!(note.message.contains("trigger_id"), "{note:?}");
    }

    #[test]
    fn a_bad_trigger_id_is_an_error() {
        let (valid, diags) = validate_cron(&json!({
            "config": { "expr": "0 9 * * *", "timezone": "UTC", "trigger_id": "Daily Report" }
        }));
        assert!(!valid);
        assert!(codes(&diags).contains(&"invalid-trigger-id"));
    }

    #[test]
    fn a_node_with_no_config_is_an_error_not_a_panic() {
        let (valid, diags) = validate_cron(&json!({}));
        assert!(!valid);
        assert_eq!(codes(&diags), vec!["missing-config"]);
    }
}
