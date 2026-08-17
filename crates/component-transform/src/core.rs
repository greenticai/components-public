//! Pure, host-testable JSON transform functions.
//!
//! Implements json_validate, jsonpath_query, json_merge_patch, and json_reshape,
//! and re-exports from the other pure modules so call sites import from one place.
//! No WIT imports; everything here runs on the host for `cargo test`.
//! WIT wiring lives in `lib.rs` (invoke-tool dispatch).

#[cfg(feature = "jsonpath")]
use crate::output::QueryResult;
use crate::output::ValidateResult;

#[allow(unused_imports)]
pub use crate::flatten::{flatten_json, unflatten_json};
#[allow(unused_imports)]
pub use crate::patch::{apply_patch, diff_json};
#[allow(unused_imports)]
pub use crate::select::{dedup_json, omit_json, pick_json};
#[allow(unused_imports)]
pub use crate::sort::sort_json;

// ──────────────────────────────────────────────────────────────────────────────
// Error type
// ──────────────────────────────────────────────────────────────────────────────

/// Errors produced by the transform functions.
///
/// `InvalidPath` is only constructed by the two `jsonpath`-gated functions, and
/// the `Invalid*` prefix is the design extension's own naming — kept so the
/// copied modules stay diffable against their source.
#[allow(dead_code)]
#[allow(clippy::enum_variant_names)]
#[derive(Debug, PartialEq)]
pub enum TransformError {
    /// The supplied JSON Schema failed to compile (e.g. invalid `type` value).
    InvalidSchema(String),
    /// The supplied JSONPath expression could not be parsed.
    InvalidPath(String),
    /// The supplied input is structurally invalid for the operation
    /// (e.g. empty delimiter, conflicting unflatten paths).
    InvalidInput(String),
}

impl std::fmt::Display for TransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchema(msg) => write!(f, "invalid schema: {msg}"),
            Self::InvalidPath(msg) => write!(f, "invalid path: {msg}"),
            Self::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
        }
    }
}

impl std::error::Error for TransformError {}

// ──────────────────────────────────────────────────────────────────────────────
// validate_json
// ──────────────────────────────────────────────────────────────────────────────

/// Validate `data` against a JSON Schema `schema`.
///
/// Returns [`ValidateResult`] with `valid=true` and an empty `errors` vector
/// when the document passes, or `valid=false` and one string per validation
/// error otherwise.
///
/// # Errors
/// Returns [`TransformError::InvalidSchema`] when `schema` cannot be compiled
/// as a valid JSON Schema (e.g. `{"type": 123}` — a non-string `type` value).
pub fn validate_json(
    data: &serde_json::Value,
    schema: &serde_json::Value,
) -> Result<ValidateResult, TransformError> {
    let compiled = jsonschema::JSONSchema::compile(schema)
        .map_err(|e| TransformError::InvalidSchema(e.to_string()))?;

    let errors: Vec<String> = match compiled.validate(data) {
        Ok(()) => vec![],
        Err(iter) => iter.map(|err| format!("{err}")).collect(),
    };

    Ok(ValidateResult {
        valid: errors.is_empty(),
        errors,
    })
}

// ──────────────────────────────────────────────────────────────────────────────
// query_jsonpath
// ──────────────────────────────────────────────────────────────────────────────

/// Select values from `data` using a JSONPath `path` expression.
///
/// Returns [`QueryResult`] with the matched values (owned clones) and their
/// count.  An empty result is valid — `count=0`, `matches=[]`.
///
/// # Errors
/// Returns [`TransformError::InvalidPath`] when `path` is not a valid JSONPath
/// expression (e.g. `"$["`).
// `query_jsonpath` and `reshape_json` are the two functions in this file that
// need `jsonpath_lib`, and that crate enables `serde_json/preserve_order`
// transitively. Cargo unifies features across the workspace, so pulling it in
// changes the JSON key order EVERY component in this repo emits — it fails
// component-chat2data-translator's tests immediately, and would silently alter
// ten components' output in production.
//
// So they are compiled out rather than deleted: the code and its tests are the
// design extension's verbatim, and should come back intact once the dependency
// question is settled (a jsonpath crate that does not force the feature, or
// moving this crate out of the workspace). Their two operations are absent from
// `OPS` in lib.rs meanwhile, which is what keeps the component honest about
// what it can do.
#[cfg(feature = "jsonpath")]
pub fn query_jsonpath(data: &serde_json::Value, path: &str) -> Result<QueryResult, TransformError> {
    let refs =
        jsonpath_lib::select(data, path).map_err(|e| TransformError::InvalidPath(e.to_string()))?;

    let matches: Vec<serde_json::Value> = refs.into_iter().cloned().collect();
    let count = matches.len();

    Ok(QueryResult { matches, count })
}

