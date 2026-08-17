//! Pure best-effort read-only SQL guard. The gateway's read-only DB role is the
//! real safety boundary; this is the first line of defense over LLM-generated
//! SQL. No WIT imports — host-testable.

const FORBIDDEN: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "TRUNCATE", "GRANT", "REVOKE",
    "MERGE", "REPLACE", "CALL", "EXEC", "ATTACH", "PRAGMA", "INTO",
];

/// Tokenize into uppercase identifier words (runs of `[A-Za-z0-9_]`).
fn words(sql: &str) -> Vec<String> {
    sql.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch.to_ascii_uppercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Ensure `sql` is a single read-only `SELECT`/`WITH` statement. Returns
/// `Err(reason)` for anything that is empty, multi-statement, commented, or
/// contains a forbidden DML/DDL keyword.
pub fn ensure_read_only(sql: &str) -> Result<(), String> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err("empty SQL".to_string());
    }
    if trimmed.contains("--") || trimmed.contains("/*") {
        return Err("SQL comments are not allowed".to_string());
    }
    let body = trimmed.strip_suffix(';').unwrap_or(trimmed).trim_end();
    if body.contains(';') {
        return Err("multiple statements are not allowed".to_string());
    }
    let tokens = words(body);
    match tokens.first().map(String::as_str) {
        Some("SELECT" | "WITH") => {}
        Some(other) => {
            return Err(format!(
                "only SELECT/WITH queries are allowed, got: {other}"
            ));
        }
        None => return Err("no SQL keyword found".to_string()),
    }
    for keyword in FORBIDDEN {
        if tokens.iter().any(|token| token == keyword) {
            return Err(format!("forbidden keyword in SQL: {keyword}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_plain_select() {
        assert!(ensure_read_only("SELECT id, name FROM users WHERE id = 1").is_ok());
    }

    #[test]
    fn allows_lowercase_and_with_cte() {
        assert!(ensure_read_only("select * from t").is_ok());
        assert!(ensure_read_only("WITH x AS (SELECT 1) SELECT * FROM x").is_ok());
    }

    #[test]
    fn strips_single_trailing_semicolon() {
        assert!(ensure_read_only("SELECT 1;").is_ok());
    }

    #[test]
    fn rejects_dml_dll() {
        assert!(ensure_read_only("DELETE FROM users").is_err());
        assert!(ensure_read_only("INSERT INTO t VALUES (1)").is_err());
        assert!(ensure_read_only("DROP TABLE t").is_err());
        assert!(ensure_read_only("UPDATE t SET a=1").is_err());
    }

    #[test]
    fn rejects_select_then_mutation() {
        assert!(ensure_read_only("SELECT 1; DROP TABLE t").is_err());
    }

    #[test]
    fn rejects_comments_and_empty() {
        assert!(ensure_read_only("SELECT 1 -- comment").is_err());
        assert!(ensure_read_only("SELECT 1 /* x */").is_err());
        assert!(ensure_read_only("   ").is_err());
    }

    #[test]
    fn select_into_is_blocked() {
        assert!(ensure_read_only("SELECT * INTO backup FROM t").is_err());
        assert!(ensure_read_only("SELECT * INTO OUTFILE '/tmp/x' FROM t").is_err());
    }

    #[test]
    fn keyword_in_string_literal_is_a_known_false_positive() {
        // The tokenizer does not parse string literals; this conservative
        // false-positive is intentional (safe > usable; gateway role is the backstop).
        assert!(ensure_read_only("SELECT 'please DELETE this row' FROM t").is_err());
    }

    #[test]
    fn paren_wrapped_select_is_allowed() {
        assert!(ensure_read_only("(SELECT 1)").is_ok());
    }
}
