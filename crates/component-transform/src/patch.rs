//! Pure, host-testable RFC 6902 JSON Patch application and diff.
//!
//! `apply_patch` — deserializes a `json_patch::Patch` from `patch` (an RFC 6902
//! op array) and applies it to a clone of `data`, returning the patched document.
//! Parse failures and application errors (e.g. a failing `test` op or a `remove`
//! on a missing path) are surfaced as `TransformError::InvalidInput`.
//!
//! `diff_json` — computes the RFC 6902 JSON Patch (op array) that transforms
//! `from` into `to`. Uses `json_patch::diff`, which is infallible for any two
//! valid `serde_json::Value` inputs. Returns a `DiffResult` with the patch and
//! a `changed` flag.
//!
//! No WIT imports; everything here runs on the host for `cargo test`.

use serde_json::Value;

use crate::core::TransformError;
use crate::output::DiffResult;

// ──────────────────────────────────────────────────────────────────────────────
// diff_json
// ──────────────────────────────────────────────────────────────────────────────

/// Compute the RFC 6902 JSON Patch that transforms `from` into `to`.
///
/// Returns a [`DiffResult`] whose `patch` field is a JSON array of operation
/// objects (`{"op","path","value?"}`), and whose `changed` field is `true` when
/// the two values differ.
///
/// This function is infallible: `json_patch::diff` accepts any two
/// `serde_json::Value` inputs and always produces a valid `Patch`. The
/// `serde_json::to_value` call on a well-formed `Patch` cannot fail in practice;
/// the `unwrap_or` fallback to an empty array keeps this panic-free regardless.
#[must_use]
pub fn diff_json(from: &Value, to: &Value) -> DiffResult {
    let p = json_patch::diff(from, to);
    let patch = serde_json::to_value(&p).unwrap_or(Value::Array(vec![]));
    let changed = patch.as_array().is_some_and(|a| !a.is_empty());
    DiffResult { patch, changed }
}

// ──────────────────────────────────────────────────────────────────────────────
// apply_patch
// ──────────────────────────────────────────────────────────────────────────────

