//! Pure, host-testable flatten / unflatten functions.
//!
//! `flatten_json` — collapses a nested JSON value into a single-level object
//! whose keys are delimiter-joined paths to each leaf.
//!
//! `unflatten_json` — the inverse: rebuilds nested JSON from a flat dot-path
//! map.  Returns `Err(TransformError::InvalidInput)` on contradictory paths
//! or an empty delimiter.
//!
//! No WIT imports; everything here runs on the host for `cargo test`.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::core::TransformError;

// ──────────────────────────────────────────────────────────────────────────────
// flatten_json
// ──────────────────────────────────────────────────────────────────────────────

/// Flatten a nested JSON value into a single-level `Value::Object`.
///
/// Rules:
/// - Scalars (string/number/bool/null) are leaves and are stored directly.
/// - An **empty** object `{}` or array `[]` is treated as a leaf-like terminal
///   and is stored at its path so that round-tripping can restore it.
/// - Non-empty objects / arrays are descended recursively.
/// - A top-level scalar (where `data` is not a container) → `{ "": data }`.
/// - Path segments are object keys and array indices as decimal strings, joined
///   by `delimiter`.
///
/// This function is infallible.
#[must_use]
pub fn flatten_json(data: &Value, delimiter: &str) -> Value {
    let mut out: Map<String, Value> = Map::new();
    flatten_recursive(data, &[], delimiter, &mut out);
    Value::Object(out)
}

