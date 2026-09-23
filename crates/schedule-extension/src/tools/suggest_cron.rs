//! `suggest_cron` — turn "every weekday at 9am Jakarta time" into a cron
//! expression and a zone.
//!
//! Deliberately a small, explainable matcher rather than a general natural
//! language parser. Every answer it gives is round-tripped through
//! [`super::cron_expr::parse`] before it is returned, so a suggestion this
//! tool makes is always one the runtime accepts; and when the phrasing is not
//! one it recognises it ABSTAINS — `{"matched": false}` with the reason —
//! instead of returning a plausible schedule nobody asked for.
//!
//! An unattended worker runs when its schedule says it does. A guessed
//! expression that is close but wrong (`0 9 * * *` for "every 9 hours") fires
//! quietly for weeks before anyone notices, which is why abstaining is the
//! better failure here.

use serde_json::{Value, json};

use super::cron_expr;

/// Zone hints an operator is likely to type, mapped to IANA names. Not a
/// timezone database: anything not listed here is left to an explicit
/// `timezone` argument, which is always honoured over a guess.
const ZONE_HINTS: &[(&str, &str)] = &[
    ("jakarta", "Asia/Jakarta"),
    ("wib", "Asia/Jakarta"),
    ("singapore", "Asia/Singapore"),
    ("amsterdam", "Europe/Amsterdam"),
    ("london", "Europe/London"),
    ("utc", "UTC"),
    ("new york", "America/New_York"),
    ("tokyo", "Asia/Tokyo"),
    ("sydney", "Australia/Sydney"),
];

const WEEKDAY_WORDS: &[(&str, u32)] = &[
    ("sunday", 0),
    ("monday", 1),
    ("tuesday", 2),
    ("wednesday", 3),
    ("thursday", 4),
    ("friday", 5),
    ("saturday", 6),
];

pub fn suggest_cron(args: &Value) -> Result<String, String> {
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .ok_or("missing required field: text")?;
    let lower = text.to_ascii_lowercase();

    let timezone = args
        .get("timezone")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| zone_hint(&lower));
    if let Some(tz) = timezone.as_deref() {
        cron_expr::parse_timezone(tz)?;
    }

    let Some((expr, rationale)) = match_expression(&lower) else {
        return Ok(json!({
            "matched": false,
            "reason": format!(
                "'{text}' is not a phrasing this tool recognises. Write the cron expression directly (minute hour day month weekday, e.g. `0 9 * * 1-5`) and check it with validate_cron."
            ),
        })
        .to_string());
    };

    // Never hand back something the runtime would refuse.
    let parsed = cron_expr::parse(&expr)
        .map_err(|why| format!("internal: suggested `{expr}` which is not valid — {why}"))?;

    Ok(json!({
        "matched": true,
        "expr": parsed.written,
        "timezone": timezone,
        "rationale": rationale,
    })
    .to_string())
}

fn zone_hint(lower: &str) -> Option<String> {
    ZONE_HINTS
        .iter()
        .find(|(hint, _)| lower.contains(hint))
        .map(|(_, iana)| (*iana).to_string())
}

/// `(expression, rationale)`, or `None` to abstain.
fn match_expression(lower: &str) -> Option<(String, String)> {
    if let Some(step) = every_n_of(lower, "minute") {
        return Some((
            format!("*/{step} * * * *"),
            format!("Every {step} minutes."),
        ));
    }
    if let Some(step) = every_n_of(lower, "hour") {
        let minute = minute_past(lower).unwrap_or(0);
        return Some((
            format!("{minute} */{step} * * *"),
            format!("Every {step} hours, at {minute} minutes past."),
        ));
    }
    if lower.contains("every minute") {
        return Some(("* * * * *".into(), "Every minute.".into()));
    }
    if lower.contains("hourly") || lower.contains("every hour") {
        let minute = minute_past(lower).unwrap_or(0);
        return Some((
            format!("{minute} * * * *"),
            format!("Every hour, at {minute} minutes past."),
        ));
    }

    // Everything below needs a time of day; without one there is nothing to
    // suggest that is not an invention.
    let (hour, minute) = time_of_day(lower)?;

    if lower.contains("weekday") || lower.contains("working day") || lower.contains("business day")
    {
        return Some((
            format!("{minute} {hour} * * 1-5"),
            format!("Monday to Friday at {hour:02}:{minute:02}."),
        ));
    }
    if let Some(day) = weekday(lower) {
        let name = WEEKDAY_WORDS
            .iter()
            .find(|(_, n)| *n == day)
            .map(|(w, _)| *w)
            .unwrap_or("that day");
        return Some((
            format!("{minute} {hour} * * {day}"),
            format!("Every {name} at {hour:02}:{minute:02}."),
        ));
    }
    if lower.contains("month") {
        let dom = day_of_month(lower).unwrap_or(1);
        return Some((
            format!("{minute} {hour} {dom} * *"),
            format!("On day {dom} of each month at {hour:02}:{minute:02}."),
        ));
    }
    if lower.contains("daily") || lower.contains("every day") || lower.contains("each day") {
        return Some((
            format!("{minute} {hour} * * *"),
            format!("Every day at {hour:02}:{minute:02}."),
        ));
    }
    None
}

/// "every 15 minutes" / "every 6 hours" → the step.
fn every_n_of(lower: &str, unit: &str) -> Option<u32> {
    let after = lower.split("every ").nth(1)?;
    let mut words = after.split_whitespace();
    let n: u32 = words.next()?.parse().ok()?;
    let word = words.next()?.trim_end_matches(&[',', '.'][..]);
    (word == unit || word == format!("{unit}s")).then_some(n)
}

