//! Pure, host-testable JSON array sort function.
//!
//! `sort_json` — sorts a JSON array using a total order over JSON values.
//!   Null < Bool < Number < String < Array < Object.
//!   Within a kind: bools false<true, numbers by f64, strings lexicographically,
//!   arrays and objects by their compact JSON string as a stable fallback.
//!   Optional `by` key sorts arrays of objects by `element[by]` (missing key
//!   treated as Null, which sorts first in asc). Uses a stable sort.
//!
//! No WIT imports; everything here runs on the host for `cargo test`.

use std::cmp::Ordering;

use serde_json::Value;

use crate::core::TransformError;
use crate::output::SortResult;

// ──────────────────────────────────────────────────────────────────────────────
// kind rank
// ──────────────────────────────────────────────────────────────────────────────

fn kind_rank(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// json_cmp — total order over serde_json::Value
// ──────────────────────────────────────────────────────────────────────────────

/// Total order over JSON values.
///
/// Kind precedence: `Null(0) < Bool(1) < Number(2) < String(3) < Array(4) < Object(5)`.
/// Within a kind:
/// - Bool: `false < true`.
/// - Number: compare as `f64`; NaN is treated as equal to NaN (no panic).
/// - String: lexicographic byte order.
/// - Array / Object: compare their compact JSON representation as a stable fallback.
fn json_cmp(a: &Value, b: &Value) -> Ordering {
    let ra = kind_rank(a);
    let rb = kind_rank(b);
    if ra != rb {
        return ra.cmp(&rb);
    }
    // Same kind — compare within kind.
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Number(x), Value::Number(y)) => {
            let fx = x.as_f64().unwrap_or(0.0);
            let fy = y.as_f64().unwrap_or(0.0);
            fx.partial_cmp(&fy).unwrap_or(Ordering::Equal)
        }
        (Value::String(x), Value::String(y)) => x.cmp(y),
        // Array and Object: use compact JSON string as a stable fallback.
        _ => {
            let sa = serde_json::to_string(a).unwrap_or_default();
            let sb = serde_json::to_string(b).unwrap_or_default();
            sa.cmp(&sb)
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// sort_json
// ──────────────────────────────────────────────────────────────────────────────

/// Sort a JSON array and return the sorted array plus its element count.
///
/// `data` must be a `Value::Array`; any other type returns
/// [`TransformError::InvalidInput`].
///
/// If `by` is `Some(key)`, elements are sorted by `element[key]` (each element
/// should be a JSON object; missing keys are treated as `Null`, which sorts
/// first in ascending order). If `by` is `None`, the elements themselves are
/// compared.
///
/// `descending` reverses the sort order when `true`.
///
/// Uses a stable sort; equal elements preserve their original relative order.
///
/// # Errors
///
/// Returns [`TransformError::InvalidInput`] when `data` is not a JSON array.
pub fn sort_json(
    data: &Value,
    by: Option<&str>,
    descending: bool,
) -> Result<SortResult, TransformError> {
    let arr = data
        .as_array()
        .ok_or_else(|| TransformError::InvalidInput("json_sort requires an array".into()))?;

    let mut sorted: Vec<Value> = arr.clone();

    sorted.sort_by(|a, b| {
        let ka: &Value;
        let kb: &Value;
        // Temporaries to extend lifetime when `by` is Some.
        let null_a;
        let null_b;
        if let Some(key) = by {
            null_a = Value::Null;
            null_b = Value::Null;
            ka = a.get(key).unwrap_or(&null_a);
            kb = b.get(key).unwrap_or(&null_b);
        } else {
            ka = a;
            kb = b;
        }
        let ord = json_cmp(ka, kb);
        if descending { ord.reverse() } else { ord }
    });

    let count = sorted.len();
    Ok(SortResult {
        sorted: Value::Array(sorted),
        count,
    })
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::core::TransformError;

    // ── sort scalars ascending ────────────────────────────────────────────────

    /// `[3,1,2]` sorted ascending by element value yields `[1,2,3]`, count 3.
    #[test]
    fn sort_scalars_ascending() {
        let data = json!([3, 1, 2]);
        let result = sort_json(&data, None, false).expect("valid array must not error");
        assert_eq!(result.sorted, json!([1, 2, 3]), "expected ascending order");
        assert_eq!(result.count, 3);
    }

    // ── sort scalars descending ───────────────────────────────────────────────

    /// `[3,1,2]` sorted descending yields `[3,2,1]`, count 3.
    #[test]
    fn sort_scalars_descending() {
        let data = json!([3, 1, 2]);
        let result = sort_json(&data, None, true).expect("valid array must not error");
        assert_eq!(result.sorted, json!([3, 2, 1]), "expected descending order");
        assert_eq!(result.count, 3);
    }

    // ── sort objects by key ───────────────────────────────────────────────────

    /// `[{"n":2},{"n":1}]` sorted ascending by key `"n"` yields `[{"n":1},{"n":2}]`.
    #[test]
    fn sort_objects_by_key_ascending() {
        let data = json!([{"n": 2}, {"n": 1}]);
        let result = sort_json(&data, Some("n"), false).expect("valid array must not error");
        assert_eq!(
            result.sorted,
            json!([{"n": 1}, {"n": 2}]),
            "expected objects sorted by n asc"
        );
    }

    // ── sort objects with missing key ─────────────────────────────────────────

    /// When one element is missing the `by` key it is treated as Null, which
    /// sorts first in ascending order — no panic.
    #[test]
    fn sort_missing_by_key_treated_as_null_no_panic() {
        let data = json!([{"n": 2}, {"other": 1}, {"n": 1}]);
        let result = sort_json(&data, Some("n"), false).expect("valid array must not error");
        // {"other":1} has no "n" → Null → sorts first.
        let sorted = result.sorted.as_array().expect("must be array");
        assert_eq!(
            sorted[0],
            json!({"other": 1}),
            "null-keyed element must be first"
        );
        assert_eq!(result.count, 3);
    }

    // ── non-array input → Err ─────────────────────────────────────────────────

    /// Passing a non-array value returns `Err(InvalidInput)` — no panic.
    #[test]
    fn sort_non_array_returns_invalid_input() {
        let data = json!({"x": 1});
        let result = sort_json(&data, None, false);
        assert!(
            matches!(result, Err(TransformError::InvalidInput(_))),
            "expected Err(InvalidInput) for non-array, got: {result:?}"
        );
    }

    // ── mixed type array sorts by kind order without panic ────────────────────

    /// `[2,"a",true,null]` sorted ascending follows the kind order
    /// `Null < Bool < Number < String` — no panic.
    #[test]
    fn sort_mixed_types_by_kind_order_no_panic() {
        let data = json!([2, "a", true, null]);
        let result = sort_json(&data, None, false).expect("valid array must not error");
        assert_eq!(
            result.sorted,
            json!([null, true, 2, "a"]),
            "expected kind-order: null < bool < number < string"
        );
    }
}
