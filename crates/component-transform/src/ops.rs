//! The node-facing layer: one handler per operation.
//!
//! Everything below is argument marshalling and the `{ok, …}` envelope. The
//! transforms themselves live in `core` / `flatten` / `patch` / `select` /
//! `sort`, which are the design extension's modules copied verbatim — so a node
//! and the matching agentic-worker tool cannot disagree about what an operation
//! does.
//!
//! Every failure is a VALUE, never a panic and never a `NodeError`: a flow
//! routes on `ok == false`, whereas a trap takes the whole run down with a
//! message no operator can act on.

use serde_json::{Map, Value};

use crate::core::{TransformError, merge_json_patch, validate_json};
use crate::flatten::{flatten_json, unflatten_json};
use crate::patch::{apply_patch, diff_json};
use crate::select::{dedup_json, omit_json, pick_json};
use crate::sort::sort_json;

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

fn field<'a>(input: &'a Value, name: &str) -> Result<&'a Value, Value> {
    input
        .get(name)
        .ok_or_else(|| err(format!("missing required field `{name}`")))
}

fn object<'a>(input: &'a Value, name: &str) -> Result<&'a Map<String, Value>, Value> {
    match field(input, name)? {
        Value::Object(map) => Ok(map),
        _ => Err(err(format!("`{name}` must be a JSON object"))),
    }
}

fn string_list(input: &Value, name: &str) -> Result<Vec<String>, Value> {
    match field(input, name)? {
        Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| err(format!("`{name}` must be an array of strings")))
            })
            .collect(),
        _ => Err(err(format!("`{name}` must be an array of strings"))),
    }
}

/// `TransformError` is the shared failure type of the copied modules; mapping it
/// here in ONE place keeps every operation's error shape identical.
fn from_transform(e: TransformError) -> Value {
    err(format!("{e:?}"))
}

macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(response) => return response,
        }
    };
}

pub fn json_validate(input: &Value) -> Value {
    let schema = tri!(field(input, "schema"));
    let data = tri!(field(input, "data"));
    // Argument order is (data, schema) — the reverse of how the fields read.
    match validate_json(data, schema) {
        Ok(r) => ok(serde_json::to_value(r).unwrap_or(Value::Null)),
        Err(e) => from_transform(e),
    }
}

pub fn json_merge_patch(input: &Value) -> Value {
    let target = tri!(field(input, "target"));
    let patch = tri!(field(input, "patch"));
    ok(merge_json_patch(target, patch))
}

pub fn json_flatten(input: &Value) -> Value {
    let data = tri!(field(input, "data"));
    let sep = input
        .get("separator")
        .and_then(Value::as_str)
        .unwrap_or(".");
    ok(flatten_json(data, sep))
}

pub fn json_unflatten(input: &Value) -> Value {
    let data = tri!(object(input, "data"));
    let sep = input
        .get("separator")
        .and_then(Value::as_str)
        .unwrap_or(".");
    match unflatten_json(data, sep) {
        Ok(v) => ok(v),
        Err(e) => from_transform(e),
    }
}

pub fn json_dedup(input: &Value) -> Value {
    let items = match tri!(field(input, "data")) {
        Value::Array(items) => items.clone(),
        _ => return err("`data` must be a JSON array"),
    };
    ok(serde_json::to_value(dedup_json(&items)).unwrap_or(Value::Null))
}

pub fn json_pick(input: &Value) -> Value {
    let data = tri!(object(input, "data"));
    let keys = tri!(string_list(input, "keys"));
    ok(pick_json(data, &keys))
}

pub fn json_omit(input: &Value) -> Value {
    let data = tri!(object(input, "data"));
    let keys = tri!(string_list(input, "keys"));
    ok(omit_json(data, &keys))
}

pub fn json_patch(input: &Value) -> Value {
    let data = tri!(field(input, "data"));
    let patch = tri!(field(input, "patch"));
    match apply_patch(data, patch) {
        Ok(v) => ok(v),
        Err(e) => from_transform(e),
    }
}

pub fn json_diff(input: &Value) -> Value {
    let from = tri!(field(input, "from"));
    let to = tri!(field(input, "to"));
    ok(serde_json::to_value(diff_json(from, to)).unwrap_or(Value::Null))
}

pub fn json_sort(input: &Value) -> Value {
    let data = tri!(field(input, "data"));
    let by = input.get("by").and_then(Value::as_str);
    let desc = input
        .get("descending")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match sort_json(data, by, desc) {
        Ok(r) => ok(serde_json::to_value(r).unwrap_or(Value::Null)),
        Err(e) => from_transform(e),
    }
}
