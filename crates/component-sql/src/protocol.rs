//! Pure request/response shaping for the gateway `/schema` + `/query` endpoints
//! and the OpenAI-compatible LLM. No WIT imports — host-testable.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    #[serde(default, rename = "type")]
    pub type_: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    #[serde(default)]
    pub columns: Vec<Column>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Schema {
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub tables: Vec<Table>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct QueryResult {
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub rows: Vec<Value>,
    #[serde(default)]
    pub row_count: u64,
    #[serde(default)]
    pub truncated: bool,
}

/// Parse a gateway `/schema` response.
pub fn parse_schema(json: &str) -> Result<Schema, String> {
    serde_json::from_str::<Schema>(json).map_err(|error| format!("decode schema: {error}"))
}

/// Render a schema as compact prompt text: one `table(col type, ...)` per line.
#[must_use]
pub fn format_schema_prompt(schema: &Schema) -> String {
    let mut out = String::new();
    for table in &schema.tables {
        let cols: Vec<String> = table
            .columns
            .iter()
            .map(|column| format!("{} {}", column.name, column.type_))
            .collect();
        let _ = writeln!(out, "{}({})", table.name, cols.join(", "));
    }
    out
}

/// Build the OpenAI-compatible chat-completions request body.
#[must_use]
pub fn build_llm_request(model: &str, engine: &str, schema_text: &str, question: &str) -> Value {
    let system = "You are a careful data analyst. Given a database schema and SQL dialect, \
                  write exactly ONE read-only SQL query (SELECT only) that answers the user's \
                  question. Return ONLY the SQL — no explanation and no markdown code fences.";
    let user = format!(
        "Dialect: {engine}\nSchema (one table per line as table(column type, ...)):\n{schema_text}\nQuestion: {question}"
    );
    json!({
        "model": model,
        "temperature": 0,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    })
}

/// Extract the SQL string from a chat-completions response, stripping markdown
/// code fences if the model added them.
pub fn extract_sql(response_json: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(response_json)
        .map_err(|error| format!("decode LLM response: {error}"))?;
    let content = value["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| "LLM response missing choices[0].message.content".to_string())?;
    let sql = strip_fences(content);
    if sql.is_empty() {
        return Err("LLM returned empty SQL".to_string());
    }
    Ok(sql)
}

fn strip_fences(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.starts_with("```") {
        let body = match trimmed.split_once('\n') {
            // Multi-line fence: drop the opening ```/```lang line + trailing ```
            Some((_, rest)) => rest.trim_end().strip_suffix("```").unwrap_or(rest),
            // Single-line fence: ```SELECT 1``` — strip the backtick delimiters
            None => trimmed.trim_start_matches('`').trim_end_matches('`'),
        };
        body.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Build the gateway `/query` request body.
#[must_use]
pub fn build_query_request(sql: &str, max_rows: u32) -> Value {
    json!({ "sql": sql, "max_rows": max_rows })
}

/// Parse a gateway `/query` response.
pub fn parse_query_response(json: &str) -> Result<QueryResult, String> {
    serde_json::from_str::<QueryResult>(json)
        .map_err(|error| format!("decode query response: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_schema_and_formats_prompt() {
        let schema = parse_schema(
            r#"{"engine":"postgres","tables":[{"name":"users","columns":[{"name":"id","type":"integer"},{"name":"email","type":"text"}]}]}"#,
        )
        .unwrap();
        assert_eq!(schema.engine, "postgres");
        let prompt = format_schema_prompt(&schema);
        assert!(prompt.contains("users(id integer, email text)"));
    }

    #[test]
    fn builds_llm_request_with_question_and_engine() {
        let req = build_llm_request("gpt-4o-mini", "mysql", "users(id int)", "how many users?");
        assert_eq!(req["model"], "gpt-4o-mini");
        assert_eq!(req["temperature"], 0);
        assert_eq!(req["messages"][0]["role"], "system");
        let user = req["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("mysql"));
        assert!(user.contains("how many users?"));
    }

    #[test]
    fn extracts_sql_plain_and_fenced() {
        let plain = r#"{"choices":[{"message":{"content":"SELECT 1"}}]}"#;
        assert_eq!(extract_sql(plain).unwrap(), "SELECT 1");
        let fenced = "{\"choices\":[{\"message\":{\"content\":\"```sql\\nSELECT 2\\n```\"}}]}";
        assert_eq!(extract_sql(fenced).unwrap(), "SELECT 2");
    }

    #[test]
    fn extract_sql_missing_content_is_err() {
        assert!(extract_sql(r#"{"choices":[]}"#).is_err());
    }

    #[test]
    fn builds_query_request_and_parses_response() {
        let req = build_query_request("SELECT 1", 100);
        assert_eq!(req["sql"], "SELECT 1");
        assert_eq!(req["max_rows"], 100);
        let resp = parse_query_response(
            r#"{"columns":["id"],"rows":[[1],[2]],"row_count":2,"truncated":false}"#,
        )
        .unwrap();
        assert_eq!(resp.columns, vec!["id"]);
        assert_eq!(resp.row_count, 2);
        assert!(!resp.truncated);
    }

    #[test]
    fn parse_query_response_defaults_missing_fields() {
        let resp = parse_query_response(r#"{"columns":["a"]}"#).unwrap();
        assert!(resp.rows.is_empty());
        assert_eq!(resp.row_count, 0);
    }

    #[test]
    fn extracts_sql_single_line_fence() {
        let fenced = "{\"choices\":[{\"message\":{\"content\":\"```SELECT 1```\"}}]}";
        assert_eq!(extract_sql(fenced).unwrap(), "SELECT 1");
    }
}
