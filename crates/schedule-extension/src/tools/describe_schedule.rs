//! `describe_schedule` — say in words when a cron expression fires, and show
//! the next few times it will.
//!
//! A cron line is the one piece of a flow an operator cannot read back. The
//! prose is a best effort over the common shapes; the FIRE TIMES are the part
//! that is always exact, because they come from the same `cron` crate the
//! runtime schedules with. When the prose cannot describe an expression it
//! says so rather than guessing — a confident wrong sentence about when an
//! unattended worker runs is worse than no sentence.

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::cron_expr::{self, ParsedCron};

/// How many upcoming fire times to show. Enough to see a weekly or monthly
/// pattern without turning the panel into a calendar.
const PREVIEW_COUNT: usize = 5;

pub fn describe_schedule(args: &Value) -> Result<String, String> {
    let expr = args
        .get("expr")
        .and_then(Value::as_str)
        .ok_or("missing required field: expr")?;
    let parsed = cron_expr::parse(expr)?;

    let tz_name = args
        .get("timezone")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let tz = match tz_name {
        Some(name) => cron_expr::parse_timezone(name)?,
        None => chrono_tz::UTC,
    };

    // The reference instant. An explicit `from` keeps this tool deterministic
    // for a caller that wants to show the same preview twice; without one it
    // is the host's clock, which is what an operator means by "next".
    let after = match args.get("from").and_then(Value::as_str) {
        Some(raw) => DateTime::parse_from_rfc3339(raw)
            .map_err(|_| format!("`from` must be an RFC 3339 timestamp, got `{raw}`"))?
            .with_timezone(&Utc),
        None => now_utc(),
    };

    Ok(json!({
        "summary": summarize(&parsed, tz_name),
        "next_fire_times": cron_expr::next_fire_times(&parsed, tz, after, PREVIEW_COUNT),
        "timezone": tz.name(),
        "timezone_declared": tz_name.is_some(),
    })
    .to_string())
}

/// The host clock, as UTC. `SystemTime` is available on wasip2; a clock that
/// cannot be read falls back to the Unix epoch, which makes the preview
/// obviously wrong rather than subtly plausible.
fn now_utc() -> DateTime<Utc> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    DateTime::from_timestamp(secs, 0)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).expect("epoch"))
}

/// Prose for the common crontab shapes, in the zone the operator declared.
fn summarize(parsed: &ParsedCron, tz_name: Option<&str>) -> String {
    let fields: Vec<&str> = parsed.written.split_whitespace().collect();
    let zone = match tz_name {
        Some(name) => format!(" ({name})"),
        None => " (UTC)".to_string(),
    };
    // Only the 5-field shape is described in words. A 6-field expression is a
    // sub-minute schedule, which no phrasing makes clearer than the times.
    let [minute, hour, dom, month, dow] = fields.as_slice() else {
        return format!("Fires on the schedule `{}`{zone}.", parsed.written);
    };
    let every_day = *dom == "*" && *month == "*" && *dow == "*";
    let at_a_fixed_time = is_number(minute) && is_number(hour);

    if every_day && at_a_fixed_time {
        return format!("Every day at {}{zone}.", clock(hour, minute));
    }
    if at_a_fixed_time
        && *dom == "*"
        && *month == "*"
        && let Some(days) = weekdays(dow)
    {
        return format!("Every {days} at {}{zone}.", clock(hour, minute));
    }
    if at_a_fixed_time && *dow == "*" && *month == "*" && is_number(dom) {
        return format!(
            "On day {dom} of every month at {}{zone}.",
            clock(hour, minute)
        );
    }
    if *hour == "*" && is_number(minute) && every_day {
        return format!("Every hour, at {minute} minutes past{zone}.");
    }
    if let Some(step) = every_n(minute)
        && *hour == "*"
        && every_day
    {
        return format!("Every {step} minutes{zone}.");
    }
    if let Some(step) = every_n(hour)
        && is_number(minute)
        && every_day
    {
        return format!("Every {step} hours, at {minute} minutes past{zone}.");
    }
    // Deliberately not a guess. The fire times below it are exact.
    format!(
        "Fires on the crontab schedule `{}`{zone}; see the next times below.",
        parsed.written
    )
}

fn is_number(field: &str) -> bool {
    !field.is_empty() && field.chars().all(|c| c.is_ascii_digit())
}

/// `*/15` → `15`. Any other step form is left to the fire times.
fn every_n(field: &str) -> Option<&str> {
    let step = field.strip_prefix("*/")?;
    is_number(step).then_some(step)
}

fn clock(hour: &str, minute: &str) -> String {
    format!(
        "{:02}:{:02}",
        hour.parse::<u32>().unwrap_or(0),
        minute.parse::<u32>().unwrap_or(0)
    )
}

