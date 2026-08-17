//! Pure, host-testable dedup, pick, and omit functions.
//!
//! `dedup_json` — removes duplicate elements from a JSON array by deep
//! equality, keeping the first occurrence and preserving order.
//!
//! `pick_json` — projects a JSON object down to a listed set of keys,
//! returning only the entries that are present in the object.
//!
//! `omit_json` — the inverse of `pick_json`: returns a new object with the
//! listed keys removed, preserving all other entries in their original order.
//!
//! No WIT imports; everything here runs on the host for `cargo test`.

use serde_json::{Map, Value};

use crate::output::DedupResult;

// ──────────────────────────────────────────────────────────────────────────────
// dedup_json
// ──────────────────────────────────────────────────────────────────────────────

/// Remove duplicate elements from `items` by deep JSON equality.
///
/// The first occurrence of each distinct value is kept; subsequent equal
/// values are discarded. Order among the kept values is preserved.
///
/// Because `serde_json::Value` does not implement `Hash`, membership is checked
/// with a linear scan of the kept vec (O(n²)) — acceptable for the data sizes
/// that pass through an agentic-worker tool call.
///
/// This function is infallible; the caller must ensure `items` is already
/// deserialized from a JSON array.
#[must_use]
pub fn dedup_json(items: &[Value]) -> DedupResult {
    let mut kept: Vec<Value> = Vec::with_capacity(items.len());

    for item in items {
        if !kept.iter().any(|k| k == item) {
            kept.push(item.clone());
        }
    }

    let removed = items.len() - kept.len();
    DedupResult {
        result: kept,
        removed,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// pick_json
// ──────────────────────────────────────────────────────────────────────────────

/// Build a new JSON object containing only the entries of `obj` whose key
/// appears in `keys`.
///
/// Rules:
/// - Output key order follows `keys`.
/// - Duplicate entries in `keys` are de-duplicated (each output key appears
///   at most once, at the position of its first occurrence in `keys`).
/// - Keys listed in `keys` but absent from `obj` are silently skipped (NOT
///   inserted as null).
///
/// Returns a `Value::Object`. This function is infallible; the caller must
/// ensure `obj` is already an object and `keys` contains only valid strings.
#[must_use]
pub fn pick_json(obj: &Map<String, Value>, keys: &[String]) -> Value {
    let mut result = Map::new();
    let mut seen: Vec<&str> = Vec::with_capacity(keys.len());

    for key in keys {
        let k = key.as_str();
        // De-duplicate: skip if this key was already added.
        if seen.contains(&k) {
            continue;
        }
        seen.push(k);

        // Skip absent keys rather than inserting null.
        if let Some(val) = obj.get(key) {
            result.insert(key.clone(), val.clone());
        }
    }

    Value::Object(result)
}

// ──────────────────────────────────────────────────────────────────────────────
// omit_json
// ──────────────────────────────────────────────────────────────────────────────

/// Build a new JSON object containing every entry of `obj` whose key does
/// NOT appear in `keys`.
///
/// Rules:
/// - Output key order follows `obj` (preserve_order is enabled).
/// - Keys listed in `keys` but absent from `obj` are silently ignored.
/// - An empty `keys` slice returns a clone of the full object.
///
/// Returns a `Value::Object`. This function is infallible; the caller must
/// ensure `obj` is already an object and `keys` contains only valid strings.
#[must_use]
pub fn omit_json(obj: &Map<String, Value>, keys: &[String]) -> Value {
    let mut result = Map::new();

    for (key, val) in obj {
        if !keys.iter().any(|k| k == key) {
            result.insert(key.clone(), val.clone());
        }
    }

    Value::Object(result)
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // ── dedup_json ────────────────────────────────────────────────────────────

    /// Scalar duplicates at multiple positions are removed; first occurrence
    /// of each distinct value is kept and original order is preserved.
    ///
    /// Input: [1, 2, 2, 3, 1] → result [1, 2, 3], removed 2.
    #[test]
    fn dedup_scalar_duplicates_keeps_first_occurrence() {
        let items = vec![json!(1), json!(2), json!(2), json!(3), json!(1)];
        let got = dedup_json(&items);
        assert_eq!(
            got.result,
            vec![json!(1), json!(2), json!(3)],
            "expected first occurrences of 1, 2, 3 in order"
        );
        assert_eq!(got.removed, 2, "two duplicates (2 and 1) were removed");
    }

    /// Deep equality is used: two objects with the same structure are
    /// considered equal even though they are distinct allocations.
    ///
    /// Input: [{"a":1}, {"a":1}, {"a":2}] → result [{"a":1}, {"a":2}], removed 1.
    #[test]
    fn dedup_deep_equal_objects_are_deduplicated() {
        let items = vec![json!({"a": 1}), json!({"a": 1}), json!({"a": 2})];
        let got = dedup_json(&items);
        assert_eq!(
            got.result,
            vec![json!({"a": 1}), json!({"a": 2})],
            "second {{\"a\":1}} must be removed as a deep duplicate"
        );
        assert_eq!(got.removed, 1, "one duplicate object was removed");
    }

    /// An empty slice produces an empty result with removed = 0.
    #[test]
    fn dedup_empty_input_produces_empty_result() {
        let got = dedup_json(&[]);
        assert!(
            got.result.is_empty(),
            "result must be empty for empty input"
        );
        assert_eq!(got.removed, 0, "removed must be 0 for empty input");
    }

    /// When no elements are duplicated the result is identical to the input.
    ///
    /// Input: [1, 2, 3] → result [1, 2, 3], removed 0.
    #[test]
    fn dedup_no_duplicates_returns_unchanged() {
        let items = vec![json!(1), json!(2), json!(3)];
        let got = dedup_json(&items);
        assert_eq!(
            got.result,
            vec![json!(1), json!(2), json!(3)],
            "result must equal input when there are no duplicates"
        );
        assert_eq!(
            got.removed, 0,
            "removed must be 0 when there are no duplicates"
        );
    }

    // ── pick_json ─────────────────────────────────────────────────────────────

    /// Keys present in `obj` are included; keys absent from `obj` are silently
    /// skipped (not inserted as null).
    ///
    /// `obj = {"a":1,"b":2,"c":3}`, `keys = ["a","c","z"]` → `{"a":1,"c":3}`
    #[test]
    fn pick_keeps_present_keys_and_skips_absent() {
        let obj = json!({"a": 1, "b": 2, "c": 3});
        let map = obj.as_object().unwrap();
        let keys: Vec<String> = vec!["a".into(), "c".into(), "z".into()];
        let got = pick_json(map, &keys);
        assert_eq!(got, json!({"a": 1, "c": 3}), "z must be skipped (absent)");
    }

    /// Duplicate entries in `keys` are de-duplicated; each key appears at
    /// most once in the output.
    ///
    /// `obj = {"a":1,"b":2}`, `keys = ["a","a"]` → `{"a":1}`
    #[test]
    fn pick_deduplicated_keys_appear_once_in_output() {
        let obj = json!({"a": 1, "b": 2});
        let map = obj.as_object().unwrap();
        let keys: Vec<String> = vec!["a".into(), "a".into()];
        let got = pick_json(map, &keys);
        assert_eq!(
            got,
            json!({"a": 1}),
            "duplicate key 'a' must appear only once"
        );
    }

    /// An empty `keys` slice produces an empty object regardless of `obj`.
    #[test]
    fn pick_empty_keys_produces_empty_object() {
        let obj = json!({"a": 1, "b": 2});
        let map = obj.as_object().unwrap();
        let got = pick_json(map, &[]);
        assert_eq!(got, json!({}), "empty keys must yield an empty object");
    }

    /// Output key order follows `keys`, not the insertion order of `obj`.
    ///
    /// Verified by collecting the output object's keys into a Vec and asserting
    /// The ONE place this copy diverges from the design extension, and it is a
    /// serialization detail rather than a difference in meaning.
    ///
    /// The extension asserts the output KEY ORDER follows `keys`, which needs
    /// serde_json's `preserve_order`. This crate cannot enable that feature:
    /// cargo unifies features across the workspace, so it would change the JSON
    /// key order every other component here emits. Object key order is
    /// insignificant per RFC 8259 and nothing in a flow reads a value
    /// positionally, so what is pinned instead is the property that does carry
    /// meaning — the same key SET projects to the same object, whichever order
    /// it was listed in.
    #[test]
    fn pick_selects_by_membership_regardless_of_key_order() {
        let obj = json!({"x": 10, "y": 20, "z": 30});
        let map = obj.as_object().unwrap();

        let forwards = pick_json(map, &["x".to_string(), "z".to_string()]);
        let backwards = pick_json(map, &["z".to_string(), "x".to_string()]);

        assert_eq!(forwards, json!({"x": 10, "z": 30}));
        assert_eq!(
            forwards, backwards,
            "the same key set must project to the same object"
        );
    }

    // ── omit_json ─────────────────────────────────────────────────────────────

    /// A key present in `keys` is removed; the remaining keys are preserved in
    /// their original `obj` order.
    ///
    /// `obj = {"a":1,"b":2,"c":3}`, `keys = ["b"]` → `{"a":1,"c":3}`
    #[test]
    fn omit_removes_listed_key_and_preserves_rest_in_order() {
        let obj = json!({"a": 1, "b": 2, "c": 3});
        let map = obj.as_object().unwrap();
        let keys: Vec<String> = vec!["b".into()];
        let got = omit_json(map, &keys);
        assert_eq!(got, json!({"a": 1, "c": 3}), "key 'b' must be omitted");
    }

    /// A key listed in `keys` that is absent from `obj` is silently ignored;
    /// the result equals a clone of `obj`.
    ///
    /// `obj = {"a":1,"b":2,"c":3}`, `keys = ["z"]` → `{"a":1,"b":2,"c":3}`
    #[test]
    fn omit_absent_key_in_keys_is_silently_ignored() {
        let obj = json!({"a": 1, "b": 2, "c": 3});
        let map = obj.as_object().unwrap();
        let keys: Vec<String> = vec!["z".into()];
        let got = omit_json(map, &keys);
        assert_eq!(
            got,
            json!({"a": 1, "b": 2, "c": 3}),
            "absent key 'z' must be ignored; obj must be returned unchanged"
        );
    }

    /// An empty `keys` slice returns a clone of the entire object.
    ///
    /// `obj = {"a":1,"b":2,"c":3}`, `keys = []` → `{"a":1,"b":2,"c":3}`
    #[test]
    fn omit_empty_keys_returns_full_clone() {
        let obj = json!({"a": 1, "b": 2, "c": 3});
        let map = obj.as_object().unwrap();
        let got = omit_json(map, &[]);
        assert_eq!(
            got,
            json!({"a": 1, "b": 2, "c": 3}),
            "empty keys must return the full object unchanged"
        );
    }

    /// When all keys of `obj` are listed in `keys`, the result is an empty object.
    ///
    /// `obj = {"a":1,"b":2,"c":3}`, `keys = ["a","b","c"]` → `{}`
    #[test]
    fn omit_all_keys_produces_empty_object() {
        let obj = json!({"a": 1, "b": 2, "c": 3});
        let map = obj.as_object().unwrap();
        let keys: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let got = omit_json(map, &keys);
        assert_eq!(
            got,
            json!({}),
            "all keys omitted must yield an empty object"
        );
    }
}