// ──────────────────────────────────────────────────────────────────────────────
// merge_json_patch
// ──────────────────────────────────────────────────────────────────────────────

/// Apply an RFC 7386 JSON Merge Patch to `target` and return the merged value.
///
/// Rules:
/// - If `patch` is **not** an object, return `patch.clone()` (full replacement).
/// - If `patch` is an object, start from `target` if it is also an object,
///   otherwise start from an empty object `{}`.  For each `(k, v)` in `patch`:
///   - if `v` is `null`, remove `k` from the result;
///   - otherwise set `result[k] = merge_json_patch(result.get(k), v)`.
///
/// This function is infallible; it never returns an error.
#[must_use]
pub fn merge_json_patch(
    target: &serde_json::Value,
    patch: &serde_json::Value,
) -> serde_json::Value {
    let serde_json::Value::Object(patch_map) = patch else {
        // Non-object patch replaces target entirely.
        return patch.clone();
    };

    // Coerce target to an object map (empty when target is not an object).
    let mut result = match target {
        serde_json::Value::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };

    for (key, patch_val) in patch_map {
        if patch_val.is_null() {
            result.remove(key);
        } else {
            let current = result.get(key).cloned().unwrap_or(serde_json::Value::Null);
            result.insert(key.clone(), merge_json_patch(&current, patch_val));
        }
    }

    serde_json::Value::Object(result)
}

// ──────────────────────────────────────────────────────────────────────────────
// reshape_json
// ──────────────────────────────────────────────────────────────────────────────

