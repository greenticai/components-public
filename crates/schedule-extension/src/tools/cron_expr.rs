//! Parsing a cron expression the way the DEPLOYED runtime parses it.
//!
//! Both greentic-start and the designer's emitter read the expression with the
//! `cron` crate, and both normalise a five-field expression by prepending the
//! seconds field (`0`). This module does exactly that and nothing else, so a
//! schedule this extension calls valid at design time is one greentic-start
//! will accept at deploy time — the whole point of validating here is that the
//! operator learns about a typo while they can still see the node, rather than
//! from a build that refuses minutes later.
//!
//! Do not "improve" the normalisation independently. If the runtime's rule
//! changes, this follows it; a rule that is nearly the same is worse than no
//! rule, because it moves the failure from design time to deploy time for
//! exactly the expressions the two disagree about.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;

/// The number of fields a cron expression carries without a seconds field.
const FIVE_FIELD: usize = 5;

/// A parsed schedule plus the text it was parsed from.
#[derive(Debug)]
pub struct ParsedCron {
    pub schedule: Schedule,
    /// The expression as the operator wrote it — what a diagnostic quotes.
    pub written: String,
    /// True when a seconds field was prepended to reach the crate's six-field
    /// form. Worth reporting: it is why `* * * * *` fires once a minute here
    /// and not once a second.
    pub seconds_prepended: bool,
}

/// Parse `expr` under the runtime's own rule. `Err` is a sentence for an
/// operator, never a crate error string.
pub fn parse(expr: &str) -> Result<ParsedCron, String> {
    let written = expr.trim();
    if written.is_empty() {
        return Err("the schedule is empty; write a cron expression such as `0 9 * * *`".into());
    }
    let fields = written.split_whitespace().count();
    let seconds_prepended = fields == FIVE_FIELD;
    let normalized = if seconds_prepended {
        format!("0 {written}")
    } else {
        written.to_string()
    };
    let schedule = Schedule::from_str(&normalized).map_err(|_| {
        format!("`{written}` is not a cron expression the runtime can read (it accepts 5 fields — minute hour day month weekday — or 6 with a leading seconds field)")
    })?;
    Ok(ParsedCron {
        schedule,
        written: written.to_string(),
        seconds_prepended,
    })
}

/// Validate an IANA zone name. The runtime resolves the zone with `chrono_tz`,
/// so an abbreviation such as `WIB` or an offset such as `+07:00` is refused
/// here rather than at deploy time.
pub fn parse_timezone(tz: &str) -> Result<Tz, String> {
    let name = tz.trim();
    Tz::from_str(name).map_err(|_| {
        format!(
            "`{name}` is not an IANA time zone name (they look like `Asia/Jakarta` or `Europe/Amsterdam`, never an abbreviation or an offset)"
        )
    })
}

/// The next `count` times this schedule fires, in the given zone, after
/// `after`. Returned as RFC 3339 strings in that zone, so what the operator
/// reads is the wall-clock time they were thinking in.
pub fn next_fire_times(
    parsed: &ParsedCron,
    tz: Tz,
    after: DateTime<Utc>,
    count: usize,
) -> Vec<String> {
    parsed
        .schedule
        .after(&after.with_timezone(&tz))
        .take(count)
        .map(|t| t.to_rfc3339())
        .collect()
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn a_five_field_expression_is_read_as_the_runtime_reads_it() {
        // The whole reason this module exists: `cron` wants six fields, the
        // operator writes five, and the runtime bridges that by prepending a
        // seconds field. Getting this wrong makes every ordinary crontab line
        // invalid at design time and valid at deploy time.
        let parsed = parse("0 9 * * *").expect("valid");
        assert!(parsed.seconds_prepended);
        assert_eq!(parsed.written, "0 9 * * *");
    }

    #[test]
    fn a_six_field_expression_is_left_alone() {
        let parsed = parse("30 0 9 * * *").expect("valid");
        assert!(!parsed.seconds_prepended);
    }

    #[test]
    fn a_nonsense_expression_is_refused_with_what_the_runtime_accepts() {
        let err = parse("every morning").expect_err("must fail");
        assert!(err.contains("not a cron expression"), "{err}");
        assert!(err.contains("5 fields"), "{err}");
    }

    #[test]
    fn an_empty_expression_says_so_rather_than_failing_to_parse() {
        let err = parse("   ").expect_err("must fail");
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn an_abbreviation_is_not_an_iana_zone() {
        // `WIB` is what an Indonesian operator would write first, and it is
        // precisely the input that would deploy and then never resolve.
        let err = parse_timezone("WIB").expect_err("must fail");
        assert!(err.contains("IANA"), "{err}");
        assert!(parse_timezone("Asia/Jakarta").is_ok());
    }

    #[test]
    fn an_offset_is_not_an_iana_zone_either() {
        assert!(parse_timezone("+07:00").is_err());
    }

    #[test]
    fn fire_times_are_reported_in_the_declared_zone() {
        let parsed = parse("0 9 * * *").expect("valid");
        let after: DateTime<Utc> = "2026-09-23T00:00:00Z".parse().expect("timestamp");
        let times = next_fire_times(&parsed, chrono_tz::Asia::Jakarta, after, 3);
        assert_eq!(times.len(), 3);
        // 09:00 in Jakarta, not 09:00 UTC — the zone is what the operator was
        // thinking in, so showing UTC here would read as a wrong answer.
        assert!(
            times[0].starts_with("2026-09-23T09:00:00+07:00"),
            "{times:?}"
        );
    }

    #[test]
    fn a_five_field_star_expression_fires_every_minute_not_every_second() {
        let parsed = parse("* * * * *").expect("valid");
        let after: DateTime<Utc> = "2026-09-23T00:00:00Z".parse().expect("timestamp");
        let times = next_fire_times(&parsed, chrono_tz::UTC, after, 2);
        assert!(times[0].starts_with("2026-09-23T00:01:00"), "{times:?}");
        assert!(times[1].starts_with("2026-09-23T00:02:00"), "{times:?}");
    }
}