/// The weekday field, in words. Numeric and three-letter names both appear in
/// real crontabs, and `cron` accepts both, so both are described.
fn weekdays(dow: &str) -> Option<String> {
    const NAMES: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    fn one(token: &str) -> Option<&'static str> {
        if let Ok(n) = token.parse::<usize>() {
            // Both 0 and 7 mean Sunday in crontab.
            return NAMES.get(n % 7).copied();
        }
        let lower = token.to_ascii_lowercase();
        NAMES
            .iter()
            .find(|n| n.to_ascii_lowercase().starts_with(&lower) && lower.len() >= 3)
            .copied()
    }

    if dow == "*" {
        return None;
    }
    if let Some((from, to)) = dow.split_once('-') {
        return Some(format!("{} to {}", one(from)?, one(to)?));
    }
    let names: Option<Vec<&str>> = dow.split(',').map(one).collect();
    let names = names?;
    match names.as_slice() {
        [single] => Some((*single).to_string()),
        [head @ .., last] => Some(format!("{} and {last}", head.join(", "))),
        [] => None,
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    fn described(args: Value) -> Value {
        serde_json::from_str(&describe_schedule(&args).expect("ok")).expect("json")
    }

    fn daily(expr: &str) -> Value {
        described(serde_json::json!({
            "expr": expr,
            "timezone": "Asia/Jakarta",
            "from": "2026-09-23T00:00:00Z",
        }))
    }

    #[test]
    fn a_daily_schedule_reads_as_a_sentence_and_lists_five_times() {
        let out = daily("0 9 * * *");
        assert_eq!(out["summary"], "Every day at 09:00 (Asia/Jakarta).");
        assert_eq!(
            out["next_fire_times"].as_array().expect("array").len(),
            PREVIEW_COUNT
        );
    }

    #[test]
    fn the_times_are_exact_even_when_the_prose_gives_up() {
        // The contract this tool keeps: prose is best effort, times are not.
        let out = described(serde_json::json!({
            "expr": "0 9 1,15 * 1-5",
            "from": "2026-09-23T00:00:00Z",
        }));
        assert!(
            out["summary"]
                .as_str()
                .expect("summary")
                .contains("crontab"),
            "{out}"
        );
        assert_eq!(out["next_fire_times"].as_array().expect("array").len(), 5);
    }

    #[test]
    fn weekday_names_are_described_both_ways_they_are_written() {
        assert_eq!(
            daily("30 8 * * 1-5")["summary"],
            "Every Monday to Friday at 08:30 (Asia/Jakarta)."
        );
        assert_eq!(
            daily("30 8 * * mon,wed,fri")["summary"],
            "Every Monday, Wednesday and Friday at 08:30 (Asia/Jakarta)."
        );
    }

    #[test]
    fn sunday_is_seven_as_well_as_zero() {
        assert_eq!(
            daily("0 7 * * 7")["summary"],
            "Every Sunday at 07:00 (Asia/Jakarta)."
        );
    }

    #[test]
    fn interval_shapes_are_described() {
        assert_eq!(
            daily("*/15 * * * *")["summary"],
            "Every 15 minutes (Asia/Jakarta)."
        );
        assert_eq!(
            daily("0 */6 * * *")["summary"],
            "Every 6 hours, at 0 minutes past (Asia/Jakarta)."
        );
        assert_eq!(
            daily("5 * * * *")["summary"],
            "Every hour, at 5 minutes past (Asia/Jakarta)."
        );
    }

    #[test]
    fn a_monthly_schedule_names_the_day() {
        assert_eq!(
            daily("0 6 1 * *")["summary"],
            "On day 1 of every month at 06:00 (Asia/Jakarta)."
        );
    }

    #[test]
    fn an_absent_timezone_says_utc_rather_than_staying_silent() {
        let out = described(serde_json::json!({
            "expr": "0 9 * * *",
            "from": "2026-09-23T00:00:00Z",
        }));
        assert_eq!(out["summary"], "Every day at 09:00 (UTC).");
        assert_eq!(out["timezone"], "UTC");
        assert_eq!(out["timezone_declared"], false);
    }

    #[test]
    fn an_invalid_expression_is_refused_with_the_runtimes_rule() {
        let err = describe_schedule(&serde_json::json!({ "expr": "every morning" }))
            .expect_err("must fail");
        assert!(err.contains("not a cron expression"), "{err}");
    }

    #[test]
    fn a_bad_from_timestamp_is_named_rather_than_silently_ignored() {
        let err =
            describe_schedule(&serde_json::json!({ "expr": "0 9 * * *", "from": "tomorrow" }))
                .expect_err("must fail");
        assert!(err.contains("RFC 3339"), "{err}");
    }

    #[test]
    fn missing_expr_errors() {
        let err = describe_schedule(&serde_json::json!({})).expect_err("must fail");
        assert!(err.contains("expr"));
    }
}