/// Build a new JSON object by projecting `data` through a `mapping` of
/// `output_key → JSONPath expression`.
///
/// For each `(out_key, path_val)` in `mapping`:
/// - `path_val` must be a `Value::String`; any other type returns
///   [`TransformError::InvalidPath`] naming the offending key.
/// - The JSONPath is evaluated against `data` via `jsonpath_lib::select`.
///   A parse error returns [`TransformError::InvalidPath`].
/// - The output key is set to the **first** match (cloned), or `Value::Null`
///   when there are no matches.
///
/// Returns a `Value::Object` on success.
///
/// # Errors
/// Returns [`TransformError::InvalidPath`] for invalid or non-string mapping
/// values, or for a JSONPath expression that fails to parse.
#[cfg(feature = "jsonpath")]
pub fn reshape_json(
    data: &serde_json::Value,
    mapping: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, TransformError> {
    let mut result = serde_json::Map::new();

    for (out_key, path_val) in mapping {
        let path_str = path_val.as_str().ok_or_else(|| {
            TransformError::InvalidPath(format!(
                "mapping value for key '{out_key}' must be a JSONPath string, got: {path_val}"
            ))
        })?;

        let matches = jsonpath_lib::select(data, path_str)
            .map_err(|e| TransformError::InvalidPath(format!("{e}")))?;

        let first = matches
            .into_iter()
            .next()
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        result.insert(out_key.clone(), first);
    }

    Ok(serde_json::Value::Object(result))
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // ── validate_json — happy path ────────────────────────────────────────────

    /// A document that satisfies the schema returns valid=true with no errors.
    #[test]
    fn validate_passing_document_is_valid() {
        let schema = json!({"type": "object", "required": ["a"]});
        let data = json!({"a": 1});
        let result = validate_json(&data, &schema).expect("schema must compile");
        assert!(
            result.valid,
            "expected valid=true for a conforming document"
        );
        assert!(
            result.errors.is_empty(),
            "expected no errors for a conforming document"
        );
    }

    // ── validate_json — validation failure ───────────────────────────────────

    /// A document missing a required field returns valid=false with ≥1 error
    /// that mentions the missing field name.
    #[test]
    fn validate_missing_required_field_is_invalid() {
        let schema = json!({"type": "object", "required": ["a"]});
        let data = json!({});
        let result = validate_json(&data, &schema).expect("schema must compile");
        assert!(
            !result.valid,
            "expected valid=false when required field is absent"
        );
        assert!(
            !result.errors.is_empty(),
            "expected at least one validation error"
        );
        let mentions_field = result.errors.iter().any(|e| e.contains('a'));
        assert!(
            mentions_field,
            "at least one error must mention the missing field 'a'; got: {:?}",
            result.errors
        );
    }

    // ── validate_json — invalid schema ───────────────────────────────────────

    /// A schema with a non-string `type` value (e.g. `{"type": 123}`) cannot
    /// be compiled and must return Err(TransformError::InvalidSchema(_)).
    #[test]
    fn validate_invalid_schema_returns_error() {
        // jsonschema 0.18 rejects {"type": 123} at compile time because the
        // `type` keyword requires a string or array of strings.
        let schema = json!({"type": 123});
        let result = validate_json(&json!({}), &schema);
        assert!(
            matches!(result, Err(TransformError::InvalidSchema(_))),
            "expected Err(InvalidSchema), got: {result:?}"
        );
    }

    // ── query_jsonpath — matching values ─────────────────────────────────────

    /// Selecting nested fields with a wildcard returns all matching values in
    /// the correct order.
    #[cfg(feature = "jsonpath")]
    #[test]
    fn query_wildcard_returns_all_matches() {
        let data = json!({"items": [{"n": 1}, {"n": 2}]});
        let result = query_jsonpath(&data, "$.items[*].n").expect("path must be valid");
        assert_eq!(result.count, 2, "expected 2 matches");
        assert_eq!(
            result.matches,
            vec![json!(1), json!(2)],
            "matches must be [1, 2] in order"
        );
    }

    // ── query_jsonpath — no matches ───────────────────────────────────────────

    /// A valid path that matches nothing returns count=0 and an empty vec.
    #[cfg(feature = "jsonpath")]
    #[test]
    fn query_no_match_returns_empty() {
        let data = json!({"items": [{"n": 1}, {"n": 2}]});
        let result = query_jsonpath(&data, "$.missing").expect("path must be valid");
        assert_eq!(result.count, 0, "expected count=0 for a non-matching path");
        assert!(
            result.matches.is_empty(),
            "expected empty matches for a non-matching path"
        );
    }

    // ── query_jsonpath — invalid path ─────────────────────────────────────────

    /// An unparseable JSONPath expression must return Err(TransformError::InvalidPath(_)).
    #[cfg(feature = "jsonpath")]
    #[test]
    fn query_invalid_path_returns_error() {
        let data = json!({"a": 1});
        let result = query_jsonpath(&data, "$[");
        assert!(
            matches!(result, Err(TransformError::InvalidPath(_))),
            "expected Err(InvalidPath) for a malformed path, got: {result:?}"
        );
    }

    // ── merge_json_patch ──────────────────────────────────────────────────────

    /// RFC 7386 §1: a null value in patch removes the key from target.
    /// {"a":1,"b":2} merged with {"b":null,"c":3} must yield {"a":1,"c":3}.
    #[test]
    fn merge_patch_null_deletes_key_and_adds_new() {
        let target = json!({"a": 1, "b": 2});
        let patch = json!({"b": null, "c": 3});
        let result = merge_json_patch(&target, &patch);
        assert_eq!(result, json!({"a": 1, "c": 3}));
    }

    /// RFC 7386 §1: nested objects are merged recursively.
    /// {"a":{"x":1}} merged with {"a":{"y":2}} must yield {"a":{"x":1,"y":2}}.
    #[test]
    fn merge_patch_nested_objects_merge_recursively() {
        let target = json!({"a": {"x": 1}});
        let patch = json!({"a": {"y": 2}});
        let result = merge_json_patch(&target, &patch);
        assert_eq!(result, json!({"a": {"x": 1, "y": 2}}));
    }

    /// RFC 7386 §1: a non-object patch replaces target entirely.
    /// {"a":1} merged with 42 must yield 42.
    #[test]
    fn merge_patch_non_object_patch_replaces_target() {
        let target = json!({"a": 1});
        let patch = json!(42);
        let result = merge_json_patch(&target, &patch);
        assert_eq!(result, json!(42));
    }

    /// RFC 7386 §1: when patch is an object but target is not, target is
    /// coerced to {} before merging.
    /// 7 merged with {"a":1} must yield {"a":1}.
    #[test]
    fn merge_patch_non_object_target_coerced_to_empty_object() {
        let target = json!(7);
        let patch = json!({"a": 1});
        let result = merge_json_patch(&target, &patch);
        assert_eq!(result, json!({"a": 1}));
    }

    /// RFC 7386 §1: a null patch-value whose current value is itself an object
    /// removes the entire key (does not recurse into the object).
    /// `{"a":{"b":1}}` merged with `{"a":null}` must yield `{}`.
    #[test]
    fn merge_patch_null_deletes_object_valued_key() {
        let target = json!({"a": {"b": 1}});
        let patch = json!({"a": null});
        let result = merge_json_patch(&target, &patch);
        assert_eq!(result, json!({}));
    }

    /// RFC 7386 §1: null can delete a single nested key while leaving siblings.
    /// `{"a":{"x":1,"y":2}}` merged with `{"a":{"y":null}}` must yield
    /// `{"a":{"x":1}}`.
    #[test]
    fn merge_patch_null_deletes_nested_key() {
        let target = json!({"a": {"x": 1, "y": 2}});
        let patch = json!({"a": {"y": null}});
        let result = merge_json_patch(&target, &patch);
        assert_eq!(result, json!({"a": {"x": 1}}));
    }

    /// RFC 7386 §1: when patch is a scalar `null` (not an object), the target
    /// is replaced entirely.  `{"a":1}` merged with `null` must yield `null`.
    #[test]
    fn merge_patch_scalar_null_patch_replaces_target() {
        let target = json!({"a": 1});
        let patch = json!(null);
        let result = merge_json_patch(&target, &patch);
        assert_eq!(result, json!(null));
    }

    // ── reshape_json ─────────────────────────────────────────────────────────

    /// A valid mapping with matching paths extracts the first value for each key.
    ///
    /// Input: `{"user":{"name":"Ada","age":36},"tags":["x","y"]}`
    /// Mapping: `{"n":"$.user.name","first_tag":"$.tags[0]"}`
    /// Expected: `{"n":"Ada","first_tag":"x"}`
    #[cfg(feature = "jsonpath")]
    #[test]
    fn reshape_extracts_first_match_for_each_key() {
        let data = json!({"user": {"name": "Ada", "age": 36}, "tags": ["x", "y"]});
        let mapping = json!({"n": "$.user.name", "first_tag": "$.tags[0]"})
            .as_object()
            .unwrap()
            .clone();
        let result = reshape_json(&data, &mapping).expect("mapping must be valid");
        assert_eq!(result, json!({"n": "Ada", "first_tag": "x"}));
    }

    /// A path in the mapping that has no match produces null for that key.
    #[cfg(feature = "jsonpath")]
    #[test]
    fn reshape_no_match_produces_null_for_key() {
        let data = json!({"user": {"name": "Ada"}});
        let mapping = json!({"missing": "$.nonexistent.field"})
            .as_object()
            .unwrap()
            .clone();
        let result = reshape_json(&data, &mapping).expect("mapping must be valid");
        assert_eq!(result, json!({"missing": null}));
    }

    /// A mapping value that is not a valid JSONPath string returns Err(InvalidPath).
    #[cfg(feature = "jsonpath")]
    #[test]
    fn reshape_invalid_jsonpath_string_returns_error() {
        let data = json!({"a": 1});
        let mapping = json!({"out": "$["}).as_object().unwrap().clone();
        let result = reshape_json(&data, &mapping);
        assert!(
            matches!(result, Err(TransformError::InvalidPath(_))),
            "expected Err(InvalidPath) for bad JSONPath, got: {result:?}"
        );
    }

    /// A mapping value that is not a string (e.g. a number) returns Err(InvalidPath).
    #[cfg(feature = "jsonpath")]
    #[test]
    fn reshape_non_string_mapping_value_returns_error() {
        let data = json!({"a": 1});
        let mapping = json!({"out": 42}).as_object().unwrap().clone();
        let result = reshape_json(&data, &mapping);
        assert!(
            matches!(result, Err(TransformError::InvalidPath(_))),
            "expected Err(InvalidPath) for non-string mapping value, got: {result:?}"
        );
    }
}