/// Recursively populate `out` while descending into `node`.
/// `segments` carries the path accumulated so far.
fn flatten_recursive(
    node: &Value,
    segments: &[&str],
    delimiter: &str,
    out: &mut Map<String, Value>,
) {
    match node {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                let mut next: Vec<&str> = segments.to_vec();
                next.push(key.as_str());
                flatten_recursive(child, &next, delimiter, out);
            }
        }
        Value::Array(arr) if !arr.is_empty() => {
            // Use a local buffer for index strings to satisfy the borrow checker.
            let index_strings: Vec<String> = (0..arr.len()).map(|i| i.to_string()).collect();
            for (i, child) in arr.iter().enumerate() {
                let mut next: Vec<&str> = segments.to_vec();
                next.push(index_strings[i].as_str());
                flatten_recursive(child, &next, delimiter, out);
            }
        }
        // Empty container or scalar — treat as a leaf.
        leaf => {
            let key = segments.join(delimiter);
            out.insert(key, leaf.clone());
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// unflatten_json
// ──────────────────────────────────────────────────────────────────────────────

/// Rebuild a nested JSON value from a flat `data` map whose keys are
/// delimiter-joined paths.
///
/// Rules:
/// - Empty `delimiter` → `Err(InvalidInput)`.
/// - The single entry `{ "": v }` → `v` (inverse of a top-level scalar).
/// - A segment composed entirely of ASCII digits denotes an array index; all
///   other segments denote object keys.
/// - Arrays are grown and padded with `Value::Null` as needed.
/// - A conflict where a node would need to be both an object and an array →
///   `Err(InvalidInput)`.
///
/// # Errors
/// Returns [`TransformError::InvalidInput`] for an empty delimiter or a
/// contradictory structure.
pub fn unflatten_json(data: &Map<String, Value>, delimiter: &str) -> Result<Value, TransformError> {
    if delimiter.is_empty() {
        return Err(TransformError::InvalidInput(
            "delimiter must not be empty".into(),
        ));
    }

    // Special case: single empty-key entry → the value itself.
    if data.len() == 1
        && let Some(v) = data.get("")
    {
        return Ok(v.clone());
    }

    // Build into a mutable root container.
    let mut root = NodeBuf::absent();

    for (flat_key, value) in data {
        let segments: Vec<&str> = flat_key.split(delimiter).collect();
        insert_value(&mut root, &segments, value.clone())?;
    }

    root.into_value()
        .ok_or_else(|| TransformError::InvalidInput("empty flat map produced no output".into()))
}

// ──────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Intermediate mutable representation used while building the nested tree.
enum NodeBuf {
    /// Not yet decided.
    Absent,
    /// A plain scalar / empty-container leaf value.
    Leaf(Value),
    /// A JSON object under construction.
    /// Using `BTreeMap` because `serde_json::Map<K, V>` only implements
    /// `IntoIterator` for `V = Value`.
    Object(BTreeMap<String, NodeBuf>),
    /// A JSON array under construction (sparse; entries may be `Absent`).
    Array(Vec<NodeBuf>),
}

impl NodeBuf {
    fn absent() -> Self {
        Self::Absent
    }

    /// Convert the finished `NodeBuf` into a `Value`, recursively.
    fn into_value(self) -> Option<Value> {
        match self {
            Self::Absent => None,
            Self::Leaf(v) => Some(v),
            Self::Object(map) => {
                let mut out = Map::new();
                for (k, v) in map {
                    out.insert(k, v.into_value().unwrap_or(Value::Null));
                }
                Some(Value::Object(out))
            }
            Self::Array(arr) => {
                let out: Vec<Value> = arr
                    .into_iter()
                    .map(|n| n.into_value().unwrap_or(Value::Null))
                    .collect();
                Some(Value::Array(out))
            }
        }
    }
}

/// All-ASCII-digit check — determines whether a path segment is an array index.
fn is_all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Recursively insert `value` at the position described by `segments` into `node`.
fn insert_value(node: &mut NodeBuf, segments: &[&str], value: Value) -> Result<(), TransformError> {
    if segments.is_empty() {
        // Leaf position — write the value (only valid when node is Absent).
        match node {
            NodeBuf::Absent => {
                *node = NodeBuf::Leaf(value);
                Ok(())
            }
            _ => Err(TransformError::InvalidInput(
                "conflicting paths: a leaf and a container share the same position".into(),
            )),
        }
    } else {
        let head = segments[0];
        let tail = &segments[1..];

        if is_all_digits(head) {
            let idx: usize = head.parse().map_err(|_| {
                TransformError::InvalidInput(format!("array index too large: {head}"))
            })?;

            // Coerce node to Array.
            match node {
                NodeBuf::Absent => {
                    *node = NodeBuf::Array(Vec::new());
                }
                NodeBuf::Array(_) => {}
                _ => {
                    return Err(TransformError::InvalidInput(format!(
                        "conflict: segment '{head}' requires an array but the node is not an array"
                    )));
                }
            }

            let NodeBuf::Array(arr) = node else {
                return Err(TransformError::InvalidInput(
                    "internal: node was coerced to Array but is no longer an Array".into(),
                ));
            };

            // Grow the array with Absent sentinels if needed.
            if arr.len() <= idx {
                arr.resize_with(idx + 1, NodeBuf::absent);
            }

            insert_value(&mut arr[idx], tail, value)
        } else {
            // Object key segment.
            match node {
                NodeBuf::Absent => {
                    *node = NodeBuf::Object(BTreeMap::new());
                }
                NodeBuf::Object(_) => {}
                _ => {
                    return Err(TransformError::InvalidInput(format!(
                        "conflict: segment '{head}' requires an object but the node is not an object"
                    )));
                }
            }

            let NodeBuf::Object(map) = node else {
                return Err(TransformError::InvalidInput(
                    "internal: node was coerced to Object but is no longer an Object".into(),
                ));
            };

            let child = map.entry(head.to_string()).or_insert_with(NodeBuf::absent);
            insert_value(child, tail, value)
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::core::TransformError;

    // ── flatten_json ──────────────────────────────────────────────────────────

    /// Nested objects and arrays are flattened to dot-joined keys.
    /// `{"a":{"b":[1,2]},"c":3}` → `{"a.b.0":1,"a.b.1":2,"c":3}`.
    #[test]
    fn flatten_nested_object_and_array() {
        let data = json!({"a": {"b": [1, 2]}, "c": 3});
        let flat = flatten_json(&data, ".");
        assert_eq!(
            flat,
            json!({"a.b.0": 1, "a.b.1": 2, "c": 3}),
            "nested objects+arrays must produce dot-joined keys"
        );
    }

    /// An empty object `{}` at a non-root position is stored as a leaf at its path.
    #[test]
    fn flatten_empty_object_is_preserved_at_path() {
        let data = json!({"x": {}, "y": 1});
        let flat = flatten_json(&data, ".");
        assert_eq!(
            flat,
            json!({"x": {}, "y": 1}),
            "empty object must be preserved at its path key"
        );
    }

    /// An empty array `[]` at a non-root position is stored as a leaf at its path.
    #[test]
    fn flatten_empty_array_is_preserved_at_path() {
        let data = json!({"list": [], "n": 0});
        let flat = flatten_json(&data, ".");
        assert_eq!(
            flat,
            json!({"list": [], "n": 0}),
            "empty array must be preserved at its path key"
        );
    }

    /// A top-level scalar (not a container) produces `{ "": scalar }`.
    #[test]
    fn flatten_top_level_scalar_produces_empty_key() {
        let data = json!(42);
        let flat = flatten_json(&data, ".");
        assert_eq!(
            flat,
            json!({"": 42}),
            "a top-level scalar must be placed under the empty key"
        );
    }

    /// A custom delimiter is respected.
    #[test]
    fn flatten_custom_delimiter() {
        let data = json!({"a": {"b": 1}});
        let flat = flatten_json(&data, "/");
        assert_eq!(
            flat,
            json!({"a/b": 1}),
            "custom '/' delimiter must be used in the output keys"
        );
    }

    // ── unflatten_json ────────────────────────────────────────────────────────

    /// Inverse of the nested-object-and-array flatten.
    #[test]
    fn unflatten_nested_object_and_array() {
        let flat = json!({"a.b.0": 1, "a.b.1": 2, "c": 3})
            .as_object()
            .unwrap()
            .clone();
        let result = unflatten_json(&flat, ".").expect("must not error");
        assert_eq!(
            result,
            json!({"a": {"b": [1, 2]}, "c": 3}),
            "unflattening must restore the original nested structure"
        );
    }

    /// `{ "": 42 }` → scalar `42`.
    #[test]
    fn unflatten_empty_key_returns_scalar() {
        let flat = json!({"": 42}).as_object().unwrap().clone();
        let result = unflatten_json(&flat, ".").expect("must not error");
        assert_eq!(
            result,
            json!(42),
            "single empty key must unflatten to its value"
        );
    }

    /// Numeric segments produce an array.
    /// `{"a.0":"x","a.1":"y"}` → `{"a":["x","y"]}`.
    #[test]
    fn unflatten_numeric_segments_build_array() {
        let flat = json!({"a.0": "x", "a.1": "y"}).as_object().unwrap().clone();
        let result = unflatten_json(&flat, ".").expect("must not error");
        assert_eq!(
            result,
            json!({"a": ["x", "y"]}),
            "all-digit segments must be treated as array indices"
        );
    }

    /// An empty delimiter must return `Err(InvalidInput)`.
    #[test]
    fn unflatten_empty_delimiter_returns_error() {
        let flat = json!({"a": 1}).as_object().unwrap().clone();
        let result = unflatten_json(&flat, "");
        assert!(
            matches!(result, Err(TransformError::InvalidInput(_))),
            "expected Err(InvalidInput) for an empty delimiter, got: {result:?}"
        );
    }

    /// A conflict — same path used as both a scalar and a sub-path — returns `Err(InvalidInput)`.
    /// `{"a": 1, "a.b": 2}` means 'a' is simultaneously a scalar leaf and a container.
    #[test]
    fn unflatten_conflicting_paths_returns_error() {
        let flat = json!({"a": 1, "a.b": 2}).as_object().unwrap().clone();
        let result = unflatten_json(&flat, ".");
        assert!(
            matches!(result, Err(TransformError::InvalidInput(_))),
            "expected Err(InvalidInput) for conflicting paths, got: {result:?}"
        );
    }

    /// A conflict where a segment is used as both an object key and an array index.
    /// `{"x.0": 1, "x.key": 2}` cannot be a consistent structure.
    #[test]
    fn unflatten_array_vs_object_conflict_returns_error() {
        let flat = json!({"x.0": 1, "x.key": 2}).as_object().unwrap().clone();
        let result = unflatten_json(&flat, ".");
        assert!(
            matches!(result, Err(TransformError::InvalidInput(_))),
            "expected Err(InvalidInput) when a node must be both array and object, got: {result:?}"
        );
    }

    /// Custom '/' delimiter unflatten.
    #[test]
    fn unflatten_custom_delimiter() {
        let flat = json!({"a/b": 1}).as_object().unwrap().clone();
        let result = unflatten_json(&flat, "/").expect("must not error");
        assert_eq!(result, json!({"a": {"b": 1}}));
    }

    // ── round-trip ────────────────────────────────────────────────────────────

    /// For any JSON value whose object keys contain no ".", flattening then
    /// unflattening must yield the original value.
    #[test]
    fn round_trip_flatten_then_unflatten() {
        let original = json!({
            "user": {
                "name": "Ada",
                "age": 36,
                "tags": ["rust", "wasm"],
                "address": {
                    "city": "London",
                    "zip": "EC1A"
                }
            },
            "scores": [10, 20, 30],
            "active": true,
            "ratio": 0.75,
            "note": null
        });

        let flat = flatten_json(&original, ".");
        let flat_map = flat.as_object().expect("flatten must return an object");
        let restored = unflatten_json(flat_map, ".").expect("round-trip must not error");

        assert_eq!(
            restored, original,
            "unflatten(flatten(x)) must equal x for a key-clean nested value"
        );
    }

    /// A top-level empty object is treated as a leaf (no keys to descend) and
    /// flattens to `{ "": {} }` so that unflatten can restore it exactly.
    #[test]
    fn flatten_top_level_empty_object_produces_single_entry() {
        let data = json!({});
        let flat = flatten_json(&data, ".");
        assert_eq!(
            flat,
            json!({"": {}}),
            "a top-level empty object must flatten to {{\"\":{{}}}} for round-trip"
        );
    }

    /// A top-level empty object round-trips through flatten → unflatten.
    #[test]
    fn round_trip_empty_object() {
        let original = json!({});
        let flat = flatten_json(&original, ".");
        let flat_map = flat.as_object().expect("flatten must return an object");
        let restored = unflatten_json(flat_map, ".").expect("must not error");
        assert_eq!(restored, original, "empty object must round-trip correctly");
    }

    /// Empty containers at nested paths survive round-trip.
    #[test]
    fn round_trip_empty_containers() {
        let original = json!({"empty_obj": {}, "empty_arr": [], "val": 1});
        let flat = flatten_json(&original, ".");
        let flat_map = flat.as_object().expect("flatten must return an object");
        let restored = unflatten_json(flat_map, ".").expect("round-trip must not error");
        assert_eq!(
            restored, original,
            "empty containers must survive round-trip"
        );
    }
}