/// "at 9am", "at 09:30", "at 17:00", "at 5pm".
fn time_of_day(lower: &str) -> Option<(u32, u32)> {
    let after = lower.split(" at ").nth(1)?;
    let token = after.split_whitespace().next()?;
    let token = token.trim_end_matches(&[',', '.'][..]);
    let (digits, meridiem) = match token.strip_suffix("am") {
        Some(rest) => (rest, Some(false)),
        None => match token.strip_suffix("pm") {
            Some(rest) => (rest, Some(true)),
            None => (token, None),
        },
    };
    let (hour, minute) = match digits.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (digits.parse::<u32>().ok()?, 0),
    };
    let hour = match meridiem {
        Some(true) if hour < 12 => hour + 12,
        Some(false) if hour == 12 => 0,
        _ => hour,
    };
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// "at 20 minutes past" / "at :20".
fn minute_past(lower: &str) -> Option<u32> {
    let after = lower.split(" at ").nth(1)?;
    let token = after.split_whitespace().next()?;
    let token = token.trim_start_matches(':');
    let n: u32 = token.parse().ok()?;
    (n < 60).then_some(n)
}

fn weekday(lower: &str) -> Option<u32> {
    WEEKDAY_WORDS
        .iter()
        .find(|(word, _)| lower.contains(word))
        .map(|(_, n)| *n)
}

/// "on the 1st", "on day 15".
fn day_of_month(lower: &str) -> Option<u32> {
    for token in lower.split_whitespace() {
        let digits: String = token.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            continue;
        }
        let rest = &token[digits.len()..];
        if matches!(rest, "st" | "nd" | "rd" | "th") {
            let n: u32 = digits.parse().ok()?;
            if (1..=31).contains(&n) {
                return Some(n);
            }
        }
    }
    let after = lower.split("day ").nth(1)?;
    let n: u32 = after.split_whitespace().next()?.parse().ok()?;
    (1..=31).contains(&n).then_some(n)
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    fn suggested(text: &str) -> Value {
        serde_json::from_str(&suggest_cron(&json!({ "text": text })).expect("ok")).expect("json")
    }

    #[test]
    fn a_weekday_morning_becomes_a_weekday_expression_with_the_zone_it_named() {
        let out = suggested("every weekday at 9am Jakarta time");
        assert_eq!(out["matched"], true);
        assert_eq!(out["expr"], "0 9 * * 1-5");
        assert_eq!(out["timezone"], "Asia/Jakarta");
    }

    #[test]
    fn a_twelve_hour_clock_is_read_as_written() {
        assert_eq!(suggested("every day at 5pm")["expr"], "0 17 * * *");
        assert_eq!(suggested("daily at 12am")["expr"], "0 0 * * *");
        assert_eq!(suggested("daily at 12pm")["expr"], "0 12 * * *");
        assert_eq!(suggested("every day at 08:30")["expr"], "30 8 * * *");
    }

    #[test]
    fn intervals_do_not_need_a_time_of_day() {
        assert_eq!(suggested("every 15 minutes")["expr"], "*/15 * * * *");
        assert_eq!(suggested("every 6 hours")["expr"], "0 */6 * * *");
        assert_eq!(suggested("hourly")["expr"], "0 * * * *");
    }

    #[test]
    fn a_named_weekday_and_a_monthly_day_are_both_recognised() {
        assert_eq!(suggested("every monday at 7am")["expr"], "0 7 * * 1");
        assert_eq!(
            suggested("on the 1st of every month at 6am")["expr"],
            "0 6 1 * *"
        );
    }

    #[test]
    fn an_unrecognised_phrasing_abstains_instead_of_inventing_a_schedule() {
        // The rule this tool is built around: a wrong schedule on an
        // unattended worker fires quietly for weeks.
        let out = suggested("when the support queue gets busy");
        assert_eq!(out["matched"], false);
        assert!(out["expr"].is_null());
        assert!(
            out["reason"]
                .as_str()
                .expect("reason")
                .contains("validate_cron"),
            "{out}"
        );
    }

    #[test]
    fn a_time_of_day_with_no_recurrence_word_abstains() {
        // "at 9am" alone says nothing about how often.
        assert_eq!(suggested("at 9am")["matched"], false);
    }

    #[test]
    fn an_explicit_timezone_argument_beats_a_word_in_the_text() {
        let out: Value = serde_json::from_str(
            &suggest_cron(&json!({
                "text": "every day at 9am Jakarta time",
                "timezone": "Europe/Amsterdam",
            }))
            .expect("ok"),
        )
        .expect("json");
        assert_eq!(out["timezone"], "Europe/Amsterdam");
    }

    #[test]
    fn an_invalid_explicit_timezone_is_refused_rather_than_dropped() {
        let err = suggest_cron(&json!({ "text": "every day at 9am", "timezone": "WIB" }))
            .expect_err("must fail");
        assert!(err.contains("IANA"), "{err}");
    }

    #[test]
    fn every_suggestion_parses_as_the_runtime_would_parse_it() {
        for text in [
            "every weekday at 9am",
            "every 15 minutes",
            "every 6 hours",
            "every day at 5pm",
            "every monday at 7am",
            "on the 15th of every month at 6am",
            "hourly at 20",
        ] {
            let out = suggested(text);
            if out["matched"] == true {
                let expr = out["expr"].as_str().expect("expr");
                cron_expr::parse(expr).unwrap_or_else(|e| panic!("{text} → {expr}: {e}"));
            }
        }
    }

    #[test]
    fn missing_text_errors() {
        let err = suggest_cron(&json!({})).expect_err("must fail");
        assert!(err.contains("text"));
    }
}
