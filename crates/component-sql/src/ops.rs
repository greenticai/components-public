//! The single operation: ask a database a question in natural language.
//!
//! The four steps are the design extension's `sql_ask`, unchanged in order and
//! in meaning — introspect the schema, have an LLM write SQL, GUARD it, run it.
//! Only where the configuration comes from is different: the extension reads a
//! worker's `secret://sql/*` keys, whereas a flow node is authored per node, so
//! the gateway and the LLM arrive as node config.
//!
//! Every failure is a VALUE. A flow routes on `ok == false`; a trap would take
//! the run down with a message no operator can act on.

use serde_json::Value;

use crate::transport::{check, get, post_json, resolve_secret, send};
use crate::{guard, protocol};

/// The extension's own defaults, kept identical so the same question against
/// the same gateway returns the same number of rows either way.
const DEFAULT_MAX_ROWS: u32 = 100;
const MAX_ROWS_CEILING: u32 = 1000;

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(response) => return response,
        }
    };
}

fn req_str<'a>(input: &'a Value, name: &str) -> Result<&'a str, Value> {
    input
        .get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| err(format!("missing required field `{name}`")))
}

fn secret(input: &Value, name: &str) -> Result<String, Value> {
    let raw = input
        .get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            err(format!(
                "missing required field `{name}` (a value, or `secret:NAME`)"
            ))
        })?;
    resolve_secret(raw).map_err(err)
}

/// Clamp rather than reject: an out-of-range `max_results` is an operator typo,
/// and failing the node teaches nothing that returning 1000 rows does not.
fn max_rows(input: &Value) -> u32 {
    input
        .get("max_results")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .map_or(DEFAULT_MAX_ROWS, |n| n.clamp(1, MAX_ROWS_CEILING))
}

pub fn ask(input: &Value) -> Value {
    let question = tri!(req_str(input, "question"));
    let gateway = tri!(req_str(input, "gateway_url"))
        .trim_end_matches('/')
        .to_string();
    let gateway_token = tri!(secret(input, "gateway_token"));
    let llm_base = tri!(req_str(input, "llm_base_url"))
        .trim_end_matches('/')
        .to_string();
    let llm_model = tri!(req_str(input, "llm_model")).to_string();
    let llm_key = tri!(secret(input, "llm_api_key"));
    let rows_wanted = max_rows(input);

    // 1. Schema (gateway-cached).
    let schema_bytes = tri!(
        send(get(&format!("{gateway}/schema"), &gateway_token))
            .and_then(|r| check("schema introspection", r))
            .map_err(err)
    );
    let schema = tri!(protocol::parse_schema(&String::from_utf8_lossy(&schema_bytes)).map_err(err));
    let engine = if schema.engine.is_empty() {
        input
            .get("engine")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        schema.engine.clone()
    };
    let schema_text = protocol::format_schema_prompt(&schema);

    // 2. LLM -> SQL.
    let llm_req = protocol::build_llm_request(&llm_model, &engine, &schema_text, question);
    let llm_bytes = tri!(
        send(post_json(
            &format!("{llm_base}/chat/completions"),
            &llm_key,
            &llm_req
        ))
        .and_then(|r| check("LLM completion", r))
        .map_err(err)
    );
    let sql = tri!(protocol::extract_sql(&String::from_utf8_lossy(&llm_bytes)).map_err(err));

    // 3. Guard. Best-effort over LLM-written SQL; the gateway's read-only DB
    //    role is the real boundary, and this node cannot weaken it.
    tri!(guard::ensure_read_only(&sql).map_err(err));

    // 4. Execute.
    let query_bytes = tri!(
        send(post_json(
            &format!("{gateway}/query"),
            &gateway_token,
            &protocol::build_query_request(&sql, rows_wanted)
        ))
        .and_then(|r| check("query", r))
        .map_err(err)
    );
    let result =
        tri!(protocol::parse_query_response(&String::from_utf8_lossy(&query_bytes)).map_err(err));

    ok(serde_json::json!({
        "sql": sql,
        "columns": result.columns,
        "rows": result.rows,
        "row_count": result.row_count,
        "truncated": result.truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Value {
        serde_json::json!({
            "question": "how many users",
            "gateway_url": "https://gw.example/",
            "gateway_token": "tok",
            "llm_base_url": "https://llm.example/v1",
            "llm_model": "gpt-4o-mini",
            "llm_api_key": "key"
        })
    }

    #[test]
    fn every_required_field_is_named_when_it_is_absent() {
        for field in [
            "question",
            "gateway_url",
            "gateway_token",
            "llm_base_url",
            "llm_model",
            "llm_api_key",
        ] {
            let mut input = full();
            input.as_object_mut().unwrap().remove(field);
            let out = ask(&input);
            assert_eq!(out["ok"], false, "{field}");
            assert!(
                out["error"].as_str().unwrap().contains(field),
                "error must name `{field}`, got {}",
                out["error"]
            );
        }
    }

    /// A blank string is the shape a half-filled form produces, and it must be
    /// refused the same way an absent field is rather than reaching the network.
    #[test]
    fn a_blank_required_field_is_refused_like_an_absent_one() {
        let mut input = full();
        input["gateway_url"] = Value::String("   ".into());
        assert_eq!(ask(&input)["ok"], false);
    }

    #[test]
    fn max_results_is_clamped_rather_than_refused() {
        assert_eq!(max_rows(&serde_json::json!({})), DEFAULT_MAX_ROWS);
        assert_eq!(max_rows(&serde_json::json!({"max_results": 0})), 1);
        assert_eq!(
            max_rows(&serde_json::json!({"max_results": 99_999})),
            MAX_ROWS_CEILING
        );
    }

    /// Off-wasm `send` fails, so a fully-populated input gets as far as the
    /// first request and no further — which is exactly the boundary that proves
    /// argument handling ran to completion.
    #[test]
    fn a_complete_input_reaches_the_network_and_reports_its_absence() {
        let out = ask(&full());
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("off-wasm"));
    }
}