/// Apply an RFC 6902 JSON Patch to `data` and return the patched document.
///
/// `patch` must be a `Value::Array` of operation objects, each of the form
/// `{"op": "<op>", "path": "<JSON Pointer>", ...}`. Supported ops:
/// `add`, `remove`, `replace`, `move`, `copy`, `test`.
///
/// On success the returned value is the result of applying all ops in order to
/// a clone of `data`.
///
/// # Errors
///
/// - `TransformError::InvalidInput` — `patch` cannot be deserialized as an RFC
///   6902 patch (wrong type, unknown op, missing fields, etc.).
/// - `TransformError::InvalidInput` — a `test` op assertion fails, a `remove`
///   or `replace` targets a path that does not exist, or any other application
///   error from the `json-patch` crate.
pub fn apply_patch(data: &Value, patch: &Value) -> Result<Value, TransformError> {
    let p: json_patch::Patch = serde_json::from_value(patch.clone())
        .map_err(|e| TransformError::InvalidInput(format!("invalid RFC 6902 patch: {e}")))?;

    let mut doc = data.clone();
    json_patch::patch(&mut doc, &p)
        .map_err(|e| TransformError::InvalidInput(format!("patch application failed: {e}")))?;

    Ok(doc)
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::core::TransformError;

    // ── diff_json ─────────────────────────────────────────────────────────────

    /// Two objects that differ by a single value produce changed=true and a
    /// non-empty patch containing a `replace` op at the changed path.
    #[test]
    fn diff_changed_values_returns_non_empty_patch() {
        let from = json!({"a": 1});
        let to = json!({"a": 2});
        let result = diff_json(&from, &to);
        assert!(result.changed, "expected changed=true when values differ");
        let ops = result.patch.as_array().expect("patch must be an array");
        assert!(
            !ops.is_empty(),
            "patch must be non-empty when values differ"
        );
        let has_replace_at_a = ops
            .iter()
            .any(|op| op["op"].as_str() == Some("replace") && op["path"].as_str() == Some("/a"));
        assert!(
            has_replace_at_a,
            "patch must contain a replace op at /a; got: {ops:?}"
        );
    }

    /// Two identical objects produce changed=false and an empty patch array.
    #[test]
    fn diff_identical_values_returns_empty_patch() {
        let from = json!({"a": 1});
        let to = json!({"a": 1});
        let result = diff_json(&from, &to);
        assert!(
            !result.changed,
            "expected changed=false for identical values"
        );
        assert_eq!(
            result.patch,
            json!([]),
            "patch must be an empty array for identical values"
        );
    }

    // ── apply_patch — happy path ──────────────────────────────────────────────

    /// A compound patch with `replace`, `remove`, and `add` ops is applied in
    /// order: `/a` is replaced with 9, `/b` is removed, and `/c` is added with
    /// value 3.
    ///
    /// Input: `{"a":1,"b":2}`
    /// Patch: `[{"op":"replace","path":"/a","value":9},{"op":"remove","path":"/b"},{"op":"add","path":"/c","value":3}]`
    /// Expected: `{"a":9,"c":3}`
    #[test]
    fn patch_replace_remove_add_applied_in_order() {
        let data = json!({"a": 1, "b": 2});
        let patch = json!([
            {"op": "replace", "path": "/a", "value": 9},
            {"op": "remove",  "path": "/b"},
            {"op": "add",     "path": "/c", "value": 3}
        ]);
        let got = apply_patch(&data, &patch).expect("valid patch must not error");
        assert_eq!(
            got,
            json!({"a": 9, "c": 3}),
            "replace+remove+add must yield {{\"a\":9,\"c\":3}}"
        );
    }

    // ── apply_patch — test op failure ─────────────────────────────────────────

    /// A `test` op that fails because the actual value differs from `value`
    /// must return `Err(TransformError::InvalidInput)`.
    ///
    /// Input: `{"a":1}`
    /// Patch: `[{"op":"test","path":"/a","value":999}]`
    #[test]
    fn patch_failing_test_op_returns_invalid_input() {
        let data = json!({"a": 1});
        let patch = json!([{"op": "test", "path": "/a", "value": 999}]);
        let result = apply_patch(&data, &patch);
        assert!(
            matches!(result, Err(TransformError::InvalidInput(_))),
            "expected Err(InvalidInput) for failing test op, got: {result:?}"
        );
    }

    // ── apply_patch — remove on missing path ──────────────────────────────────

    /// A `remove` op targeting a path that does not exist in the document must
    /// return `Err(TransformError::InvalidInput)`.
    ///
    /// Input: `{"a":1}`
    /// Patch: `[{"op":"remove","path":"/nope"}]`
    #[test]
    fn patch_remove_on_missing_path_returns_invalid_input() {
        let data = json!({"a": 1});
        let patch = json!([{"op": "remove", "path": "/nope"}]);
        let result = apply_patch(&data, &patch);
        assert!(
            matches!(result, Err(TransformError::InvalidInput(_))),
            "expected Err(InvalidInput) for remove on missing path, got: {result:?}"
        );
    }

    // ── apply_patch — malformed patch ─────────────────────────────────────────

    /// A value that is not an RFC 6902 op array (e.g. a plain object) cannot
    /// be deserialized as a `json_patch::Patch` and must return
    /// `Err(TransformError::InvalidInput)`.
    ///
    /// Patch: `{"op":"bogus"}` (an object, not an array of ops)
    #[test]
    fn patch_malformed_patch_not_an_op_array_returns_invalid_input() {
        let data = json!({"a": 1});
        let patch = json!({"op": "bogus"});
        let result = apply_patch(&data, &patch);
        assert!(
            matches!(result, Err(TransformError::InvalidInput(_))),
            "expected Err(InvalidInput) for malformed patch, got: {result:?}"
        );
    }
}
