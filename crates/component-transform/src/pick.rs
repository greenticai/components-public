//! `json_pick` — project a JSON object down to a listed set of keys.
//!
//! Semantics are copied from the `greentic.transform` design extension's
//! `pick_json`, deliberately field for field: the same operation must not mean
//! two different things depending on whether an agentic worker calls it as a
//! TOOL or a flow runs it as a NODE. The tests below are the ones that pin it.

use serde_json::{Map, Value};

/// Build a new JSON object containing, in `keys` order, every entry of `obj`
/// whose key appears in `keys`.
///
/// Three rules, each of which a caller can and does depend on:
/// - output key order follows `keys`, not `obj`
/// - a key listed in `keys` but absent from `obj` is SKIPPED, not inserted as
///   null — a missing field and a null field are different facts downstream
/// - duplicate entries in `keys` are de-duplicated
pub fn pick_json(obj: &Map<String, Value>, keys: &[String]) -> Value {
    let mut result = Map::new();
    let mut seen: Vec<&str> = Vec::with_capacity(keys.len());

    for key in keys {
        let k = key.as_str();
        if seen.contains(&k) {
            continue;
        }
        seen.push(k);

        if let Some(val) = obj.get(key) {
            result.insert(key.clone(), val.clone());
        }
    }

    Value::Object(result)
}

/// Run the `json_pick` operation over a node invocation's input.
///
/// Returns the runner's `{ok, ...}` envelope rather than a `Result`: a bad
/// input is a routable outcome for the flow, not a component crash.
pub fn handle_pick(input: &Value) -> Value {
    let data = match input.get("data") {
        Some(Value::Object(map)) => map,
        Some(_) => return err("`data` must be a JSON object"),
        None => return err("missing required field `data`"),
    };

    let keys: Vec<String> = match input.get("keys") {
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item.as_str() {
                    Some(s) => out.push(s.to_string()),
                    None => return err("`keys` must be an array of strings"),
                }
            }
            out
        }
        Some(_) => return err("`keys` must be an array of strings"),
        None => return err("missing required field `keys`"),
    };

    serde_json::json!({ "ok": true, "result": pick_json(data, &keys) })
}

fn err(message: &str) -> Value {
    serde_json::json!({ "ok": false, "error": message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(v: &Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn keeps_present_keys_and_skips_absent() {
        let obj = map(&json!({"a": 1, "b": 2, "c": 3}));
        let keys = vec!["a".to_string(), "c".to_string(), "z".to_string()];
        assert_eq!(
            pick_json(&obj, &keys),
            json!({"a": 1, "c": 3}),
            "an absent key must be skipped, never inserted as null"
        );
    }

    #[test]
    fn duplicate_keys_are_de_duplicated() {
        let obj = map(&json!({"a": 1, "b": 2}));
        let keys = vec!["a".to_string(), "a".to_string()];
        assert_eq!(pick_json(&obj, &keys), json!({"a": 1}));
    }

    /// Output order follows `keys`, not the input object — which is why this
    /// crate requires serde_json's `preserve_order`.
    #[test]
    fn output_order_follows_keys_not_the_object() {
        let obj = map(&json!({"a": 1, "b": 2, "c": 3}));
        let keys = vec!["c".to_string(), "a".to_string()];
        let got = pick_json(&obj, &keys);
        let order: Vec<&String> = got.as_object().unwrap().keys().collect();
        assert_eq!(order, vec!["c", "a"]);
    }

    #[test]
    fn a_non_object_data_is_a_routable_error_not_a_panic() {
        assert_eq!(handle_pick(&json!({"data": 5, "keys": []}))["ok"], false);
        assert_eq!(handle_pick(&json!({"keys": []}))["ok"], false);
    }

    #[test]
    fn a_non_string_key_is_a_routable_error() {
        let out = handle_pick(&json!({"data": {"a": 1}, "keys": [1]}));
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().unwrap().contains("array of strings"));
    }

    #[test]
    fn a_well_formed_call_reports_ok_with_the_projection() {
        let out = handle_pick(&json!({"data": {"a": 1, "b": 2}, "keys": ["b"]}));
        assert_eq!(out["ok"], true);
        assert_eq!(out["result"], json!({"b": 2}));
    }
}
